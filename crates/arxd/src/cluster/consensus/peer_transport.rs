//! Private one-exchange Append transport. No factory, engine or service startup wiring.
use super::{Arx, Id, SmAdapter, peer_registry, peer_wire};
use crate::cluster::{
    enrollment::{ClusterId, NodeId},
    pki::verify::{self, AuthenticatedPeer, PeerVerifier, Registry},
};
use openraft::{
    errors::{RPCError, Unreachable},
    network::{NetAppend, RPCOption},
    raft::{AppendEntriesRequest, AppendError, AppendResult},
};
use rustls::{
    ClientConfig, ServerConfig,
    pki_types::{CertificateDer, PrivateKeyDer, ServerName},
    time_provider::TimeProvider,
};
use std::{
    cmp::Ordering,
    future::{Future, poll_fn},
    net::SocketAddr,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Poll, ready},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{OwnedSemaphorePermit, Semaphore, oneshot, watch},
    task::{JoinError, JoinHandle, JoinSet},
    time::{Instant, timeout_at},
};
use tokio_rustls::{TlsAcceptor, TlsConnector};

const CONNECTIONS: usize = 32;
const HANDSHAKES: usize = 4;
const INBOUND_BUDGET: Duration = Duration::from_secs(5);
const CLOSE_BUDGET: Duration = Duration::from_secs(5);
const SEND_BUFFER: usize = 65536;

