//! Closed enrollment wire and listener custody. This slice has no live backend.
use super::{BoundVoters, ClusterId, CodeId, JoinAnchor, NodeId, RequestId};
use crate::cluster::{pki::preproof::Refused, state_store::LogPos};
use rustls::{
    HandshakeKind, ServerConfig,
    crypto::ring,
    pki_types::{CertificateDer, PrivateKeyDer},
    server::NoServerSessionStorage,
    time_provider::TimeProvider,
};
use std::{
    future::Future,
    io,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf},
    net::{TcpListener, TcpStream},
    sync::{OwnedSemaphorePermit, Semaphore, watch},
    task::{JoinHandle, JoinSet},
    time::{Instant, timeout, timeout_at},
};
use tokio_rustls::{TlsAcceptor, server::TlsStream};

const SESSIONS: usize = 8;
const HANDSHAKES: usize = 2;
const CIPHERTEXT: usize = 128 * 1024;
const MAX_DER: usize = 4096;
pub(in crate::cluster) const SOCKET_BUDGET: Duration = Duration::from_secs(12);
const WORK_BUDGET: Duration = Duration::from_secs(5);
const HEADER: usize = 8;
type Challenge = [u8; 16];

#[derive(Clone, Copy)]
enum Kind {
    Query = 1,
    Anchor = 2,
    Redeem = 3,
    Result = 4,
}

impl Kind {
    fn admits(self, len: usize) -> bool {
        match self {
            Self::Query => len == 32,
            Self::Anchor => len == 1 || (58..=658).contains(&len),
            Self::Redeem => (131..=4226).contains(&len),
            Self::Result => len == 1 || (36..=4131).contains(&len),
        }
    }
}

async fn read_frame(reader: &mut (impl AsyncRead + Unpin), kind: Kind) -> Result<Vec<u8>, Refused> {
    let mut header = [0; HEADER];
    reader.read_exact(&mut header).await.map_err(|_| Refused)?;
    let len = usize::from(u16::from_be_bytes([header[6], header[7]]));
    if &header[..5] != b"ARXE\x01" || header[5] != kind as u8 || !kind.admits(len) {
        return Err(Refused);
    }
    let mut body = vec![0; len];
    reader.read_exact(&mut body).await.map_err(|_| Refused)?;
    Ok(body)
}

fn frame(kind: Kind, body: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(HEADER + body.len());
    bytes.extend(b"ARXE\x01");
    bytes.push(kind as u8);
    bytes.extend(
        u16::try_from(body.len())
            .expect("bounded frame")
            .to_be_bytes(),
    );
    bytes.extend(body);
    bytes
}

