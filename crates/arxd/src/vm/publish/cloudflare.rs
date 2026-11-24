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

/// What one DNS mutation may have handed to the network.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::vm) enum Sent {
    /// The underlying transport accepted no request byte.
    Never,
    /// The underlying transport accepted request bytes. Never reclassified.
    Possibly,
}

#[derive(Default)]
struct GateState {
    revoked: bool,
    possibly: bool,
}

/// The first-write rule, applied while the caller holds its send authority.
/// Once `possibly` is set, writes pass straight through. Otherwise the write
/// runs only while `open` holds: `Pending` and any zero-progress result leave
/// the request unsent and revocable, and only an accepted byte sets
/// `possibly`, which is never cleared.
pub(in crate::vm) fn arbitrate(
    possibly: &mut bool,
    open: impl FnOnce() -> bool,
    write: &mut dyn FnMut() -> std::task::Poll<io::Result<usize>>,
) -> std::task::Poll<io::Result<usize>> {
    if *possibly {
        return write();
    }
    if !open() {
        return std::task::Poll::Ready(Err(io::Error::other("submission withheld")));
    }
    let result = write();
    if matches!(result, std::task::Poll::Ready(Ok(accepted)) if accepted > 0) {
        *possibly = true;
    }
    result
}

/// The authority that decides one mutation's first underlying write. Every
/// cancellation source it honors must take the same lock as
/// [`SendAuthority::first_write`], so each one either precedes a first write
/// (and withholds it) or follows it (and finds the request possibly sent).
pub(in crate::vm) trait SendAuthority: Sync {
    /// One attempt of the underlying write, under the authority, by
    /// [`arbitrate`].
    fn first_write(
        &self,
        write: &mut dyn FnMut() -> std::task::Poll<io::Result<usize>>,
    ) -> std::task::Poll<io::Result<usize>>;
    fn sent(&self) -> Sent;
}

/// A standalone first-write authority with its own lock, for transport
/// fixtures and requests outside an activation.
#[derive(Default)]
pub(in crate::vm) struct SendGate(std::sync::Mutex<GateState>);

impl SendGate {
    fn state(&self) -> std::sync::MutexGuard<'_, GateState> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Revocation. Returns `true` when no byte was accepted, in which case
    /// none can be.
    #[allow(dead_code, reason = "used by transport fixtures")]
    pub(in crate::vm) fn revoke(&self) -> bool {
        let mut state = self.state();
        state.revoked = true;
        !state.possibly
    }

    pub(in crate::vm) fn sent(&self) -> Sent {
        if self.state().possibly {
            Sent::Possibly
        } else {
            Sent::Never
        }
    }
}

/// One DNS mutation's send authority.
pub(in crate::vm) struct Submission<'a>(Authority<'a>);

enum Authority<'a> {
    /// A standalone gate plus other cancellation sampled under its lock.
    Gate(Arc<SendGate>, &'a (dyn Fn() -> bool + Sync)),
    /// An activation's authority, shared with its revocation, shutdown seal
    /// and origin retirement.
    Shared(&'a dyn SendAuthority),
}

impl<'a> Submission<'a> {
    pub(in crate::vm) fn new(
        gate: Arc<SendGate>,
        cancelled: &'a (dyn Fn() -> bool + Sync),
    ) -> Self {
        Self(Authority::Gate(gate, cancelled))
    }

    pub(in crate::vm) fn with(authority: &'a dyn SendAuthority) -> Self {
        Self(Authority::Shared(authority))
    }

    /// A submission no cancellation can withhold.
    #[allow(dead_code, reason = "used by transport fixtures")]
    pub(in crate::vm) fn unconditional() -> Submission<'static> {
        fn never() -> bool {
            false
        }
        Submission::new(Arc::default(), &never)
    }

    pub(in crate::vm) fn sent(&self) -> Sent {
        match &self.0 {
            Authority::Gate(gate, _) => gate.sent(),
            Authority::Shared(authority) => authority.sent(),
        }
    }

    /// Arbitrates one attempt of the first underlying write under the
    /// authority. Cancellation is re-sampled on every attempt; see
    /// [`arbitrate`].
    pub(in crate::vm) fn first_write(
        &self,
        write: impl FnOnce() -> std::task::Poll<io::Result<usize>>,
    ) -> std::task::Poll<io::Result<usize>> {
        let mut write = Some(write);
        let mut once = || {
            write
                .take()
                .map_or(std::task::Poll::Pending, |write| write())
        };
        match &self.0 {
            Authority::Gate(gate, cancelled) => {
                let mut state = gate.state();
                let revoked = state.revoked;
                arbitrate(&mut state.possibly, || !revoked && !cancelled(), &mut once)
            }
            Authority::Shared(authority) => authority.first_write(&mut once),
        }
    }
}