type DispatchFuture = Pin<Box<dyn Future<Output = Result<AppendResult<Arx>, Refusal>> + Send>>;
/// Implementations must not retain store ownership outside the transport keeper.
trait AppendDispatch: Send + Sync {
    fn append(&self, request: AppendEntriesRequest<Arx>) -> DispatchFuture;
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Refusal {
    Busy,
    Closed,
    NotReady,
    Address,
    Tls,
    Identity,
    Frame,
    Deadline,
    Dispatch,
    Unclean,
}
impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Busy => "peer transport busy",
            Self::Closed => "peer transport closed",
            Self::NotReady => "peer transport not ready",
            Self::Address => "peer transport address refused",
            Self::Tls => "peer TLS refused",
            Self::Identity => "peer identity refused",
            Self::Frame => "peer frame refused",
            Self::Deadline => "peer deadline expired",
            Self::Dispatch => "peer dispatch unavailable",
            Self::Unclean => "peer transport close unclean",
        })
    }
}
impl std::error::Error for Refusal {}
fn rpc_error(reason: Refusal) -> RPCError<Arx> {
    RPCError::Unreachable(Unreachable::new(&reason))
}
fn endpoint(text: &str) -> Result<SocketAddr, Refusal> {
    if text.len() > 200 {
        return Err(Refusal::Address);
    }
    let address: SocketAddr = text.parse().map_err(|_| Refusal::Address)?;
    let ip = address.ip();
    if address.port() == 0
        || address.to_string() != text
        || ip.is_unspecified()
        || ip.is_multicast()
        || ip.is_loopback()
        || matches!(ip, std::net::IpAddr::V4(a) if a.is_broadcast())
        || matches!(address, SocketAddr::V6(a) if a.scope_id() != 0 || a.ip().to_ipv4_mapped().is_some())
    {
        return Err(Refusal::Address);
    }
    Ok(address)
}
struct Credentials {
    root: CertificateDer<'static>,
    leaf: CertificateDer<'static>,
    key: PrivateKeyDer<'static>,
    time: Arc<dyn TimeProvider>,
}
struct Resources {
    cluster: ClusterId,
    node: NodeId,
    registry: Registry,
    credentials: Credentials,
    server: Arc<ServerConfig>,
    dispatch: Arc<dyn AppendDispatch>,
}
impl Resources {
    fn client_config(&self, target: NodeId) -> Result<Arc<ClientConfig>, Refusal> {
        let verifier = PeerVerifier::new(
            self.credentials.root.clone(),
            self.cluster,
            self.registry.clone(),
        )
        .map_err(|_| Refusal::Tls)?
        .client(target);
        verify::client_config(
            verifier,
            self.credentials.leaf.clone(),
            self.credentials.key.clone_key(),
            self.credentials.time.clone(),
        )
        .map(Arc::new)
        .map_err(|_| Refusal::Tls)
    }
}
struct State {
    closed: bool,
    resources: Option<Arc<Resources>>,
    keeper: Option<SmAdapter>,
    tasks: JoinSet<()>,
    // <=32 task results plus the single accept-loop result during close.
    join_errors: Vec<JoinError>,
    accept: Option<JoinHandle<()>>,
    close_deadline: Option<Instant>,
    aborting: bool,
    unclean: bool,
    finished: bool,
    releasing: bool,
}
struct Shared {
    state: Mutex<State>,
    connections: Arc<Semaphore>,
    handshakes: Arc<Semaphore>,
    stop: watch::Sender<bool>,
    #[cfg(test)]
    hooks: TestHooks,
}
struct TransportOwner {
    shared: Arc<Shared>,
}
#[derive(Clone)]
struct AppendClient {
    shared: Arc<Shared>,
    target: Option<NodeId>,
    address: Result<SocketAddr, Refusal>,
}
struct Admission {
    resources: Arc<Resources>,
    connection: OwnedSemaphorePermit,
    handshake: OwnedSemaphorePermit,
}
impl Shared {
    fn closed(&self) -> bool {
        self.state.lock().map_or(true, |state| state.closed)
    }
    fn reap(state: &mut State) {
        while let Some(result) = state.tasks.try_join_next() {
            if let Err(error) = result {
                state.unclean = true;
                state.join_errors.push(error);
            }
        }
    }
    /// The caller holds the same gate through registration of the admitted task.
    fn admission(&self, state: &mut State) -> Result<Admission, Refusal> {
        if state.closed {
            return Err(Refusal::Closed);
        }
        Self::reap(state);
        if state.tasks.len() >= CONNECTIONS {
            return Err(Refusal::Busy);
        }
        let resources = state.resources.clone().ok_or(Refusal::NotReady)?;
        let connection = self
            .connections
            .clone()
            .try_acquire_owned()
            .map_err(|_| Refusal::Busy)?;
        let handshake = self
            .handshakes
            .clone()
            .try_acquire_owned()
            .map_err(|_| Refusal::Busy)?;
        Ok(Admission {
            resources,
            connection,
            handshake,
        })
    }
}
impl TransportOwner {
    fn new(
        cluster: ClusterId,
        node: NodeId,
        sm: &SmAdapter,
        credentials: Credentials,
        dispatch: Arc<dyn AppendDispatch>,
    ) -> Result<Self, Refusal> {
        let registry = peer_registry::registry(sm);
        let verifier = PeerVerifier::new(credentials.root.clone(), cluster, registry.clone())
            .map_err(|_| Refusal::Tls)?
            .server();
        let server = Arc::new(
            verify::server_config(
                verifier,
                credentials.leaf.clone(),
                credentials.key.clone_key(),
                credentials.time.clone(),
            )
            .map_err(|_| Refusal::Tls)?,
        );
        let (stop, _) = watch::channel(false);
        Ok(Self {
            shared: Arc::new(Shared {
                state: Mutex::new(State {
                    closed: false,
                    resources: Some(Arc::new(Resources {
                        cluster,
                        node,
                        registry,
                        credentials,
                        server,
                        dispatch,
                    })),
                    keeper: Some(sm.clone()),
                    tasks: JoinSet::new(),
                    join_errors: Vec::new(),
                    accept: None,
                    close_deadline: None,
                    aborting: false,
                    unclean: false,
                    finished: false,
                    releasing: false,
                }),
                connections: Arc::new(Semaphore::new(CONNECTIONS)),
                handshakes: Arc::new(Semaphore::new(HANDSHAKES)),
                stop,
                #[cfg(test)]
                hooks: TestHooks::default(),
            }),
        })
    }
    fn client(&self, target: u64, address: &str) -> AppendClient {
        AppendClient {
            shared: self.shared.clone(),
            target: NodeId::new(target),
            address: endpoint(address),
        }
    }
    #[cfg(test)]
    fn loopback_client(&self, target: u64, address: SocketAddr) -> AppendClient {
        AppendClient {
            shared: self.shared.clone(),
            target: NodeId::new(target),
            address: if address.ip().is_loopback() && address.port() != 0 {
                Ok(address)
            } else {
                Err(Refusal::Address)
            },
        }
    }
    async fn listen(&mut self, address: &str) -> Result<(), Refusal> {
        if self.shared.closed() {
            return Err(Refusal::Closed);
        }
        let address = endpoint(address)?;
        let listener = TcpListener::bind(address)
            .await
            .map_err(|_| Refusal::Address)?;
        self.listen_on(listener)
    }
    fn listen_on(&mut self, listener: TcpListener) -> Result<(), Refusal> {
        let mut state = self.shared.state.lock().map_err(|_| Refusal::Unclean)?;
        if state.closed {
            return Err(Refusal::Closed);
        }
        if state.resources.is_none() || state.accept.is_some() {
            return Err(Refusal::NotReady);
        }
        let shared = self.shared.clone();
        let mut stop = shared.stop.subscribe();
        state.accept = Some(tokio::spawn(async move {
            loop {
                if *stop.borrow() {
                    return;
                }
                let incoming = tokio::select! {
                    biased;
                    _ = stop.changed() => return,
                    incoming = listener.accept() => incoming,
                };
                let Ok((socket, _)) = incoming else {
                    return;
                };
                let deadline = Instant::now() + INBOUND_BUDGET;
                let mut state = match shared.state.lock() {
                    Ok(state) => state,
                    Err(_) => return,
                };
                let admitted = shared.admission(&mut state);
                let join_errors = if state.closed {
                    Vec::new()
                } else {
                    std::mem::take(&mut state.join_errors)
                };
                let admission = match admitted {
                    Ok(admission) => admission,
                    Err(reason) => {
                        drop(state);
                        drop(join_errors);
                        event(&shared, Event::Refused(reason));
                        continue;
                    }
                };
                let task_shared = shared.clone();
                state.tasks.spawn(async move {
                    let Admission {
                        resources,
                        connection: _connection,
                        handshake,
                    } = admission;
                    let result = timeout_at(
                        deadline,
                        inbound(&task_shared, resources, socket, handshake, deadline),
                    )
                    .await
                    .unwrap_or(Err(Refusal::Deadline));
                    event(&task_shared, Event::Finished(result.err()));
                });
                drop(state);
                drop(join_errors);
            }
        }));
        Ok(())
    }
    /// All handles remain in State across every await and cancelled close future.
    async fn close(&mut self) -> Result<(), Refusal> {
        let deadline = {
            let mut state = self.shared.state.lock().map_err(|_| Refusal::Unclean)?;
            if state.finished {
                return if state.unclean {
                    Err(Refusal::Unclean)
                } else {
                    Ok(())
                };
            }
            state.closed = true;
            if state.releasing {
                state.unclean = true;
            }
            let deadline = *state
                .close_deadline
                .get_or_insert_with(|| Instant::now() + CLOSE_BUDGET);
            if Instant::now() >= deadline || state.aborting {
                state.unclean = true;
                state.aborting = true;
                if let Some(accept) = &state.accept {
                    accept.abort();
                }
                state.tasks.abort_all();
            }
            deadline
        };
        self.shared.stop.send_replace(true);
        let drained = timeout_at(
            deadline,
            poll_fn(|cx| {
                let Ok(mut state) = self.shared.state.lock() else {
                    return Poll::Ready(Err(Refusal::Unclean));
                };
                if let Some(accept) = &mut state.accept {
                    let result = ready!(Pin::new(accept).poll(cx));
                    if let Err(error) = result {
                        state.unclean = true;
                        state.join_errors.push(error);
                    }
                    state.accept.take();
                }
                while let Poll::Ready(Some(result)) = state.tasks.poll_join_next(cx) {
                    if let Err(error) = result {
                        state.unclean = true;
                        state.join_errors.push(error);
                    }
                }
                if state.tasks.is_empty() {
                    Poll::Ready(Ok(()))
                } else {
                    Poll::Pending
                }
            }),
        )
        .await;
        if !matches!(drained, Ok(Ok(()))) || Instant::now() >= deadline {
            let mut state = self.shared.state.lock().map_err(|_| Refusal::Unclean)?;
            state.unclean = true;
            state.aborting = true;
            if let Some(accept) = &state.accept {
                accept.abort();
            }
            state.tasks.abort_all();
        }
        #[cfg(test)]
        {
            let aborting = self
                .shared
                .state
                .lock()
                .map_err(|_| Refusal::Unclean)?
                .aborting;
            if aborting {
                stage(&self.shared, Stage::CloseAborted).await;
            }
        }
        // Cancelling here also leaves each handle and keeper in persistent State.
        let (resources, keeper, join_errors, unclean) = poll_fn(|cx| {
            let Ok(mut state) = self.shared.state.lock() else {
                return Poll::Ready(Err(Refusal::Unclean));
            };
            if let Some(accept) = &mut state.accept {
                let result = ready!(Pin::new(accept).poll(cx));
                if let Err(error) = result {
                    state.unclean = true;
                    state.join_errors.push(error);
                }
                state.accept.take();
            }
            while let Poll::Ready(Some(result)) = state.tasks.poll_join_next(cx) {
                if let Err(error) = result {
                    state.unclean = true;
                    state.join_errors.push(error);
                }
            }
            if !state.tasks.is_empty() {
                return Poll::Pending;
            }
            // Move handles out, but never run dispatch/store destructors under the gate.
            state.releasing = true;
            let join_errors = std::mem::take(&mut state.join_errors);
            Poll::Ready(Ok((
                state.resources.take(),
                state.keeper.take(),
                join_errors,
                state.unclean,
            )))
        })
        .await?;
        // No await from extraction through release/completion: cancellation cannot
        // strand these references in a suspended close future. A destructor panic
        // leaves releasing=true, so a subsequent close cannot claim clean release.
        drop(join_errors);
        drop(resources);
        drop(keeper);
        let mut state = self.shared.state.lock().map_err(|_| Refusal::Unclean)?;
        state.releasing = false;
        state.finished = true;
        state.unclean |= unclean || Instant::now() >= deadline;
        if state.unclean {
            Err(Refusal::Unclean)
        } else {
            Ok(())
        }
    }
}
impl Drop for TransportOwner {
    fn drop(&mut self) {
        self.shared.stop.send_replace(true);
        if let Ok(mut state) = self.shared.state.lock() {
            state.closed = true;
            if !state.finished {
                state.unclean = true;
                if let Some(accept) = &state.accept {
                    accept.abort();
                }
                state.tasks.abort_all();
            }
        }
    }
}
fn before_dispatch(shared: &Shared, deadline: Instant) -> Result<(), Refusal> {
    if Instant::now() >= deadline {
        return Err(Refusal::Deadline);
    }
    if shared.closed() {
        return Err(Refusal::Closed);
    }
    Ok(())
}
fn request_shape(request: &AppendEntriesRequest<Arx>, source: NodeId) -> Result<(), Refusal> {
    if request.entries.len() > crate::cluster::log_store::MAX_APPEND
        || !request.vote.committed
        || request.vote.leader_id.node_id != source.get()
    {
        return Err(Refusal::Frame);
    }
    let mut previous = request.prev_log_id;
    for entry in &request.entries {
        let index = previous
            .map_or(Some(0), |id| id.index.checked_add(1))
            .ok_or(Refusal::Frame)?;
        if entry.log_id.index != index || previous.is_some_and(|p| entry.log_id <= p) {
            return Err(Refusal::Frame);
        }
        previous = Some(entry.log_id);
    }
    Ok(())
}
fn reply_shape(
    sent_last: Option<Id>,
    sent_prev: Option<Id>,
    sent_vote: openraft::Vote<super::LeaderId>,
    reply: &AppendResult<Arx>,
) -> Result<(), Refusal> {
    let valid = match reply {
        Ok(accepted) => *accepted == sent_last,
        Err(AppendError::Conflict { log_id, .. }) => Some(*log_id) == sent_prev,
        Err(AppendError::HigherVote(vote)) => {
            vote.partial_cmp(&sent_vote) == Some(Ordering::Greater)
        }
    };
    if valid { Ok(()) } else { Err(Refusal::Frame) }
}
async fn read_frame<S: AsyncRead + Unpin>(
    stream: &mut S,
    expected: peer_wire::Route,
    kind: peer_wire::Kind,
) -> Result<Vec<u8>, Refusal> {
    let mut fixed = [0; peer_wire::HEADER_LEN];
    stream
        .read_exact(&mut fixed)
        .await
        .map_err(|_| Refusal::Frame)?;
    let header = peer_wire::inspect_header(&fixed).map_err(|_| Refusal::Frame)?;
    if header.kind != kind || header.route != expected {
        return Err(Refusal::Frame);
    }
    let total = fixed
        .len()
        .checked_add(header.payload_len)
        .ok_or(Refusal::Frame)?;
    let mut frame = Vec::with_capacity(total);
    frame.extend(fixed);
    frame.resize(total, 0);
    stream
        .read_exact(&mut frame[fixed.len()..])
        .await
        .map_err(|_| Refusal::Frame)?;
    Ok(frame)
}
async fn clean_end<S: AsyncRead + Unpin>(stream: &mut S) -> Result<(), Refusal> {
    let mut extra = [0; 1];
    match stream.read(&mut extra).await {
        Ok(0) => Ok(()),
        _ => Err(Refusal::Frame),
    }
}
async fn send_frame<S: AsyncWrite + Unpin>(stream: &mut S, frame: &[u8]) -> Result<(), Refusal> {
    stream.write_all(frame).await.map_err(|_| Refusal::Frame)?;
    stream.flush().await.map_err(|_| Refusal::Frame)?;
    stream.shutdown().await.map_err(|_| Refusal::Frame)
}
async fn inbound(
    shared: &Shared,
    resources: Arc<Resources>,
    socket: TcpStream,
    handshake: OwnedSemaphorePermit,
    deadline: Instant,
) -> Result<(), Refusal> {
    stage(shared, Stage::ServerHandshake).await;
    before_dispatch(shared, deadline)?;
    let mut stream = TlsAcceptor::from(resources.server.clone())
        .accept_with(socket, |c| c.set_buffer_limit(Some(SEND_BUFFER)))
        .await
        .map_err(|_| Refusal::Tls)?;
    let peer = AuthenticatedPeer::from_server(stream.get_ref().1)
        .and_then(|peer| peer.authenticated_identity())
        .map_err(|_| Refusal::Identity)?;
    drop(handshake);
    stage(shared, Stage::ServerAuthenticated).await;
    before_dispatch(shared, deadline)?;
    if peer.0 != resources.cluster {
        return Err(Refusal::Identity);
    }
    let route = peer_wire::Route {
        cluster: resources.cluster,
        source: peer.1.get(),
        target: resources.node.get(),
    };
    let frame = read_frame(&mut stream, route, peer_wire::Kind::AppendRequest).await?;
    stage(shared, Stage::RequestRead).await;
    before_dispatch(shared, deadline)?;
    clean_end(&mut stream).await?;
    let (_, request) = peer_wire::decode_request(&frame).map_err(|_| Refusal::Frame)?;
    request_shape(&request, peer.1)?;
    let sent_last = request
        .entries
        .last()
        .map(|entry| entry.log_id)
        .or(request.prev_log_id);
    let sent_prev = request.prev_log_id;
    let sent_vote = request.vote;
    stage(shared, Stage::BeforeDispatch).await;
    before_dispatch(shared, deadline)?;
    (resources.registry)(peer.0, peer.1, peer.2).map_err(|_| Refusal::Identity)?;
    before_dispatch(shared, deadline)?;
    let reply = resources.dispatch.append(request).await?;
    reply_shape(sent_last, sent_prev, sent_vote, &reply)?;
    let frame = peer_wire::encode_reply(
        peer_wire::Route {
            cluster: route.cluster,
            source: route.target,
            target: route.source,
        },
        &reply,
    )
    .map_err(|_| Refusal::Frame)?;
    stage(shared, Stage::BeforeReply).await;
    if Instant::now() >= deadline {
        return Err(Refusal::Deadline);
    }
    send_frame(&mut stream, &frame).await
}
impl NetAppend<Arx> for AppendClient {
    async fn append_entries(
        &mut self,
        request: AppendEntriesRequest<Arx>,
        option: RPCOption,
    ) -> Result<AppendResult<Arx>, RPCError<Arx>> {
        let entry = Instant::now();
        // The pinned soft_ttl implementation multiplies before dividing. Refuse
        // overflow rather than allowing an adversarial duration to panic.
        option
            .hard_ttl()
            .checked_mul(3)
            .ok_or_else(|| rpc_error(Refusal::Deadline))?;
        let ttl = option.soft_ttl();
        let deadline = entry
            .checked_add(ttl)
            .filter(|_| !ttl.is_zero())
            .ok_or_else(|| rpc_error(Refusal::Deadline))?;
        let address = self.address.map_err(rpc_error)?;
        let target = self.target.ok_or_else(|| rpc_error(Refusal::Address))?;
        let (tx, rx) = oneshot::channel();
        // Declared outside the lock scope so refusal unwinding unlocks first.
        let mut join_errors = Vec::new();
        {
            let mut state = self
                .shared
                .state
                .lock()
                .map_err(|_| rpc_error(Refusal::Unclean))?;
            let admitted = self.shared.admission(&mut state);
            if !state.closed {
                join_errors.append(&mut state.join_errors);
            }
            let admission = admitted.map_err(rpc_error)?;
            let shared = self.shared.clone();
            state.tasks.spawn(async move {
                let Admission {
                    resources,
                    connection: _connection,
                    handshake,
                } = admission;
                let result = timeout_at(
                    deadline,
                    outbound(
                        &shared, resources, address, target, request, handshake, deadline, &tx,
                    ),
                )
                .await
                .unwrap_or(Err(Refusal::Deadline));
                let reason = result.as_ref().err().copied();
                // Socket/resources are gone; task retains its connection unit until return.
                let _ = tx.send(result);
                event(&shared, Event::Finished(reason));
            });
        }
        drop(join_errors);
        match timeout_at(deadline, rx).await {
            Ok(Ok(result)) if Instant::now() < deadline => result.map_err(rpc_error),
            _ => Err(rpc_error(Refusal::Deadline)),
        }
    }
}
#[allow(clippy::too_many_arguments)]
async fn outbound(
    shared: &Shared,
    resources: Arc<Resources>,
    address: SocketAddr,
    target: NodeId,
    request: AppendEntriesRequest<Arx>,
    handshake: OwnedSemaphorePermit,
    deadline: Instant,
    caller: &oneshot::Sender<Result<AppendResult<Arx>, Refusal>>,
) -> Result<AppendResult<Arx>, Refusal> {
    before_dispatch(shared, deadline)?;
    if caller.is_closed() {
        return Err(Refusal::Closed);
    }
    request_shape(&request, resources.node)?;
    let sent_last = request
        .entries
        .last()
        .map(|entry| entry.log_id)
        .or(request.prev_log_id);
    let sent_prev = request.prev_log_id;
    let sent_vote = request.vote;
    let route = peer_wire::Route {
        cluster: resources.cluster,
        source: resources.node.get(),
        target: target.get(),
    };
    let frame = peer_wire::encode_request(route, &request).map_err(|_| Refusal::Frame)?;
    let config = resources.client_config(target)?;
    stage(shared, Stage::BeforeConnect).await;
    before_dispatch(shared, deadline)?;
    if caller.is_closed() {
        return Err(Refusal::Closed);
    }
    let socket = TcpStream::connect(address)
        .await
        .map_err(|_| Refusal::Tls)?;
    stage(shared, Stage::ClientHandshake).await;
    before_dispatch(shared, deadline)?;
    if caller.is_closed() {
        return Err(Refusal::Closed);
    }
    let mut stream = TlsConnector::from(config)
        .connect_with(
            ServerName::try_from(verify::SERVER_NAME).map_err(|_| Refusal::Tls)?,
            socket,
            |c| c.set_buffer_limit(Some(SEND_BUFFER)),
        )
        .await
        .map_err(|_| Refusal::Tls)?;
    let peer = AuthenticatedPeer::from_client(stream.get_ref().1)
        .and_then(|peer| peer.authenticated_identity())
        .map_err(|_| Refusal::Identity)?;
    drop(handshake);
    stage(shared, Stage::ClientAuthenticated).await;
    before_dispatch(shared, deadline)?;
    if caller.is_closed() {
        return Err(Refusal::Closed);
    }
    if peer.0 != resources.cluster || peer.1 != target {
        return Err(Refusal::Identity);
    }
    send_frame(&mut stream, &frame).await?;
    let frame = read_frame(
        &mut stream,
        peer_wire::Route {
            cluster: route.cluster,
            source: route.target,
            target: route.source,
        },
        peer_wire::Kind::AppendReply,
    )
    .await?;
    stage(shared, Stage::ReplyRead).await;
    before_dispatch(shared, deadline)?;
    if caller.is_closed() {
        return Err(Refusal::Closed);
    }
    clean_end(&mut stream).await?;
    let (_, reply) = peer_wire::decode_reply(&frame).map_err(|_| Refusal::Frame)?;
    reply_shape(sent_last, sent_prev, sent_vote, &reply)?;
    before_dispatch(shared, deadline)?;
    if caller.is_closed() {
        return Err(Refusal::Closed);
    }
    (resources.registry)(peer.0, peer.1, peer.2).map_err(|_| Refusal::Identity)?;
    before_dispatch(shared, deadline)?;
    if caller.is_closed() {
        return Err(Refusal::Closed);
    }
    Ok(reply)
}

