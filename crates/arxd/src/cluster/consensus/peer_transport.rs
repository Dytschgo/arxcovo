//! Private one-exchange Append transport. No factory, engine or service startup wiring.
use super::{
    Arx, Id, SmAdapter,
    learner_admission::{self, Controller, Reply as AdmissionReply},
    peer_control_wire as control, peer_registry, peer_rpc_wire as peer_wire,
};
use crate::cluster::{
    enrollment::{ClusterId, NodeId},
    pki::verify::{self, AuthenticatedPeer, PeerVerifier, Registry},
};
use openraft::{
    errors::{RPCError, Unreachable},
    network::RPCOption,
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
    sync::{Arc, Mutex, Weak},
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

pub(super) type DispatchFuture =
    Pin<Box<dyn Future<Output = Result<AppendResult<Arx>, Refusal>> + Send>>;
/// Implementations must not retain store ownership outside the transport keeper.
pub(super) trait AppendDispatch: Send + Sync {
    fn append(&self, request: AppendEntriesRequest<Arx>) -> DispatchFuture;
    fn ballot(&self, _request: openraft::raft::VoteRequest<Arx>, _pre: bool) -> BallotFuture {
        Box::pin(async { Err(Refusal::Dispatch) })
    }
    fn snapshot(
        &self,
        _vote: super::Vote<super::LeaderId>,
        _snapshot: openraft::Snapshot<super::LeaderId, super::Members, Vec<u8>>,
    ) -> SnapshotFuture {
        Box::pin(async { Err(Refusal::Dispatch) })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Refusal {
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
pub(super) fn rpc_error(reason: Refusal) -> RPCError<Arx> {
    RPCError::Unreachable(Unreachable::new(&reason))
}
pub(super) fn endpoint(text: &str) -> Result<SocketAddr, Refusal> {
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
pub(super) struct Credentials {
    pub(super) root: CertificateDer<'static>,
    pub(super) leaf: CertificateDer<'static>,
    pub(super) key: PrivateKeyDer<'static>,
    pub(super) time: Arc<dyn TimeProvider>,
}
struct Resources {
    cluster: ClusterId,
    node: NodeId,
    registry: Registry,
    credentials: Credentials,
    learner: Option<Arc<peer_registry::LearnerGate>>,
    server: Arc<ServerConfig>,
    dispatch: Arc<dyn AppendDispatch>,
}
impl Resources {
    fn client_config(&self, target: NodeId) -> Result<Arc<ClientConfig>, Refusal> {
        self.client_with_registry(target, self.registry.clone())
    }

    fn admission_client_config(&self, target: NodeId) -> Result<Arc<ClientConfig>, Refusal> {
        let gate = self.learner.as_ref().ok_or(Refusal::Identity)?.clone();
        self.client_with_registry(
            target,
            Arc::new(move |cluster, node, key| gate.authorize_admission_anchor(cluster, node, key)),
        )
    }

    fn client_with_registry(
        &self,
        target: NodeId,
        registry: Registry,
    ) -> Result<Arc<ClientConfig>, Refusal> {
        let verifier = PeerVerifier::new(self.credentials.root.clone(), self.cluster, registry)
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
    pending: Option<Credentials>,
    attach_attempted: bool,
    closed: bool,
    resources: Option<Arc<Resources>>,
    admission: Option<Arc<Controller>>,
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
    cluster: ClusterId,
    node: NodeId,
    failed: std::sync::atomic::AtomicBool,
    state: Mutex<State>,
    connections: Arc<Semaphore>,
    handshakes: Arc<Semaphore>,
    stop: watch::Sender<bool>,
    #[cfg(test)]
    hooks: TestHooks,
}
pub(super) struct TransportOwner {
    shared: Arc<Shared>,
}
#[derive(Clone)]
pub(super) struct AppendClient {
    shared: Weak<Shared>,
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
        if self.failed.load(std::sync::atomic::Ordering::Acquire) {
            return Err(Refusal::Dispatch);
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
    #[cfg(test)]
    pub(super) fn new(
        cluster: ClusterId,
        node: NodeId,
        sm: &SmAdapter,
        credentials: Credentials,
        dispatch: Arc<dyn AppendDispatch>,
    ) -> Result<Self, Refusal> {
        let resources = make_resources(cluster, node, sm, credentials, dispatch)?;
        let (stop, _) = watch::channel(false);
        Ok(Self {
            shared: Arc::new(Shared {
                cluster,
                node,
                failed: std::sync::atomic::AtomicBool::new(false),
                state: Mutex::new(State {
                    pending: None,
                    attach_attempted: true,
                    closed: false,
                    resources: Some(resources),
                    admission: None,
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
    pub(super) fn client(&self, target: u64, address: &str) -> AppendClient {
        AppendClient {
            shared: Arc::downgrade(&self.shared),
            target: NodeId::new(target),
            address: endpoint(address),
        }
    }
    #[cfg(test)]
    pub(super) fn loopback_client(&self, target: u64, address: SocketAddr) -> AppendClient {
        AppendClient {
            shared: Arc::downgrade(&self.shared),
            target: NodeId::new(target),
            address: if address.ip().is_loopback() && address.port() != 0 {
                Ok(address)
            } else {
                Err(Refusal::Address)
            },
        }
    }
    pub(super) async fn listen(&mut self, address: &str) -> Result<(), Refusal> {
        if self.shared.closed() {
            return Err(Refusal::Closed);
        }
        let address = endpoint(address)?;
        let listener = TcpListener::bind(address)
            .await
            .map_err(|_| Refusal::Address)?;
        self.listen_on(listener)
    }
    pub(super) fn listen_on(&mut self, listener: TcpListener) -> Result<(), Refusal> {
        let mut state = self.shared.state.lock().map_err(|_| Refusal::Unclean)?;
        if state.closed {
            return Err(Refusal::Closed);
        }
        if state.resources.is_none() || state.accept.is_some() {
            return Err(Refusal::NotReady);
        }
        Self::register_listener(&self.shared, &mut state, listener);
        Ok(())
    }
    fn register_listener(shared: &Arc<Shared>, state: &mut State, listener: TcpListener) {
        let shared = shared.clone();
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
    }
    /// All handles remain in State across every await and cancelled close future.
    pub(super) async fn close(&mut self) -> Result<(), Refusal> {
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
        let (resources, keeper, pending, join_errors, unclean) = poll_fn(|cx| {
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
                state.pending.take(),
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
        drop(pending);
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
    if shared.failed.load(std::sync::atomic::Ordering::Acquire) {
        return Err(Refusal::Dispatch);
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
    kind: Option<peer_wire::Kind>,
) -> Result<Vec<u8>, Refusal> {
    let mut fixed = [0; peer_wire::HEADER_LEN];
    stream
        .read_exact(&mut fixed)
        .await
        .map_err(|_| Refusal::Frame)?;
    let header = peer_wire::inspect_header(&fixed).map_err(|_| Refusal::Frame)?;
    if kind.map_or_else(|| header.kind.reply().is_err(), |kind| header.kind != kind)
        || header.route != expected
    {
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
pub(super) async fn clean_end<S: AsyncRead + Unpin>(stream: &mut S) -> Result<(), Refusal> {
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
    let source = socket.peer_addr();
    stage(shared, Stage::ServerHandshake).await;
    before_dispatch(shared, deadline)?;
    let mut stream = TlsAcceptor::from(resources.server.clone())
        .accept_with(socket, |c| c.set_buffer_limit(Some(SEND_BUFFER)))
        .await
        .map_err(|_| Refusal::Tls)?;
    let authenticated =
        AuthenticatedPeer::from_server(stream.get_ref().1).map_err(|_| Refusal::Identity)?;
    let peer = authenticated
        .authenticated_identity()
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
    let mut magic = [0; 4];
    stream
        .read_exact(&mut magic)
        .await
        .map_err(|_| Refusal::Frame)?;
    if magic == control::MAGIC {
        let address =
            control::read_request(&mut stream, route, source.map_err(|_| Refusal::Address)?)
                .await?;
        // Learner bootstrap has no control authority, even for the original anchor.
        if resources.learner.is_some() {
            return Err(Refusal::Identity);
        }
        (resources.registry)(peer.0, peer.1, peer.2).map_err(|_| Refusal::Identity)?;
        before_dispatch(shared, deadline)?;
        let controller = shared
            .state
            .lock()
            .map_err(|_| Refusal::Closed)?
            .admission
            .clone()
            .ok_or(Refusal::NotReady)?;
        let reply = controller.admit(authenticated, address, deadline).await;
        #[cfg(test)]
        if matches!(reply, AdmissionReply::Applied(..))
            && controller
                .hooks
                .suppress_success
                .swap(false, std::sync::atomic::Ordering::AcqRel)
        {
            *controller.hooks.lost_reply.lock().unwrap() = Some(reply);
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(Refusal::Deadline);
        }
        let frame = control::reply(reverse(route), reply);
        return send_frame(&mut stream, &frame).await;
    }
    let frame = read_frame(&mut magic.as_slice().chain(&mut stream), route, None).await?;
    stage(shared, Stage::RequestRead).await;
    before_dispatch(shared, deadline)?;
    clean_end(&mut stream).await?;
    let request = Request::decode(&frame)?;
    request.shape(resources.cluster, peer.1)?;
    let expected = request.expected();
    stage(shared, Stage::BeforeDispatch).await;
    before_dispatch(shared, deadline)?;
    if let Some(learner) = &resources.learner {
        if let Request::Append(append) = &request
            && !learner.matches_original_append(append)
        {
            return Err(Refusal::Identity);
        }
        let bootstrap_allowed = matches!(request, Request::Append(_) | Request::Snapshot(..));
        let _lane = learner
            .authorize(peer.0, peer.1, peer.2, bootstrap_allowed)
            .map_err(|_| Refusal::Identity)?;
        before_dispatch(shared, deadline)?;
        #[cfg(test)]
        event(shared, Event::LearnerDispatch(_lane));
    } else {
        (resources.registry)(peer.0, peer.1, peer.2).map_err(|_| Refusal::Identity)?;
        before_dispatch(shared, deadline)?;
    }
    let reply = match request {
        Request::Append(r) => resources.dispatch.append(r).await.map(Reply::Append),
        Request::Ballot(r, pre) => resources
            .dispatch
            .ballot(r, pre)
            .await
            .map(|r| Reply::Ballot(r, pre)),
        Request::Snapshot(v, s) => resources.dispatch.snapshot(v, s).await.map(Reply::Snapshot),
    };
    if matches!(reply, Err(Refusal::Dispatch)) {
        shared
            .failed
            .store(true, std::sync::atomic::Ordering::Release);
    }
    let reply = reply?;
    expected.check(&reply)?;
    let frame = reply.encode(peer_wire::Route {
        cluster: route.cluster,
        source: route.target,
        target: route.source,
    })?;
    stage(shared, Stage::BeforeReply).await;
    if Instant::now() >= deadline {
        return Err(Refusal::Deadline);
    }
    send_frame(&mut stream, &frame).await
}
impl AppendClient {
    async fn exchange(
        &mut self,
        request: Request,
        option: RPCOption,
        mut cancel: Pin<Box<dyn Future<Output = openraft::errors::ReplicationClosed> + Send>>,
    ) -> Result<Reply, ExchangeError> {
        let entry = Instant::now();
        if let Some(reason) = poll_fn(|cx| {
            Poll::Ready(match cancel.as_mut().poll(cx) {
                Poll::Ready(r) => Some(r),
                Poll::Pending => None,
            })
        })
        .await
        {
            return Err(ExchangeError::Cancelled(reason));
        }
        // The pinned soft_ttl implementation multiplies before dividing. Refuse
        // overflow rather than allowing an adversarial duration to panic.
        option
            .hard_ttl()
            .checked_mul(3)
            .ok_or(ExchangeError::Refused(Refusal::Deadline))?;
        let ttl = option.soft_ttl();
        let deadline = entry
            .checked_add(ttl)
            .filter(|_| !ttl.is_zero())
            .ok_or(ExchangeError::Refused(Refusal::Deadline))?;
        let shared = self
            .shared
            .upgrade()
            .ok_or(ExchangeError::Refused(Refusal::Closed))?;
        let address = self.address.map_err(ExchangeError::Refused)?;
        let target = self
            .target
            .ok_or(ExchangeError::Refused(Refusal::Address))?;
        let (tx, rx) = oneshot::channel();
        // Declared outside the lock scope so refusal unwinding unlocks first.
        let mut join_errors = Vec::new();
        {
            let mut state = shared
                .state
                .lock()
                .map_err(|_| ExchangeError::Refused(Refusal::Unclean))?;
            let admitted = shared.admission(&mut state);
            if !state.closed {
                join_errors.append(&mut state.join_errors);
            }
            let admission = admitted.map_err(ExchangeError::Refused)?;
            let shared = shared.clone();
            state.tasks.spawn(async move {
                let Admission {
                    resources,
                    connection: _connection,
                    handshake,
                } = admission;
                let result = tokio::select! { biased;
                    reason = &mut cancel => Err(ExchangeError::Cancelled(reason)),
                    result = timeout_at(
                    deadline,
                    outbound(
                        &shared, resources, address, target, request, handshake, deadline, &tx,
                    ),
                ) => result.unwrap_or(Err(Refusal::Deadline)).map_err(ExchangeError::Refused),
                };
                let result = if matches!(result, Err(ExchangeError::Cancelled(_))) {
                    result
                } else {
                    match poll_fn(|cx| {
                        Poll::Ready(match cancel.as_mut().poll(cx) {
                            Poll::Ready(c) => Some(c),
                            Poll::Pending => None,
                        })
                    })
                    .await
                    {
                        Some(c) => Err(ExchangeError::Cancelled(c)),
                        None => result,
                    }
                };
                let reason = result.as_ref().err().map(|e| match e {
                    ExchangeError::Refused(r) => *r,
                    ExchangeError::Cancelled(_) => Refusal::Closed,
                });
                // Socket/resources are gone; task retains its connection unit until return.
                let _ = tx.send(result);
                event(&shared, Event::Finished(reason));
            });
        }
        drop(join_errors);
        match timeout_at(deadline, rx).await {
            Ok(Ok(result)) if Instant::now() < deadline => result,
            _ => Err(ExchangeError::Refused(Refusal::Deadline)),
        }
    }
}
#[allow(clippy::too_many_arguments)]
async fn outbound(
    shared: &Shared,
    resources: Arc<Resources>,
    address: SocketAddr,
    target: NodeId,
    request: Request,
    handshake: OwnedSemaphorePermit,
    deadline: Instant,
    caller: &oneshot::Sender<Result<Reply, ExchangeError>>,
) -> Result<Reply, Refusal> {
    before_dispatch(shared, deadline)?;
    if caller.is_closed() {
        return Err(Refusal::Closed);
    }
    request.shape(resources.cluster, resources.node)?;
    let expected = request.expected();
    let kind = request.kind().reply().map_err(|_| Refusal::Frame)?;
    let route = peer_wire::Route {
        cluster: resources.cluster,
        source: resources.node.get(),
        target: target.get(),
    };
    let frame = request.encode(route)?;
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
    #[cfg(test)]
    if kind == peer_wire::Kind::SnapshotReply {
        stream
            .write_all(&frame[..frame.len() - 1])
            .await
            .map_err(|_| Refusal::Frame)?;
        stream.flush().await.map_err(|_| Refusal::Frame)?;
        stage(shared, Stage::SnapshotPartialBody).await;
        send_frame(&mut stream, &frame[frame.len() - 1..]).await?;
    } else {
        send_frame(&mut stream, &frame).await?;
    }
    #[cfg(not(test))]
    send_frame(&mut stream, &frame).await?;
    let frame = read_frame(
        &mut stream,
        peer_wire::Route {
            cluster: route.cluster,
            source: route.target,
            target: route.source,
        },
        Some(kind),
    )
    .await?;
    stage(shared, Stage::ReplyRead).await;
    before_dispatch(shared, deadline)?;
    if caller.is_closed() {
        return Err(Refusal::Closed);
    }
    clean_end(&mut stream).await?;
    let reply = Reply::decode(&frame, kind)?;
    expected.check(&reply)?;
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
pub(super) enum Stage {
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
    #[cfg(test)]
    SnapshotPartialBody,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Event {
    #[cfg(test)]
    LearnerDispatch(peer_registry::Lane),
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
pub(super) type BallotFuture =
    Pin<Box<dyn Future<Output = Result<openraft::raft::VoteResponse<Arx>, Refusal>> + Send>>;
pub(super) type SnapshotFuture =
    Pin<Box<dyn Future<Output = Result<openraft::raft::SnapshotResponse<Arx>, Refusal>> + Send>>;
type Ballot = openraft::raft::VoteRequest<Arx>;
type BallotReply = openraft::raft::VoteResponse<Arx>;
type Snapshot = openraft::Snapshot<super::LeaderId, super::Members, Vec<u8>>;
type BallotVote = super::Vote<super::LeaderId>;
enum Request {
    Append(AppendEntriesRequest<Arx>),
    Ballot(Ballot, bool),
    Snapshot(BallotVote, Snapshot),
}
enum Reply {
    Append(AppendResult<Arx>),
    Ballot(BallotReply, bool),
    Snapshot(openraft::raft::SnapshotResponse<Arx>),
}
enum ExchangeError {
    Refused(Refusal),
    Cancelled(openraft::errors::ReplicationClosed),
}
enum Expected {
    Append(Option<Id>, Option<Id>, BallotVote),
    Ballot(BallotVote, Option<Id>, bool),
    Snapshot(BallotVote),
}
impl Request {
    fn kind(&self) -> peer_wire::Kind {
        use peer_wire::Kind::*;
        match self {
            Self::Append(_) => AppendRequest,
            Self::Ballot(_, false) => VoteRequest,
            Self::Ballot(_, true) => PreVoteRequest,
            Self::Snapshot(..) => SnapshotRequest,
        }
    }
    fn expected(&self) -> Expected {
        match self {
            Self::Append(r) => Expected::Append(
                r.entries.last().map(|e| e.log_id).or(r.prev_log_id),
                r.prev_log_id,
                r.vote,
            ),
            Self::Ballot(r, p) => Expected::Ballot(r.vote, r.last_log_id, *p),
            Self::Snapshot(v, _) => Expected::Snapshot(*v),
        }
    }
    fn shape(&self, c: crate::cluster::enrollment::ClusterId, n: NodeId) -> Result<(), Refusal> {
        match self {
            Self::Append(r) => request_shape(r, n),
            Self::Ballot(r, _) => {
                if r.vote.leader_id.node_id == n.get() {
                    Ok(())
                } else {
                    Err(Refusal::Frame)
                }
            }
            Self::Snapshot(v, s) => {
                if !v.committed || v.leader_id.node_id != n.get() {
                    return Err(Refusal::Frame);
                }
                peer_wire::inspect_snapshot(c, *v, s).map_err(|_| Refusal::Frame)
            }
        }
    }
    fn encode(&self, r: peer_wire::Route) -> Result<Vec<u8>, Refusal> {
        match self {
            Self::Append(q) => peer_wire::encode_request(r, q),
            Self::Ballot(q, _) => peer_wire::encode_vote(r, self.kind(), q),
            Self::Snapshot(v, s) => peer_wire::encode_snapshot(r, *v, s),
        }
        .map_err(|_| Refusal::Frame)
    }
    fn decode(b: &[u8]) -> Result<Self, Refusal> {
        let h = peer_wire::inspect_header(b.get(..44).ok_or(Refusal::Frame)?)
            .map_err(|_| Refusal::Frame)?;
        use peer_wire::Kind::*;
        match h.kind {
            AppendRequest => peer_wire::decode_request(b).map(|(_, r)| Self::Append(r)),
            VoteRequest | PreVoteRequest => peer_wire::decode_vote(b, h.kind)
                .map(|(_, r)| Self::Ballot(r, h.kind == PreVoteRequest)),
            SnapshotRequest => peer_wire::decode_snapshot(b).map(|(_, v, s)| Self::Snapshot(v, s)),
            _ => return Err(Refusal::Frame),
        }
        .map_err(|_| Refusal::Frame)
    }
}
impl Reply {
    fn encode(&self, r: peer_wire::Route) -> Result<Vec<u8>, Refusal> {
        match self {
            Self::Append(q) => peer_wire::encode_reply(r, q),
            Self::Ballot(q, p) => peer_wire::encode_vote_reply(
                r,
                if *p {
                    peer_wire::Kind::PreVoteReply
                } else {
                    peer_wire::Kind::VoteReply
                },
                q,
            ),
            Self::Snapshot(q) => peer_wire::encode_snapshot_reply(r, q),
        }
        .map_err(|_| Refusal::Frame)
    }
    fn decode(b: &[u8], k: peer_wire::Kind) -> Result<Self, Refusal> {
        use peer_wire::Kind::*;
        match k {
            AppendReply => peer_wire::decode_reply(b).map(|(_, r)| Self::Append(r)),
            VoteReply | PreVoteReply => {
                peer_wire::decode_vote_reply(b, k).map(|(_, r)| Self::Ballot(r, k == PreVoteReply))
            }
            SnapshotReply => peer_wire::decode_snapshot_reply(b).map(|(_, r)| Self::Snapshot(r)),
            _ => return Err(Refusal::Frame),
        }
        .map_err(|_| Refusal::Frame)
    }
}
impl Expected {
    fn check(&self, r: &Reply) -> Result<(), Refusal> {
        let valid = match (self, r) {
            (Self::Append(l, p, v), Reply::Append(r)) => return reply_shape(*l, *p, *v, r),
            (Self::Ballot(v, l, p), Reply::Ballot(r, q)) => {
                p == q
                    && (!r.vote_granted
                        || (r.last_log_id <= *l
                            && if *p {
                                matches!(
                                    r.vote.partial_cmp(v),
                                    Some(Ordering::Less | Ordering::Equal)
                                )
                            } else {
                                r.vote == *v
                            }))
            }
            (Self::Snapshot(v), Reply::Snapshot(r)) => matches!(
                r.vote.partial_cmp(v),
                Some(Ordering::Equal | Ordering::Greater)
            ),
            _ => false,
        };
        if valid { Ok(()) } else { Err(Refusal::Frame) }
    }
}
fn unary_error(e: ExchangeError) -> RPCError<Arx> {
    rpc_error(match e {
        ExchangeError::Refused(r) => r,
        ExchangeError::Cancelled(_) => Refusal::Closed,
    })
}
impl openraft::network::RaftNetworkV2<Arx> for AppendClient {
    type SnapshotData = Vec<u8>;
    async fn append_entries(
        &mut self,
        r: AppendEntriesRequest<Arx>,
        o: RPCOption,
    ) -> Result<AppendResult<Arx>, RPCError<Arx>> {
        match self
            .exchange(Request::Append(r), o, Box::pin(std::future::pending()))
            .await
            .map_err(unary_error)?
        {
            Reply::Append(r) => Ok(r),
            _ => Err(rpc_error(Refusal::Frame)),
        }
    }
    async fn vote(&mut self, r: Ballot, o: RPCOption) -> Result<BallotReply, RPCError<Arx>> {
        self.send_ballot(r, false, o).await
    }
    async fn pre_vote(&mut self, r: Ballot, o: RPCOption) -> Result<BallotReply, RPCError<Arx>> {
        self.send_ballot(r, true, o).await
    }
    async fn full_snapshot(
        &mut self,
        v: BallotVote,
        s: Snapshot,
        cancel: impl Future<Output = openraft::errors::ReplicationClosed> + Send + 'static,
        o: RPCOption,
    ) -> Result<openraft::raft::SnapshotResponse<Arx>, openraft::errors::StreamingError<Arx>> {
        use openraft::errors::StreamingError;
        match self
            .exchange(Request::Snapshot(v, s), o, Box::pin(cancel))
            .await
        {
            Ok(Reply::Snapshot(r)) => Ok(r),
            Err(ExchangeError::Cancelled(c)) => Err(StreamingError::Closed(c)),
            Err(ExchangeError::Refused(r)) => {
                Err(StreamingError::Unreachable(Unreachable::new(&r)))
            }
            _ => Err(StreamingError::Unreachable(Unreachable::new(
                &Refusal::Frame,
            ))),
        }
    }
    async fn transfer_leader(
        &mut self,
        _: openraft::raft::TransferLeaderRequest<Arx>,
        _: RPCOption,
    ) -> Result<openraft::raft::TransferLeaderResponse<Arx>, RPCError<Arx>> {
        Err(rpc_error(Refusal::NotReady))
    }
}
impl AppendClient {
    async fn send_ballot(
        &mut self,
        r: Ballot,
        p: bool,
        o: RPCOption,
    ) -> Result<BallotReply, RPCError<Arx>> {
        match self
            .exchange(Request::Ballot(r, p), o, Box::pin(std::future::pending()))
            .await
            .map_err(unary_error)?
        {
            Reply::Ballot(r, _) => Ok(r),
            _ => Err(rpc_error(Refusal::Frame)),
        }
    }
}

fn make_resources(
    cluster: ClusterId,
    node: NodeId,
    sm: &SmAdapter,
    credentials: Credentials,
    dispatch: Arc<dyn AppendDispatch>,
) -> Result<Arc<Resources>, Refusal> {
    make_resources_for_learner(cluster, node, sm, credentials, dispatch, None)
}

fn make_resources_for_learner(
    cluster: ClusterId,
    node: NodeId,
    sm: &SmAdapter,
    credentials: Credentials,
    dispatch: Arc<dyn AppendDispatch>,
    learner: Option<Arc<peer_registry::LearnerGate>>,
) -> Result<Arc<Resources>, Refusal> {
    let registry = peer_registry::registry(sm);
    let inbound: Registry = match &learner {
        Some(gate) => {
            let gate = gate.clone();
            Arc::new(move |cluster, node, key| gate.authorize(cluster, node, key, true).map(|_| ()))
        }
        None => registry.clone(),
    };
    let verifier = PeerVerifier::new(credentials.root.clone(), cluster, inbound)
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
    Ok(Arc::new(Resources {
        cluster,
        node,
        registry,
        credentials,
        learner,
        server,
        dispatch,
    }))
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Health {
    Pending,
    Attaching,
    Ready,
    Closing,
    Closed,
    EngineUnavailable,
}
#[derive(Clone)]
pub(super) struct PeerFactory {
    shared: Weak<Shared>,
    #[cfg(test)]
    loopback: bool,
}
impl PeerFactory {
    pub(super) fn install_admission(&self, controller: Arc<Controller>) -> Result<(), Refusal> {
        let shared = self.shared.upgrade().ok_or(Refusal::Closed)?;
        let mut state = shared.state.lock().map_err(|_| Refusal::Closed)?;
        if state.closed
            || state.admission.is_some()
            || state.resources.as_ref().is_none_or(|r| r.learner.is_some())
        {
            return Err(Refusal::NotReady);
        }
        state.admission = Some(controller);
        Ok(())
    }

    fn client(&self, target: u64, node: &openraft::BasicNode) -> AppendClient {
        AppendClient {
            shared: self.shared.clone(),
            target: NodeId::new(target),
            address: {
                #[cfg(test)]
                if self.loopback {
                    return AppendClient {
                        shared: self.shared.clone(),
                        target: NodeId::new(target),
                        address: node
                            .addr
                            .parse::<SocketAddr>()
                            .map_err(|_| Refusal::Address)
                            .and_then(|a| {
                                if a.ip().is_loopback()
                                    && a.port() != 0
                                    && a.to_string() == node.addr
                                {
                                    Ok(a)
                                } else {
                                    Err(Refusal::Address)
                                }
                            }),
                    };
                }
                endpoint(&node.addr)
            },
        }
    }
}
impl openraft::network::RaftNetworkFactory<Arx> for PeerFactory {
    type Network = AppendClient;
    async fn new_client(&mut self, t: u64, n: &openraft::BasicNode) -> AppendClient {
        self.client(t, n)
    }
    async fn new_heartbeat_client(&mut self, t: u64, n: &openraft::BasicNode) -> AppendClient {
        self.client(t, n)
    }
    async fn new_snapshot_client(&mut self, t: u64, n: &openraft::BasicNode) -> AppendClient {
        self.client(t, n)
    }
}
impl TransportOwner {
    pub(super) fn pending(
        cluster: ClusterId,
        node: NodeId,
        credentials: Credentials,
    ) -> (Self, PeerFactory) {
        Self::pending_stage(cluster, node, Some(credentials))
    }
    #[cfg(any(test, target_os = "linux"))]
    pub(super) fn pending_founder(cluster: ClusterId, node: NodeId) -> (Self, PeerFactory) {
        Self::pending_stage(cluster, node, None)
    }
    fn pending_stage(
        cluster: ClusterId,
        node: NodeId,
        credentials: Option<Credentials>,
    ) -> (Self, PeerFactory) {
        let (stop, _) = watch::channel(false);
        let shared = Arc::new(Shared {
            cluster,
            node,
            failed: std::sync::atomic::AtomicBool::new(false),
            state: Mutex::new(State {
                pending: credentials,
                attach_attempted: false,
                closed: false,
                resources: None,
                admission: None,
                keeper: None,
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
        });
        let factory = PeerFactory {
            shared: Arc::downgrade(&shared),
            #[cfg(test)]
            loopback: false,
        };
        (Self { shared }, factory)
    }
    pub(super) fn health(&self) -> Health {
        if self
            .shared
            .failed
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return Health::EngineUnavailable;
        }
        let Ok(s) = self.shared.state.lock() else {
            return Health::EngineUnavailable;
        };
        if s.finished {
            Health::Closed
        } else if s.closed {
            Health::Closing
        } else if s.resources.is_some() {
            Health::Ready
        } else if s.attach_attempted {
            Health::Attaching
        } else {
            Health::Pending
        }
    }
    pub(super) async fn attach(
        &mut self,
        sm: &SmAdapter,
        dispatch: Arc<dyn AppendDispatch>,
        address: &str,
    ) -> Result<(), Refusal> {
        let credentials = self.begin_attach()?;
        let resources = make_resources(
            self.shared.cluster,
            self.shared.node,
            sm,
            credentials,
            dispatch,
        )?;
        let listener = TcpListener::bind(endpoint(address)?)
            .await
            .map_err(|_| Refusal::Address)?;
        self.publish(sm, resources, listener)
    }
    fn begin_attach(&self) -> Result<Credentials, Refusal> {
        self.take_attach_credentials(None)
    }
    fn take_attach_credentials(&self, late: Option<Credentials>) -> Result<Credentials, Refusal> {
        let mut state = self.shared.state.lock().map_err(|_| Refusal::Unclean)?;
        if state.closed {
            return Err(Refusal::Closed);
        }
        if state.attach_attempted {
            return Err(Refusal::NotReady);
        }
        state.attach_attempted = true;
        match (state.pending.take(), late) {
            (Some(credentials), None) | (None, Some(credentials)) => Ok(credentials),
            _ => Err(Refusal::NotReady),
        }
    }
    #[cfg(any(test, target_os = "linux"))]
    pub(super) fn attach_founder(
        &mut self,
        sm: &SmAdapter,
        dispatch: Arc<dyn AppendDispatch>,
        address: &str,
        credentials: Credentials,
    ) -> Result<(), Refusal> {
        // The caller holds the job's close gate through bind and registration.
        // There is no await (and therefore no unowned listener) under that gate.
        let credentials = self.take_attach_credentials(Some(credentials))?;
        let resources = make_resources(
            self.shared.cluster,
            self.shared.node,
            sm,
            credentials,
            dispatch,
        )?;
        let listener = std::net::TcpListener::bind(endpoint(address)?)
            .and_then(|listener| {
                listener.set_nonblocking(true)?;
                Ok(listener)
            })
            .and_then(TcpListener::from_std)
            .map_err(|_| Refusal::Address)?;
        self.publish(sm, resources, listener)
    }

    #[cfg(all(test, target_os = "linux"))]
    pub(super) fn founder_tls_probe(
        &self,
    ) -> Result<(Arc<ClientConfig>, impl Future<Output = bool> + use<>), Refusal> {
        let state = self.shared.state.lock().map_err(|_| Refusal::Unclean)?;
        let config = state
            .resources
            .as_ref()
            .ok_or(Refusal::NotReady)?
            .client_config(self.shared.node)?;
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        *self.shared.hooks.events.lock().unwrap() = Some(tx);
        Ok((config, async move {
            while let Some(event) = rx.recv().await {
                match event {
                    Event::Stage(Stage::ServerAuthenticated) => return true,
                    Event::Finished(_) | Event::Refused(_) => return false,
                    _ => {}
                }
            }
            false
        }))
    }
    fn publish(
        &mut self,
        sm: &SmAdapter,
        resources: Arc<Resources>,
        listener: TcpListener,
    ) -> Result<(), Refusal> {
        // Publish and register accept under the same gate; no task can be admitted between them.
        let mut state = self.shared.state.lock().map_err(|_| Refusal::Unclean)?;
        if state.closed {
            return Err(Refusal::Closed);
        }
        state.resources = Some(resources);
        state.keeper = Some(sm.clone());
        Self::register_listener(&self.shared, &mut state, listener);
        Ok(())
    }
    #[cfg(target_os = "linux")]
    pub(super) fn attach_learner(
        &mut self,
        sm: &SmAdapter,
        dispatch: Arc<dyn AppendDispatch>,
        address: SocketAddr,
        credentials: Credentials,
        gate: Arc<peer_registry::LearnerGate>,
    ) -> Result<(), Refusal> {
        let credentials = self.take_attach_credentials(Some(credentials))?;
        let resources = make_resources_for_learner(
            self.shared.cluster,
            self.shared.node,
            sm,
            credentials,
            dispatch,
            Some(gate),
        )?;
        let listener = std::net::TcpListener::bind(endpoint(&address.to_string())?)
            .and_then(|listener| {
                listener.set_nonblocking(true)?;
                Ok(listener)
            })
            .and_then(TcpListener::from_std)
            .map_err(|_| Refusal::Address)?;
        self.publish(sm, resources, listener)
    }

    #[cfg(all(test, target_os = "linux"))]
    pub(super) fn factory_for_test(&self) -> PeerFactory {
        PeerFactory {
            shared: Arc::downgrade(&self.shared),
            loopback: false,
        }
    }

    #[cfg(all(test, target_os = "linux"))]
    pub(super) fn dispatch_observer(&self) -> tokio::sync::mpsc::Receiver<Event> {
        let (tx, rx) = tokio::sync::mpsc::channel(16);
        *self.shared.hooks.events.lock().unwrap() = Some(tx);
        rx
    }

    pub(super) fn joined(&self) -> bool {
        self.shared.state.lock().is_ok_and(|s| {
            s.finished
                && s.accept.is_none()
                && s.tasks.is_empty()
                && s.resources.is_none()
                && s.keeper.is_none()
        })
    }
}

#[cfg(test)]
impl TransportOwner {
    fn attach_reserved(
        &mut self,
        sm: &SmAdapter,
        dispatch: Arc<dyn AppendDispatch>,
        listener: TcpListener,
        expected: SocketAddr,
    ) -> Result<(), Refusal> {
        let credentials = self.begin_attach()?;
        if !expected.ip().is_loopback()
            || expected.port() == 0
            || listener.local_addr().map_err(|_| Refusal::Address)? != expected
        {
            return Err(Refusal::Address);
        }
        let snapshot = {
            let store = sm.0.try_lock().map_err(|_| Refusal::Identity)?;
            if let Some((_, m)) = store.membership() {
                let membership = super::decode(m).map_err(|_| Refusal::Identity)?;
                if membership
                    .get_node(&self.shared.node.get())
                    .is_none_or(|n| n.addr != expected.to_string())
                {
                    return Err(Refusal::Address);
                }
            }
            store.snapshot()
        };
        super::StateStore::inspect_snapshot(&snapshot, self.shared.cluster)
            .map_err(|_| Refusal::Identity)?;
        let resources = make_resources(
            self.shared.cluster,
            self.shared.node,
            sm,
            credentials,
            dispatch,
        )?;
        self.publish(sm, resources, listener)
    }
}

impl TransportOwner {
    pub(super) fn observe_failure(&self) {
        self.shared
            .failed
            .store(true, std::sync::atomic::Ordering::Release);
    }
}

fn reverse(route: peer_wire::Route) -> peer_wire::Route {
    peer_wire::Route {
        cluster: route.cluster,
        source: route.target,
        target: route.source,
    }
}

pub(super) struct AdmissionClient {
    shared: Weak<Shared>,
    issuer: SocketAddr,
    advertised: SocketAddr,
}

impl TransportOwner {
    pub(super) fn admission_client(
        &self,
        issuer: SocketAddr,
        advertised: SocketAddr,
    ) -> Result<AdmissionClient, Refusal> {
        if endpoint(&issuer.to_string())? != issuer
            || endpoint(&advertised.to_string())? != advertised
            || !control::qualified(issuer)
            || !control::qualified(advertised)
        {
            return Err(Refusal::Address);
        }
        let state = self.shared.state.lock().map_err(|_| Refusal::Closed)?;
        if state.closed
            || state.accept.is_none()
            || state
                .resources
                .as_ref()
                .and_then(|r| r.learner.as_ref())
                .is_none()
        {
            return Err(Refusal::NotReady);
        }
        Ok(AdmissionClient {
            shared: Arc::downgrade(&self.shared),
            issuer,
            advertised,
        })
    }
}

pub(super) struct AdmissionResponse {
    reply: oneshot::Receiver<Result<Result<AdmissionReply, Refusal>, tokio::time::error::Elapsed>>,
    deadline: Instant,
    sent: Arc<std::sync::atomic::AtomicBool>,
}

impl AdmissionResponse {
    pub(super) async fn reply(self) -> AdmissionReply {
        match timeout_at(self.deadline, self.reply).await {
            Ok(Ok(Ok(Ok(reply)))) if Instant::now() < self.deadline => reply,
            _ if self.sent.load(std::sync::atomic::Ordering::SeqCst) => AdmissionReply::Unknown,
            _ => AdmissionReply::Refused,
        }
    }
}

impl AdmissionClient {
    pub(super) fn request(&self) -> Option<AdmissionResponse> {
        self.exchange(self.advertised)
    }

    #[cfg(all(test, target_os = "linux"))]
    pub(super) fn conflicting_address(&self, advertised: SocketAddr) -> Option<AdmissionResponse> {
        self.exchange(advertised)
    }

    fn exchange(&self, advertised: SocketAddr) -> Option<AdmissionResponse> {
        use std::sync::atomic::AtomicBool;
        let deadline = Instant::now() + learner_admission::BUDGET;
        let shared = self.shared.upgrade()?;
        let (tx, rx) = oneshot::channel();
        let sent = Arc::new(AtomicBool::new(false));
        let mut join_errors = Vec::new();
        {
            let mut state = shared.state.lock().ok()?;
            let admission = shared.admission(&mut state);
            if !state.closed {
                join_errors.append(&mut state.join_errors);
            }
            let admission = admission.ok()?;
            let shared = shared.clone();
            let issuer = self.issuer;
            let sent = sent.clone();
            state.tasks.spawn(async move {
                let reply = timeout_at(
                    deadline,
                    outbound_admission(&shared, admission, issuer, advertised, deadline, &sent),
                )
                .await;
                let _ = tx.send(reply);
            });
        }
        drop(join_errors);
        Some(AdmissionResponse {
            reply: rx,
            deadline,
            sent,
        })
    }
}

async fn outbound_admission(
    shared: &Shared,
    admission: Admission,
    issuer: SocketAddr,
    advertised: SocketAddr,
    deadline: Instant,
    sent: &std::sync::atomic::AtomicBool,
) -> Result<AdmissionReply, Refusal> {
    let Admission {
        resources,
        connection: _connection,
        handshake,
    } = admission;
    before_dispatch(shared, deadline)?;
    let gate = resources.learner.as_ref().ok_or(Refusal::Identity)?.clone();
    let target = gate.admission_anchor().map_err(|_| Refusal::Identity)?.0;
    let config = resources.admission_client_config(target)?;
    let socket = TcpStream::connect(issuer).await.map_err(|_| Refusal::Tls)?;
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
    let route = peer_wire::Route {
        cluster: resources.cluster,
        source: resources.node.get(),
        target: target.get(),
    };
    gate.authorize_admission_anchor(peer.0, peer.1, peer.2)
        .map_err(|_| Refusal::Identity)?;
    sent.store(true, std::sync::atomic::Ordering::SeqCst);
    before_dispatch(shared, deadline)?;
    send_frame(&mut stream, &control::request(route, advertised)).await?;
    let reply = control::read_reply(&mut stream, reverse(route), advertised).await?;
    clean_end(&mut stream).await?;
    gate.authorize_admission_anchor(peer.0, peer.1, peer.2)
        .map_err(|_| Refusal::Identity)?;
    before_dispatch(shared, deadline)?;
    Ok(reply)
}