async fn clean_end(reader: &mut (impl AsyncRead + Unpin)) -> Result<(), Refused> {
    // tokio-rustls returns UnexpectedEof for a TCP EOF without close_notify.
    match reader.read(&mut [0]).await {
        Ok(0) => Ok(()),
        _ => Err(Refused),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Query {
    cluster: ClusterId,
    code: CodeId,
}

impl Query {
    fn parse(body: &[u8]) -> Self {
        Self {
            cluster: field(body, 0),
            code: field(body, 16),
        }
    }

    fn frame(self) -> Vec<u8> {
        let mut body = [0; 32];
        body[..16].copy_from_slice(&self.cluster);
        body[16..].copy_from_slice(&self.code);
        frame(Kind::Query, &body)
    }
}

pub(in crate::cluster) fn query_frame(cluster: ClusterId, code: CodeId) -> Vec<u8> {
    Query { cluster, code }.frame()
}

pub(in crate::cluster) async fn receive_anchor(
    reader: &mut (impl AsyncRead + Unpin),
) -> Result<(Challenge, BoundVoters), Refused> {
    let body = read_frame(reader, Kind::Anchor).await?;
    if body[0] != 0 || body.len() == 1 {
        return Err(Refused);
    }
    Ok((
        field(&body, 1),
        BoundVoters::decode(&body[17..]).map_err(|_| Refused)?,
    ))
}

struct Continuation {
    query: Query,
    anchor: JoinAnchor,
    challenge: Challenge,
}

impl Continuation {
    fn accept(self, body: Vec<u8>) -> Result<Redeem, Refused> {
        let request = field(&body, 0);
        if request == [0; 16]
            || Query::parse(&body[16..]) != self.query
            || &body[48..80] != self.anchor.as_bytes()
            || body[80..96] != self.challenge
        {
            return Err(Refused);
        }
        Ok(Redeem {
            request,
            query: self.query,
            anchor: self.anchor,
            proof: field(&body, 96),
            csr: der_field(&body, 128)?.to_vec(),
        })
    }
}

struct Redeem {
    request: RequestId,
    query: Query,
    anchor: JoinAnchor,
    proof: [u8; 32],
    csr: Vec<u8>,
}

pub(in crate::cluster) fn redeem_frame(
    code: &super::JoinCode,
    request: RequestId,
    challenge: Challenge,
    csr: &[u8],
) -> Result<Vec<u8>, Refused> {
    if request == [0; 16] || !(1..=MAX_DER).contains(&csr.len()) {
        return Err(Refused);
    }
    let mut body = Vec::with_capacity(130 + csr.len());
    body.extend(request);
    body.extend(code.cluster());
    body.extend(code.code_id());
    body.extend(code.anchor().as_bytes());
    body.extend(challenge);
    body.extend(code.proof().bytes());
    append_der(&mut body, csr);
    Ok(frame(Kind::Redeem, &body))
}

#[derive(PartialEq, Eq)]
struct Reply {
    node: NodeId,
    original: LogPos,
    leaf: Vec<u8>,
}

impl Reply {
    fn decode(body: &[u8]) -> Result<Self, Refused> {
        if body[0] != 0 || body.len() == 1 {
            return Err(Refused);
        }
        let node = u64::from_be_bytes(field(body, 1));
        let original = LogPos {
            term: u64::from_be_bytes(field(body, 9)),
            leader: u64::from_be_bytes(field(body, 17)),
            index: u64::from_be_bytes(field(body, 25)),
        };
        let leaf = der_field(body, 33)?;
        let node = NodeId::new(node)
            .filter(|node| node.get() >= 2)
            .ok_or(Refused)?;
        if original.index > i64::MAX as u64 {
            return Err(Refused);
        }
        Ok(Self {
            node,
            original,
            leaf: leaf.to_vec(),
        })
    }

    fn frame(&self) -> Result<Vec<u8>, Refused> {
        if self.node.get() < 2
            || self.original.index > i64::MAX as u64
            || !(1..=MAX_DER).contains(&self.leaf.len())
        {
            return Err(Refused);
        }
        let mut body = Vec::with_capacity(35 + self.leaf.len());
        body.push(0);
        body.extend(self.node.get().to_be_bytes());
        body.extend(self.original.term.to_be_bytes());
        body.extend(self.original.leader.to_be_bytes());
        body.extend(self.original.index.to_be_bytes());
        append_der(&mut body, &self.leaf);
        Ok(frame(Kind::Result, &body))
    }
}

/// Protocol data only. The original voter list comes from the authenticated exchange.
pub(in crate::cluster) struct Completed {
    voters: BoundVoters,
    reply: Reply,
}

pub(in crate::cluster) async fn receive_result(
    reader: &mut (impl AsyncRead + Unpin),
    voters: BoundVoters,
) -> Result<Completed, Refused> {
    let body = read_frame(reader, Kind::Result).await?;
    let reply = Reply::decode(&body)?;
    clean_end(reader).await?;
    Ok(Completed { voters, reply })
}

fn field<const N: usize>(body: &[u8], offset: usize) -> [u8; N] {
    body[offset..offset + N]
        .try_into()
        .expect("checked frame length")
}

fn der_field(body: &[u8], offset: usize) -> Result<&[u8], Refused> {
    let len = usize::from(u16::from_be_bytes(field(body, offset)));
    let der = &body[offset + 2..];
    if !(1..=MAX_DER).contains(&len) || der.len() != len {
        return Err(Refused);
    }
    Ok(der)
}

fn append_der(body: &mut Vec<u8>, der: &[u8]) {
    body.extend(u16::try_from(der.len()).expect("bounded DER").to_be_bytes());
    body.extend(der);
}

#[derive(Clone)]
pub(in crate::cluster) struct SocketLimits {
    incoming: usize,
    outgoing: usize,
    chunk: usize,
    #[cfg(test)]
    write_fault: Option<Arc<tests::WriteFault>>,
}

impl Default for SocketLimits {
    fn default() -> Self {
        Self {
            incoming: CIPHERTEXT,
            outgoing: CIPHERTEXT,
            chunk: 4096,
            #[cfg(test)]
            write_fault: None,
        }
    }
}

/// Count actual ciphertext below TLS, including its handshake and close alerts.
pub(in crate::cluster) struct QuotaSocket {
    tcp: TcpStream,
    limits: SocketLimits,
    deadline: Instant,
}

impl QuotaSocket {
    pub(in crate::cluster) fn new(tcp: TcpStream, limits: SocketLimits, deadline: Instant) -> Self {
        Self {
            tcp,
            limits,
            deadline,
        }
    }
}

impl AsyncRead for QuotaSocket {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        output: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if Instant::now() >= self.deadline {
            return Poll::Ready(Err(io::ErrorKind::TimedOut.into()));
        }
        if output.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        let limit = self
            .limits
            .incoming
            .min(self.limits.chunk)
            .min(output.remaining());
        if limit == 0 {
            return Poll::Ready(Err(io::Error::other("enrollment ciphertext read cap")));
        }
        let mut bounded = ReadBuf::new(output.initialize_unfilled_to(limit));
        std::task::ready!(Pin::new(&mut self.tcp).poll_read(cx, &mut bounded))?;
        let read = bounded.filled().len();
        self.limits.incoming -= read;
        output.advance(read);
        Poll::Ready(Ok(()))
    }
}

impl AsyncWrite for QuotaSocket {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        if Instant::now() >= self.deadline {
            return Poll::Ready(Err(io::ErrorKind::TimedOut.into()));
        }
        if bytes.is_empty() {
            return Poll::Ready(Ok(0));
        }
        let limit = bytes.len().min(self.limits.outgoing).min(self.limits.chunk);
        if limit == 0 {
            return Poll::Ready(Err(io::Error::other("enrollment ciphertext write cap")));
        }
        #[cfg(test)]
        let limit = if let Some(fault) = &self.limits.write_fault {
            limit.min(fault.limit()?)
        } else {
            limit
        };
        let wrote = std::task::ready!(Pin::new(&mut self.tcp).poll_write(cx, &bytes[..limit]))?;
        self.limits.outgoing -= wrote;
        #[cfg(test)]
        if let Some(fault) = &self.limits.write_fault {
            fault.wrote(wrote);
        }
        Poll::Ready(Ok(wrote))
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if Instant::now() >= self.deadline {
            return Poll::Ready(Err(io::ErrorKind::TimedOut.into()));
        }
        Pin::new(&mut self.tcp).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if Instant::now() >= self.deadline {
            return Poll::Ready(Err(io::ErrorKind::TimedOut.into()));
        }
        Pin::new(&mut self.tcp).poll_shutdown(cx)
    }
}