/// The transport below TLS. Once armed, every write goes through the
/// submission's first-write authority; before arming (TLS handshake) and for
/// requests without a submission it passes through.
struct GatedIo<'s, 'c, S> {
    inner: S,
    submission: Option<&'s Submission<'c>>,
    armed: bool,
}

impl<S: AsyncRead + Unpin> AsyncRead for GatedIo<'_, '_, S> {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        std::pin::Pin::new(&mut self.get_mut().inner).poll_read(cx, buf)
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for GatedIo<'_, '_, S> {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<io::Result<usize>> {
        let this = self.get_mut();
        let inner = &mut this.inner;
        match this.submission.filter(|_| this.armed) {
            Some(submission) => {
                submission.first_write(|| std::pin::Pin::new(inner).poll_write(cx, buf))
            }
            None => std::pin::Pin::new(inner).poll_write(cx, buf),
        }
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        std::pin::Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        std::pin::Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}

/// TLS over a gated transport. The handshake completes and its output is
/// flushed before the gate arms, so the first gated write carries the
/// request's application data. Early data is off in this client
/// configuration.
async fn tls_gated<'s, 'c, S: AsyncRead + AsyncWrite + Unpin>(
    tls: &TlsConnector,
    io: S,
    submission: Option<&'s Submission<'c>>,
) -> io::Result<tokio_rustls::client::TlsStream<GatedIo<'s, 'c, S>>> {
    let name = ServerName::try_from(API_HOST).expect("fixed TLS server name");
    let gated = GatedIo {
        inner: io,
        submission,
        armed: false,
    };
    let mut stream = tls.connect(name, gated).await?;
    stream.flush().await?;
    stream.get_mut().0.armed = true;
    Ok(stream)
}

pub(in crate::vm) trait DnsApi {
    async fn preflight(&self, route: &Route) -> Result<(), DnsError>;
    /// Returns the validated, owned record ID acknowledged by the single POST.
    /// The caller persists it before [`DnsApi::verify`], so a failed
    /// verification never discards an acknowledged ID. The first request byte
    /// is written only through `submission`'s first-write authority.
    async fn create(
        &self,
        route: &Route,
        submission: &Submission<'_>,
    ) -> Result<RecordId, DnsError>;
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
    async fn create(
        &self,
        route: &Route,
        submission: &Submission<'_>,
    ) -> Result<RecordId, DnsError> {
        let body = serde_json::to_vec(&Create {
            kind: "CNAME",
            name: route.name.as_str(),
            content: &route.target,
            comment: &route.comment,
            proxied: true,
            ttl: 1,
        })
        .map_err(|_| DnsError::InvalidResponse)?;
        let reply = self
            .request(Method::Post, &route.records(), &body, Some(submission))
            .await?;
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
        let response = self.request(Method::Get, &path, &[], None).await?;
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
        let reply = self.request(Method::Get, &path, &[], None).await?;
        if (200..300).contains(&reply.status) {
            let record: Record = reply.result()?;
            if id.as_str() != record.id || !route.owned(&record) {
                return Ok(Removal::ForeignOrAmbiguous);
            }
            let deleted: Deleted = self
                .request(Method::Delete, &path, &[], None)
                .await?
                .result()?;
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
    /// A mutation's first request byte passes its `submission`'s first-write
    /// authority, below TLS and after connection setup.
    async fn request(
        &self,
        method: Method,
        path: &str,
        body: &[u8],
        submission: Option<&Submission<'_>>,
    ) -> Result<Response, DnsError> {
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
                    exchange(tls_gated(tls, stream, submission).await?, &headers, body).await
                }
                #[cfg(test)]
                Transport::Fixture(addr) => {
                    let gated = GatedIo {
                        inner: TcpStream::connect(addr).await?,
                        submission,
                        armed: true,
                    };
                    exchange(gated, &headers, body).await
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

#[cfg(test)]
mod first_write {
    //! The first underlying write decides DNS evidence. A controlled transport
    //! holds or fails writes, and records every byte it accepts.
    use std::{
        future::Future,
        pin::Pin,
        sync::{
            Mutex,
            atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst},
        },
        task::{Context, Poll, Waker},
    };

    use super::*;

    /// Holds every write at `Pending` while `hold` is set (counting each held
    /// attempt), fails the next write with zero progress while `fail` is set,
    /// and logs accepted bytes.
    struct Controlled<S> {
        inner: S,
        hold: Arc<AtomicBool>,
        held: Arc<AtomicUsize>,
        fail: Arc<AtomicBool>,
        accepted: Arc<Mutex<Vec<u8>>>,
    }

    impl<S> Controlled<S> {
        fn new(inner: S) -> Self {
            Self {
                inner,
                hold: Arc::default(),
                held: Arc::default(),
                fail: Arc::default(),
                accepted: Arc::default(),
            }
        }
    }

    impl<S: AsyncRead + Unpin> AsyncRead for Controlled<S> {
        fn poll_read(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &mut tokio::io::ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            Pin::new(&mut self.get_mut().inner).poll_read(cx, buf)
        }
    }

    impl<S: AsyncWrite + Unpin> AsyncWrite for Controlled<S> {
        fn poll_write(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<io::Result<usize>> {
            let this = self.get_mut();
            if this.hold.load(SeqCst) {
                this.held.fetch_add(1, SeqCst);
                return Poll::Pending;
            }
            if this.fail.swap(false, SeqCst) {
                return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
            }
            let written = Pin::new(&mut this.inner).poll_write(cx, buf);
            if let Poll::Ready(Ok(accepted)) = written {
                this.accepted
                    .lock()
                    .unwrap()
                    .extend_from_slice(&buf[..accepted]);
            }
            written
        }

        fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Pin::new(&mut self.get_mut().inner).poll_flush(cx)
        }

        fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
        }
    }

    fn never() -> bool {
        false
    }

    fn poll_once<F: Future>(future: Pin<&mut F>) -> Poll<F::Output> {
        future.poll(&mut Context::from_waker(Waker::noop()))
    }

    #[tokio::test]
    async fn a_pending_first_write_stays_revocable_and_sends_nothing() {
        let (client, _server) = tokio::io::duplex(4096);
        let io = Controlled::new(client);
        let (hold, accepted) = (io.hold.clone(), io.accepted.clone());
        hold.store(true, SeqCst);
        let gate = Arc::new(SendGate::default());
        let submission = Submission::new(gate.clone(), &never);
        let gated = GatedIo {
            inner: io,
            submission: Some(&submission),
            armed: true,
        };
        let mut request = Box::pin(exchange(gated, "POST / HTTP/1.1\r\n\r\n", b"{}"));
        assert!(poll_once(request.as_mut()).is_pending());
        assert_eq!(gate.sent(), Sent::Never);
        assert!(gate.revoke(), "a pending first write must stay revocable");
        hold.store(false, SeqCst);
        assert!(request.await.is_err());
        assert_eq!(gate.sent(), Sent::Never);
        assert!(
            accepted.lock().unwrap().is_empty(),
            "a byte followed revocation"
        );
    }

    #[tokio::test]
    async fn a_zero_byte_first_write_failure_is_never_sent() {
        let (client, _server) = tokio::io::duplex(4096);
        let io = Controlled::new(client);
        io.fail.store(true, SeqCst);
        let accepted = io.accepted.clone();
        let gate = Arc::new(SendGate::default());
        let submission = Submission::new(gate.clone(), &never);
        let gated = GatedIo {
            inner: io,
            submission: Some(&submission),
            armed: true,
        };
        assert!(
            exchange(gated, "POST / HTTP/1.1\r\n\r\n", b"{}")
                .await
                .is_err()
        );
        assert_eq!(gate.sent(), Sent::Never);
        assert!(accepted.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn an_accepted_first_byte_is_never_reclassified() {
        let (client, server) = tokio::io::duplex(4096);
        let io = Controlled::new(client);
        let accepted = io.accepted.clone();
        let gate = Arc::new(SendGate::default());
        let submission = Submission::new(gate.clone(), &never);
        let gated = GatedIo {
            inner: io,
            submission: Some(&submission),
            armed: true,
        };
        let mut request = Box::pin(exchange(gated, "POST / HTTP/1.1\r\n\r\n", b"{}"));
        // The request is written and now waits for the response.
        assert!(poll_once(request.as_mut()).is_pending());
        assert_eq!(gate.sent(), Sent::Possibly);
        assert!(!gate.revoke(), "a revocation after a byte must know it");
        assert_eq!(gate.sent(), Sent::Possibly);
        drop(server);
        let _ = request.await;
        assert!(accepted.lock().unwrap().starts_with(b"POST "));
    }

    /// TLS buffers the request's application data while the socket below it is
    /// held at `Pending`; a revocation then sends no application byte at all.
    #[tokio::test]
    async fn buffered_tls_application_data_waits_on_the_first_write() {
        use rcgen::{BasicConstraints, CertificateParams, CertifiedIssuer, IsCa, KeyPair};
        use rustls::pki_types::PrivatePkcs8KeyDer;
        let provider = || Arc::new(rustls::crypto::ring::default_provider());
        let mut params = CertificateParams::default();
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        let ca = CertifiedIssuer::self_signed(params, KeyPair::generate().unwrap()).unwrap();
        let key = KeyPair::generate().unwrap();
        let leaf = CertificateParams::new(vec![API_HOST.to_owned()])
            .unwrap()
            .signed_by(&key, &ca)
            .unwrap();
        let server = rustls::ServerConfig::builder_with_provider(provider())
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                vec![leaf.der().clone()],
                PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
            )
            .unwrap();
        let mut roots = RootCertStore::empty();
        roots.add(ca.der().clone()).unwrap();
        let client = ClientConfig::builder_with_provider(provider())
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_root_certificates(roots)
            .with_no_client_auth();
        let (client_io, server_io) = tokio::io::duplex(64 * 1024);
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(server));
        let connector = TlsConnector::from(Arc::new(client));
        let io = Controlled::new(client_io);
        let (hold, held, accepted) = (io.hold.clone(), io.held.clone(), io.accepted.clone());
        let gate = Arc::new(SendGate::default());
        let submission = Submission::new(gate.clone(), &never);
        // Both sides finish the handshake together while both duplex halves
        // live, so the server's post-handshake TLS 1.3 tickets always have a
        // writable peer. The server stream stays open through cancellation.
        let (stream, mut peer) = tokio::time::timeout(Duration::from_secs(5), async {
            tokio::try_join!(
                tls_gated(&connector, io, Some(&submission)),
                acceptor.accept(server_io)
            )
        })
        .await
        .expect("TLS handshake exceeded its bound")
        .expect("TLS handshake failed");
        // Everything the socket accepted so far is handshake output.
        let handshake = accepted.lock().unwrap().len();
        assert!(handshake > 0, "the handshake wrote nothing");
        // The socket now holds every write.
        hold.store(true, SeqCst);
        let mut request = Box::pin(exchange(
            stream,
            "POST / HTTP/1.1

",
            b"{}",
        ));
        assert!(poll_once(request.as_mut()).is_pending());
        assert!(
            held.load(SeqCst) > 0,
            "no post-handshake ciphertext write reached the held socket"
        );
        assert_eq!(gate.sent(), Sent::Never);
        assert!(gate.revoke(), "a held first write must stay revocable");
        hold.store(false, SeqCst);
        assert!(request.await.is_err());
        assert_eq!(gate.sent(), Sent::Never);
        assert_eq!(
            accepted.lock().unwrap().len(),
            handshake,
            "application data followed revocation"
        );
        // The client stream is gone. The peer sees only the abrupt end of the
        // connection, which is expected here, and no application byte.
        let mut received = Vec::new();
        let _ = tokio::time::timeout(Duration::from_secs(5), peer.read_to_end(&mut received))
            .await
            .expect("peer read exceeded its bound");
        assert!(received.is_empty(), "application data followed revocation");
    }
}