// Hooks are observation/gates around actual I/O, never replacements for TLS or registry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Stage {
    ServerHandshake,
    ServerAuthenticated,
    RequestRead,
    BeforeDispatch,
    BeforeReply,
    BeforeConnect,
    ClientHandshake,
    ClientAuthenticated,
    ReplyRead,
    #[cfg(test)]
    CloseAborted,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Event {
    Stage(Stage),
    Refused(Refusal),
    Finished(Option<Refusal>),
}
fn event(shared: &Shared, event: Event) {
    #[cfg(test)]
    shared.hooks.emit(event);
    #[cfg(not(test))]
    let _ = (shared, event);
}
async fn stage(shared: &Shared, stage: Stage) {
    event(shared, Event::Stage(stage));
    #[cfg(test)]
    shared.hooks.wait(stage).await;
}
#[cfg(test)]
#[derive(Default)]
struct TestHooks {
    events: Mutex<Option<tokio::sync::mpsc::Sender<Event>>>,
    gates: Mutex<std::collections::BTreeMap<Stage, Arc<Semaphore>>>,
}
#[cfg(test)]
impl TestHooks {
    fn emit(&self, event: Event) {
        if let Some(events) = self.events.lock().unwrap().as_ref() {
            let _ = events.try_send(event);
        }
    }
    async fn wait(&self, stage: Stage) {
        let gate = self.gates.lock().unwrap().get(&stage).cloned();
        if let Some(gate) = gate {
            let _ = gate.acquire().await;
        }
    }
}
#[cfg(test)]
mod tests;