fn server_config(
    leaf: CertificateDer<'static>,
    key: PrivateKeyDer<'static>,
    time: Arc<dyn TimeProvider>,
) -> Result<ServerConfig, Refused> {
    if leaf.is_empty() || leaf.len() > MAX_DER {
        return Err(Refused);
    }
    let mut config = ServerConfig::builder_with_details(Arc::new(ring::default_provider()), time)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|_| Refused)?
        .with_no_client_auth()
        .with_single_cert(vec![leaf], key)
        .map_err(|_| Refused)?;
    config.session_storage = Arc::new(NoServerSessionStorage {});
    config.send_tls13_tickets = 0;
    config.max_early_data_size = 0;
    Ok(config)
}

/// Both operations must finish within their admitted budget. The owner joins even
/// if a backend violates that contract, because expiry cannot prove cancellation.
trait Backend: Send + Sync + 'static {
    fn anchor(
        &self,
        query: Query,
        deadline: Instant,
    ) -> impl Future<Output = Result<BoundVoters, Refused>> + Send;

    fn redeem(
        &self,
        request: Redeem,
        deadline: Instant,
    ) -> impl Future<Output = Result<Reply, Refused>> + Send;
}

struct Control {
    admission: Mutex<()>,
    closing: watch::Sender<bool>,
    sessions: Arc<Semaphore>,
    handshakes: Arc<Semaphore>,
}

impl Control {
    fn stop(&self) {
        let _gate = self.admission.lock().unwrap();
        self.closing.send_replace(true);
    }

    fn open(&self, deadline: Instant) -> Result<std::sync::MutexGuard<'_, ()>, Refused> {
        let gate = self.admission.lock().map_err(|_| Refused)?;
        if *self.closing.borrow() || Instant::now() >= deadline {
            return Err(Refused);
        }
        Ok(gate)
    }

    fn admit<T, F>(
        &self,
        socket_deadline: Instant,
        work: impl FnOnce(Instant) -> F,
    ) -> Result<JoinHandle<T>, Refused>
    where
        T: Send + 'static,
        F: Future<Output = T> + Send + 'static,
    {
        let _gate = self.open(socket_deadline)?;
        let deadline = Instant::now() + WORK_BUDGET;
        if deadline > socket_deadline {
            return Err(Refused);
        }
        // Register ownership under the same gate that latches close.
        Ok(tokio::spawn(work(deadline)))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Close {
    Joined,
    Unconfirmed,
}

struct ListenerOwner {
    control: Arc<Control>,
    supervisor: JoinHandle<Close>,
    outcome: Option<Close>,
}

impl ListenerOwner {
    fn start(
        listener: TcpListener,
        leaf: CertificateDer<'static>,
        key: PrivateKeyDer<'static>,
        time: Arc<dyn TimeProvider>,
        backend: impl Backend,
    ) -> Result<Self, Refused> {
        let config = server_config(leaf, key, time)?;
        let control = Arc::new(Control {
            admission: Mutex::new(()),
            closing: watch::channel(false).0,
            sessions: Arc::new(Semaphore::new(SESSIONS)),
            handshakes: Arc::new(Semaphore::new(HANDSHAKES)),
        });
        let supervisor = tokio::spawn(supervise(
            listener,
            TlsAcceptor::from(Arc::new(config)),
            Arc::new(backend),
            control.clone(),
        ));
        Ok(Self {
            control,
            supervisor,
            outcome: None,
        })
    }

    async fn close(&mut self, observation: Duration) -> Close {
        self.control.stop();
        if let Some(outcome) = self.outcome {
            return outcome;
        }
        match timeout(observation, &mut self.supervisor).await {
            Ok(outcome) => {
                let outcome = outcome.unwrap_or(Close::Unconfirmed);
                self.outcome = Some(outcome);
                outcome
            }
            Err(_) => Close::Unconfirmed,
        }
    }
}

impl Drop for ListenerOwner {
    fn drop(&mut self) {
        // Detaching the supervisor leaves it running and owning all its joins.
        self.control.stop();
    }
}

async fn supervise(
    listener: TcpListener,
    acceptor: TlsAcceptor,
    backend: Arc<impl Backend>,
    control: Arc<Control>,
) -> Close {
    let mut closing = control.closing.subscribe();
    let mut connections = JoinSet::new();
    let mut outcome = Close::Joined;
    loop {
        tokio::select! {
            biased;
            _ = closing.wait_for(|closed| *closed) => break,
            Some(join) = connections.join_next(), if !connections.is_empty() => {
                if !matches!(join, Ok((Ok(()) | Err(SessionError::Refused), _))) {
                    outcome = Close::Unconfirmed;
                }
            }
            incoming = listener.accept() => {
                let Ok((tcp, _)) = incoming else {
                    control.stop();
                    break;
                };
                let deadline = Instant::now() + SOCKET_BUDGET;
                let Ok(_gate) = control.open(deadline) else { continue };
                let Ok(session) = control.sessions.clone().try_acquire_owned() else { continue };
                let Ok(handshake) = control.handshakes.clone().try_acquire_owned() else { continue };
                let acceptor = acceptor.clone();
                let backend = backend.clone();
                let control = control.clone();
                connections.spawn(async move {
                    let result = serve(tcp, acceptor, backend, control, handshake, deadline).await;
                    (result, session)
                });
            }
        }
    }
    drop(listener);
    while let Some(join) = connections.join_next().await {
        if !matches!(join, Ok((Ok(()) | Err(SessionError::Refused), _))) {
            outcome = Close::Unconfirmed;
        }
    }
    outcome
}

#[derive(Clone, Copy)]
enum SessionError {
    Refused,
    WorkLost,
}

impl From<Refused> for SessionError {
    fn from(_: Refused) -> Self {
        Self::Refused
    }
}

async fn before_work<T>(
    control: &Control,
    deadline: Instant,
    io: impl Future<Output = Result<T, Refused>>,
) -> Result<T, Refused> {
    let mut closing = control.closing.subscribe();
    tokio::select! {
        biased;
        _ = closing.wait_for(|closed| *closed) => Err(Refused),
        result = timeout_at(deadline, io) => result.map_err(|_| Refused)?,
    }
}

async fn join_work<T>(
    socket: TlsStream<QuotaSocket>,
    deadline: Instant,
    mut work: JoinHandle<T>,
) -> Result<(TlsStream<QuotaSocket>, T), SessionError> {
    match timeout_at(deadline, &mut work).await {
        Ok(Ok(result)) if Instant::now() < deadline => Ok((socket, result)),
        Ok(Ok(_)) => Err(SessionError::Refused),
        Ok(Err(_)) => Err(SessionError::WorkLost),
        Err(_) => {
            drop(socket);
            work.await.map_err(|_| SessionError::WorkLost)?;
            Err(SessionError::Refused)
        }
    }
}

async fn send_terminal(
    mut socket: TlsStream<QuotaSocket>,
    deadline: Instant,
    bytes: &[u8],
) -> Result<(), Refused> {
    timeout_at(deadline, async {
        socket.write_all(bytes).await?;
        socket.shutdown().await
    })
    .await
    .map_err(|_| Refused)?
    .map_err(|_| Refused)
}

async fn serve(
    tcp: TcpStream,
    acceptor: TlsAcceptor,
    backend: Arc<impl Backend>,
    control: Arc<Control>,
    handshake: OwnedSemaphorePermit,
    deadline: Instant,
) -> Result<(), SessionError> {
    let socket = before_work(&control, deadline, async {
        tcp.set_nodelay(true).map_err(|_| Refused)?;
        acceptor
            .accept(QuotaSocket::new(tcp, SocketLimits::default(), deadline))
            .await
            .map_err(|_| Refused)
    })
    .await;
    drop(handshake);
    let mut socket = socket?;
    if socket.get_ref().1.handshake_kind() != Some(HandshakeKind::Full)
        || socket.get_ref().1.peer_certificates().is_some()
    {
        return Err(Refused.into());
    }

    let query = before_work(&control, deadline, read_frame(&mut socket, Kind::Query)).await?;
    let query = Query::parse(&query);
    let anchor_backend = backend.clone();
    let work = control.admit(deadline, move |until| async move {
        anchor_backend.anchor(query, until).await
    })?;
    let (mut socket, voters) = join_work(socket, deadline, work).await?;
    let Ok(voters) = voters else {
        return Ok(send_terminal(socket, deadline, &frame(Kind::Anchor, &[1])).await?);
    };
    let mut challenge = [0; 16];
    getrandom::fill(&mut challenge).map_err(|_| Refused)?;
    let continuation = Continuation {
        query,
        anchor: voters.anchor(query.cluster),
        challenge,
    };
    let mut body = Vec::with_capacity(658);
    body.push(0);
    body.extend(challenge);
    body.extend(voters.encode());
    before_work(&control, deadline, async {
        socket
            .write_all(&frame(Kind::Anchor, &body))
            .await
            .map_err(|_| Refused)?;
        socket.flush().await.map_err(|_| Refused)
    })
    .await?;
    let request = before_work(&control, deadline, async {
        let request = continuation.accept(read_frame(&mut socket, Kind::Redeem).await?)?;
        clean_end(&mut socket).await?;
        Ok(request)
    })
    .await?;
    let work = control.admit(deadline, move |until| async move {
        backend.redeem(request, until).await
    })?;
    let (socket, reply) = join_work(socket, deadline, work).await?;
    let reply = reply
        .and_then(|reply| reply.frame())
        .unwrap_or_else(|_| frame(Kind::Result, &[1]));
    Ok(send_terminal(socket, deadline, &reply).await?)
}

#[cfg(test)]
mod tests;
