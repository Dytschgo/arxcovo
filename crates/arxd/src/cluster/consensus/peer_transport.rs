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
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicU8, Ordering as AtomicOrdering},
    },
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
#[cfg(any(test, target_os = "linux"))]
const ISSUER_EXPORTER_LABEL: &[u8] = b"EXPORTER-Arxcovo-voter-ca/sender-to-receiver/v1";

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
#[cfg(any(test, target_os = "linux"))]
fn activation_listener(
    address: SocketAddr,
    inherited: Option<std::net::TcpListener>,
) -> Result<TcpListener, Refusal> {
    let listener = match inherited {
        Some(listener) => {
            #[cfg(target_os = "linux")]
            crate::service::cluster_runtime::listen_inherited(&listener)
                .map_err(|_| Refusal::Address)?;
            listener
        }
        None => std::net::TcpListener::bind(address).map_err(|_| Refusal::Address)?,
    };
    if listener.local_addr().ok() != Some(address) {
        return Err(Refusal::Address);
    }
    listener
        .set_nonblocking(true)
        .map_err(|_| Refusal::Address)?;
    TcpListener::from_std(listener).map_err(|_| Refusal::Address)
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
    registry: peer_registry::AppliedRegistry,
    credentials: Credentials,
    learner: Option<Arc<peer_registry::LearnerGate>>,
    server: Arc<ServerConfig>,
    dispatch: Arc<dyn AppendDispatch>,
}
impl Resources {
    // The owned outbound exchange checks applied authority before its first write.
    fn outbound_tls_config(&self, target: NodeId) -> Result<Arc<ClientConfig>, Refusal> {
        self.client_with_registry(target, Arc::new(|_, _, _| Ok(())))
    }

    // Raw TLS probes have no owned exchange to perform post-TLS authorization.
    #[cfg(test)]
    fn client_config(&self, target: NodeId) -> Result<Arc<ClientConfig>, Refusal> {
        self.client_with_registry(target, self.registry.immediate())
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
    #[cfg(any(test, target_os = "linux"))]
    voter_receiver: Option<Arc<dyn issuer_channel::Receiver>>,
    #[cfg(any(test, target_os = "linux"))]
    voter_transfers: [Option<(NodeId, Arc<issuer_channel::AttemptState>)>; 2],
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
#[cfg(target_os = "linux")]
pub(super) struct BuildPeer(Weak<Shared>);

#[cfg(target_os = "linux")]
fn build_lock<T>(lock: &Mutex<T>) -> Option<std::sync::MutexGuard<'_, T>> {
    // Rustls verification is synchronous. Bound contention without changing
    // the Raft verifier's asynchronous STORE_WAIT or transport deadlines.
    let end = std::time::Instant::now() + Duration::from_millis(4);
    loop {
        match lock.try_lock() {
            Ok(guard) if std::time::Instant::now() < end => return Some(guard),
            Err(std::sync::TryLockError::WouldBlock) if std::time::Instant::now() < end => {
                std::thread::sleep(Duration::from_micros(250));
            }
            _ => return None,
        }
    }
}

#[cfg(target_os = "linux")]
impl BuildPeer {
    fn shared(&self) -> Option<Arc<Shared>> {
        let shared = self.0.upgrade()?;
        (!shared.failed.load(AtomicOrdering::Acquire)).then_some(shared)
    }

    pub(super) fn identity(&self) -> Option<(ClusterId, NodeId)> {
        let shared = self.shared()?;
        Some((shared.cluster, shared.node))
    }

    fn resources(&self) -> Option<Arc<Resources>> {
        let shared = self.shared()?;
        let state = build_lock(&shared.state)?;
        if state.closed {
            return None;
        }
        let resources = state.resources.clone()?;
        drop(state);
        if let Some(gate) = &resources.learner {
            let deadline = Instant::now() + Duration::from_millis(4);
            loop {
                match gate.inspect_strict(Some(deadline), |_| ()) {
                    Ok(()) => break,
                    Err(peer_registry::ObservationRefusal::StoreBusy)
                        if Instant::now() < deadline =>
                    {
                        std::thread::sleep(Duration::from_micros(250));
                    }
                    Err(_) => return None,
                }
            }
        }
        Some(resources)
    }

    fn key(
        &self,
        cluster: ClusterId,
        node: NodeId,
    ) -> Option<crate::cluster::enrollment::KeyDigest> {
        let shared = self.shared()?;
        let state = build_lock(&shared.state)?;
        if state.closed || shared.cluster != cluster {
            return None;
        }
        let sm = state.keeper.clone()?;
        drop(state);
        let store = build_lock(&sm.0)?;
        crate::cluster::delegated_build::applied_binding(&store, cluster, node)
    }

    pub(super) fn binding(&self, cluster: ClusterId, node: NodeId) -> Option<[u8; 32]> {
        self.key(cluster, node).map(|key| *key.as_bytes())
    }

    fn verifier(&self) -> Registry {
        let weak = self.0.clone();
        Arc::new(move |cluster, node, key| {
            let current = BuildPeer(weak.clone()).key(cluster, node);
            if current == Some(key) {
                Ok(())
            } else {
                Err(verify::VerifyRefusal::NotMember)
            }
        })
    }

    pub(super) fn build_client(&self, target: NodeId) -> Option<Arc<ClientConfig>> {
        let resources = self.resources()?;
        self.key(resources.cluster, resources.node)?;
        let mut client = resources
            .client_with_registry(target, self.verifier())
            .ok()?;
        Arc::get_mut(&mut client)?.alpn_protocols = vec![b"arxcovo-build/1".to_vec()];
        Some(client)
    }

    pub(super) fn build_server(&self) -> Option<Arc<ServerConfig>> {
        let resources = self.resources()?;
        self.key(resources.cluster, resources.node)?;
        let creds = &resources.credentials;
        let verifier = PeerVerifier::new(creds.root.clone(), resources.cluster, self.verifier())
            .ok()?
            .server();
        let mut config = verify::server_config(
            verifier,
            creds.leaf.clone(),
            creds.key.clone_key(),
            creds.time.clone(),
        )
        .ok()?;
        config.alpn_protocols = vec![b"arxcovo-build/1".to_vec()];
        Some(Arc::new(config))
    }
}

#[cfg(target_os = "linux")]
impl TransportOwner {
    pub(super) fn build_peer(&self) -> BuildPeer {
        BuildPeer(Arc::downgrade(&self.shared))
    }
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
                    #[cfg(any(test, target_os = "linux"))]
                    voter_receiver: None,
                    #[cfg(any(test, target_os = "linux"))]
                    voter_transfers: [const { None }; 2],
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
                let Ok((socket, _peer)) = incoming else {
                    return;
                };
                let deadline = Instant::now() + INBOUND_BUDGET;
                #[cfg(all(test, target_os = "linux"))]
                let trace = shared.hooks.gated_inbound.get().and_then(Weak::upgrade);
                #[cfg(all(test, target_os = "linux"))]
                if let Some(trace) = &trace {
                    trace.inbound(_peer, "accepted", None);
                }
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
                #[cfg(all(test, target_os = "linux"))]
                if let Some(trace) = &trace {
                    trace.inbound(_peer, "admission", trace_refusal(admitted.as_ref().err()));
                }
                let admission = match admitted {
                    Ok(admission) => admission,
                    Err(reason) => {
                        drop(state);
                        drop(join_errors);
                        #[cfg(all(test, target_os = "linux"))]
                        GatedCallTrace::server_event(
                            trace.as_deref().map(|trace| (trace, _peer)),
                            Event::Refused(reason),
                        );
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
                        inbound(
                            &task_shared,
                            resources,
                            socket,
                            handshake,
                            deadline,
                            #[cfg(all(test, target_os = "linux"))]
                            trace.as_deref().map(|trace| (trace, _peer)),
                        ),
                    )
                    .await
                    .unwrap_or(Err(Refusal::Deadline));
                    #[cfg(all(test, target_os = "linux"))]
                    if let Some(trace) = &trace {
                        trace.inbound(_peer, "finished", trace_refusal(result.as_ref().err()));
                        GatedCallTrace::server_event(
                            Some((trace, _peer)),
                            Event::Finished(result.as_ref().err().copied()),
                        );
                    }
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
    #[cfg(all(test, target_os = "linux"))] trace: Option<(&GatedCallTrace, SocketAddr)>,
) -> Result<(), Refusal> {
    let source = socket.peer_addr();
    #[cfg(all(test, target_os = "linux"))]
    GatedCallTrace::server_event(trace, Event::Stage(Stage::ServerHandshake));
    stage(shared, Stage::ServerHandshake).await;
    before_dispatch(shared, deadline)?;
    #[cfg(all(test, target_os = "linux"))]
    if let Some((trace, peer)) = trace {
        trace.inbound(peer, "handshake start", None);
    }
    let stream = TlsAcceptor::from(resources.server.clone())
        .accept_with(socket, |c| c.set_buffer_limit(Some(SEND_BUFFER)))
        .await;
    #[cfg(all(test, target_os = "linux"))]
    if let Some((trace, peer)) = trace {
        trace.inbound(
            peer,
            "TLS client handshake",
            stream.as_ref().err().map(handshake_error),
        );
    }
    let mut stream = stream.map_err(|_| Refusal::Tls)?;
    let authenticated =
        AuthenticatedPeer::from_server(stream.get_ref().1).map_err(|_| Refusal::Identity)?;
    let peer = authenticated
        .authenticated_identity()
        .map_err(|_| Refusal::Identity)?;
    // TLS proved the certificate and key. Check local authority before
    // reading any application data, regardless of the peer's lane.
    let authorized = match &resources.learner {
        Some(learner) => learner
            .authorize(peer.0, peer.1, peer.2, true)
            .await
            .map(|_| ()),
        None => resources.registry.authorize(peer.0, peer.1, peer.2).await,
    };
    #[cfg(all(test, target_os = "linux"))]
    if let Some((trace, peer)) = trace {
        trace.inbound(
            peer,
            "inbound applied pre-frame authorization",
            authorized
                .as_ref()
                .err()
                .map(|reason| format!("{reason:?}")),
        );
    }
    authorized.map_err(|_| Refusal::Tls)?;
    drop(handshake);
    #[cfg(all(test, target_os = "linux"))]
    GatedCallTrace::server_event(trace, Event::Stage(Stage::ServerAuthenticated));
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
    #[cfg(any(test, target_os = "linux"))]
    if magic == control::issuer::MAGIC {
        return issuer_channel::receive(
            shared,
            &resources,
            stream,
            authenticated,
            source.map_err(|_| Refusal::Address)?,
            deadline,
        )
        .await;
    }
    if magic == control::MAGIC {
        let address =
            control::read_request(&mut stream, route, source.map_err(|_| Refusal::Address)?)
                .await?;
        // Learner bootstrap has no control authority, even for the original anchor.
        if resources.learner.is_some() {
            return Err(Refusal::Identity);
        }
        resources
            .registry
            .authorize(peer.0, peer.1, peer.2)
            .await
            .map_err(|_| Refusal::Identity)?;
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
    #[cfg(all(test, target_os = "linux"))]
    GatedCallTrace::server_event(trace, Event::Stage(Stage::RequestRead));
    stage(shared, Stage::RequestRead).await;
    before_dispatch(shared, deadline)?;
    clean_end(&mut stream).await?;
    let request = Request::decode(&frame)?;
    request.shape(resources.cluster, peer.1)?;
    let expected = request.expected();
    #[cfg(all(test, target_os = "linux"))]
    if let Some((trace, peer)) = trace {
        trace.inbound(peer, "before dispatch", None);
    }
    #[cfg(all(test, target_os = "linux"))]
    GatedCallTrace::server_event(trace, Event::Stage(Stage::BeforeDispatch));
    stage(shared, Stage::BeforeDispatch).await;
    before_dispatch(shared, deadline)?;
    if let Some(learner) = &resources.learner {
        if let Request::Append(append) = &request
            && !learner.matches_original_append(append)
        {
            return Err(Refusal::Identity);
        }
        let bootstrap_allowed = matches!(request, Request::Append(_) | Request::Snapshot(..));
        let authorized = learner
            .authorize(peer.0, peer.1, peer.2, bootstrap_allowed)
            .await;
        #[cfg(all(test, target_os = "linux"))]
        if let Some((trace, peer)) = trace {
            trace.inbound(
                peer,
                "learner dispatch authorization",
                authorized
                    .as_ref()
                    .err()
                    .map(|reason| format!("{reason:?}")),
            );
        }
        let _lane = authorized.map_err(|_| Refusal::Identity)?;
        before_dispatch(shared, deadline)?;
        #[cfg(all(test, target_os = "linux"))]
        GatedCallTrace::server_event(trace, Event::LearnerDispatch(_lane));
        #[cfg(test)]
        event(shared, Event::LearnerDispatch(_lane));
    } else {
        resources
            .registry
            .authorize(peer.0, peer.1, peer.2)
            .await
            .map_err(|_| Refusal::Identity)?;
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
    #[cfg(all(test, target_os = "linux"))]
    GatedCallTrace::server_event(trace, Event::Stage(Stage::BeforeReply));
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
        #[cfg(all(test, target_os = "linux"))] trace: Option<Arc<GatedCallTrace>>,
    ) -> Result<Reply, ExchangeError> {
        let entry = Instant::now();
        #[cfg(all(test, target_os = "linux"))]
        if let Some(trace) = &trace {
            let _ = trace.entry.set(entry);
        }
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
        #[cfg(all(test, target_os = "linux"))]
        if let Some(trace) = &trace {
            let _ = trace.soft_ttl.set(ttl);
        }
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
                        #[cfg(all(test, target_os = "linux"))]
                        trace.as_deref(),
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
    #[cfg(all(test, target_os = "linux"))] trace: Option<&GatedCallTrace>,
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
    #[cfg(all(test, target_os = "linux"))]
    if let Some(trace) = trace {
        trace.caller("configuration start", None);
    }
    let config = resources.outbound_tls_config(target);
    #[cfg(all(test, target_os = "linux"))]
    if let Some(trace) = trace {
        trace.caller("configuration", trace_refusal(config.as_ref().err()));
    }
    let config = config?;
    stage(shared, Stage::BeforeConnect).await;
    before_dispatch(shared, deadline)?;
    if caller.is_closed() {
        return Err(Refusal::Closed);
    }
    #[cfg(all(test, target_os = "linux"))]
    if let Some(trace) = trace {
        trace.caller("connect start", None);
    }
    let socket = TcpStream::connect(address).await;
    #[cfg(all(test, target_os = "linux"))]
    if let Some(trace) = trace {
        if let Ok(socket) = &socket
            && let Ok(address) = socket.local_addr()
        {
            trace.set_source(address);
        }
        trace.caller(
            "connect",
            socket
                .as_ref()
                .err()
                .map(|error| format!("socket {:?}", error.kind())),
        );
    }
    let socket = socket.map_err(|_| Refusal::Tls)?;
    stage(shared, Stage::ClientHandshake).await;
    before_dispatch(shared, deadline)?;
    if caller.is_closed() {
        return Err(Refusal::Closed);
    }
    let name = ServerName::try_from(verify::SERVER_NAME);
    #[cfg(all(test, target_os = "linux"))]
    if let Some(trace) = trace {
        if name.is_err() {
            trace.caller("configuration", Some("server name".into()));
        } else {
            trace.caller("handshake start", None);
        }
    }
    let stream = TlsConnector::from(config)
        .connect_with(name.map_err(|_| Refusal::Tls)?, socket, |c| {
            c.set_buffer_limit(Some(SEND_BUFFER))
        })
        .await;
    #[cfg(all(test, target_os = "linux"))]
    if let Some(trace) = trace {
        trace.caller(
            "TLS server handshake",
            stream.as_ref().err().map(handshake_error),
        );
    }
    let mut stream = stream.map_err(|_| Refusal::Tls)?;
    let peer = AuthenticatedPeer::from_client(stream.get_ref().1)
        .and_then(|peer| peer.authenticated_identity())
        .map_err(|_| Refusal::Identity)?;
    #[cfg(test)]
    stage(shared, Stage::BeforeClientAuthorization).await;
    before_dispatch(shared, deadline)?;
    if caller.is_closed() {
        return Err(Refusal::Closed);
    }
    let authorized = resources.registry.authorize(peer.0, peer.1, peer.2).await;
    #[cfg(all(test, target_os = "linux"))]
    if let Some(trace) = trace {
        trace.caller(
            "outbound applied pre-frame authorization",
            authorized
                .as_ref()
                .err()
                .map(|reason| format!("{reason:?}")),
        );
    }
    authorized.map_err(|_| Refusal::Tls)?;
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
    #[cfg(all(test, target_os = "linux"))]
    if let Some(trace) = trace {
        trace.caller("reply read", None);
    }
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
    resources
        .registry
        .authorize(peer.0, peer.1, peer.2)
        .await
        .map_err(|_| Refusal::Identity)?;
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
    #[cfg(test)]
    BeforeClientAuthorization,
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
#[cfg(all(test, target_os = "linux"))]
pub(super) struct GatedCallerOutcome {
    pub(super) source: Option<SocketAddr>,
    pub(super) result: Result<(), Refusal>,
}

// Record only the selected caller. Accept may precede publication of its
// local endpoint, so keep a bounded buffer and match by endpoint at readout.
#[cfg(all(test, target_os = "linux"))]
#[derive(Default)]
pub(super) struct GatedCallTrace {
    entry: std::sync::OnceLock<Instant>,
    soft_ttl: std::sync::OnceLock<Duration>,
    source: std::sync::OnceLock<SocketAddr>,
    caller: Mutex<Vec<TimedStage>>,
    inbound: Mutex<Vec<(SocketAddr, TimedStage)>>,
    server_events: Mutex<GatedServerEvents>,
    server_changed: tokio::sync::Notify,
}

#[cfg(all(test, target_os = "linux"))]
const GATED_EVENT_LIMIT: usize = 8 * CONNECTIONS;

#[cfg(all(test, target_os = "linux"))]
#[derive(Default)]
struct GatedServerEvents {
    candidates: Vec<(SocketAddr, Instant, Event)>,
    unrelated: Vec<(Duration, Event)>,
    candidates_overflowed: bool,
    unrelated_omitted: usize,
}
#[cfg(all(test, target_os = "linux"))]
type TimedStage = (Duration, &'static str, Option<String>);

#[cfg(all(test, target_os = "linux"))]
impl GatedCallTrace {
    fn set_source(&self, source: SocketAddr) {
        let _ = self.source.set(source);
        self.server_changed.notify_one();
    }

    fn server_event(trace: Option<(&Self, SocketAddr)>, event: Event) {
        let Some((trace, peer)) = trace else { return };
        let Some(entry) = trace.entry.get() else {
            return;
        };
        let at = Instant::now();
        let mut events = trace.server_events.lock().unwrap();
        if trace.source.get().is_some_and(|source| *source != peer) {
            if events.unrelated.len() < GATED_EVENT_LIMIT {
                events.unrelated.push((at.duration_since(*entry), event));
            } else {
                events.unrelated_omitted = events.unrelated_omitted.saturating_add(1);
            }
            return;
        }
        if events.candidates.len() < GATED_EVENT_LIMIT {
            events.candidates.push((peer, at, event));
        } else {
            events.candidates_overflowed = true;
        }
        drop(events);
        trace.server_changed.notify_one();
    }

    // The accept task may report stages before the caller publishes its endpoint.
    // Keep those candidates until a single reader can correlate them.
    pub(super) async fn next_server_event(
        &self,
        cursor: &mut usize,
    ) -> Result<(SocketAddr, Instant, Event), &'static str> {
        loop {
            if let Some(source) = self.source.get() {
                let events = self.server_events.lock().unwrap();
                if events.candidates_overflowed {
                    return Err("gated transport observation overflowed");
                }
                for &(peer, at, event) in &events.candidates[*cursor..] {
                    *cursor += 1;
                    if peer == *source {
                        return Ok((peer, at, event));
                    }
                }
            }
            self.server_changed.notified().await;
        }
    }

    pub(super) fn caller(&self, stage: &'static str, error: Option<String>) {
        if let Some(entry) = self.entry.get() {
            self.caller
                .lock()
                .unwrap()
                .push((entry.elapsed(), stage, error));
        }
    }

    fn inbound(&self, peer: SocketAddr, stage: &'static str, error: Option<String>) {
        if let Some(entry) = self.entry.get()
            && self.source.get().is_none_or(|source| *source == peer)
        {
            let elapsed = entry.elapsed();
            let mut inbound = self.inbound.lock().unwrap();
            // At most eight observations per accepted connection.
            if inbound.len() < 8 * CONNECTIONS {
                inbound.push((peer, (elapsed, stage, error)));
            }
        }
    }

    // Endpoints are only correlation keys and never escape the trace.
    pub(super) fn summary(&self) -> impl std::fmt::Debug {
        let inbound = self
            .inbound
            .lock()
            .unwrap()
            .iter()
            .filter(|(peer, _)| Some(peer) == self.source.get())
            .map(|(_, stage)| stage.clone())
            .collect::<Vec<_>>();
        let events = self.server_events.lock().unwrap();
        let mut unrelated = Vec::new();
        if let (Some(source), Some(entry)) = (self.source.get(), self.entry.get()) {
            unrelated.extend(
                events
                    .candidates
                    .iter()
                    .filter(|(peer, _, _)| peer != source)
                    .map(|(_, at, event)| (at.duration_since(*entry), *event)),
            );
        }
        unrelated.extend_from_slice(&events.unrelated);
        (
            self.soft_ttl.get().copied(),
            self.caller.lock().unwrap().clone(),
            inbound,
            unrelated,
            events.unrelated_omitted,
        )
    }
}

#[cfg(all(test, target_os = "linux"))]
fn trace_refusal(reason: Option<&Refusal>) -> Option<String> {
    reason.map(|reason| format!("{reason:?}"))
}

#[cfg(all(test, target_os = "linux"))]
fn handshake_error(error: &std::io::Error) -> String {
    use rustls::{CertificateError, Error};
    use verify::VerifyRefusal;

    match error
        .get_ref()
        .and_then(|error| error.downcast_ref::<Error>())
    {
        Some(Error::FailedToGetCurrentTime) => "TLS clock unavailable".into(),
        Some(Error::InvalidCertificate(
            CertificateError::Expired
            | CertificateError::ExpiredContext { .. }
            | CertificateError::NotValidYet
            | CertificateError::NotValidYetContext { .. },
        )) => "TLS certificate time".into(),
        Some(Error::InvalidCertificate(CertificateError::Other(other))) => {
            match other.0.downcast_ref::<VerifyRefusal>() {
                Some(reason) => format!("TLS peer verification {reason:?}"),
                None => "TLS certificate".into(),
            }
        }
        Some(Error::InvalidCertificate(_)) => "TLS certificate".into(),
        Some(Error::AlertReceived(alert)) => format!("TLS peer alert {alert:?}"),
        Some(_) => "TLS protocol".into(),
        None => format!("socket {:?}", error.kind()),
    }
}

#[cfg(all(test, target_os = "linux"))]
type GatedCaller = (
    NodeId,
    Arc<GatedCallTrace>,
    oneshot::Sender<GatedCallerOutcome>,
);

#[cfg(test)]
#[derive(Default)]
struct TestHooks {
    drop_issuer_ack: std::sync::atomic::AtomicBool,
    events: Mutex<Option<tokio::sync::mpsc::Sender<Event>>>,
    gates: Mutex<std::collections::BTreeMap<Stage, Arc<Semaphore>>>,
    #[cfg(all(test, target_os = "linux"))]
    gated_caller: Mutex<Option<GatedCaller>>,
    #[cfg(all(test, target_os = "linux"))]
    gated_inbound: std::sync::OnceLock<Weak<GatedCallTrace>>,
}
#[cfg(test)]
impl TestHooks {
    #[cfg(all(test, target_os = "linux"))]
    fn take_gated_caller(
        &self,
        target: Option<NodeId>,
    ) -> Option<(Arc<GatedCallTrace>, oneshot::Sender<GatedCallerOutcome>)> {
        let mut armed = self.gated_caller.lock().unwrap();
        if target != armed.as_ref().map(|(target, _, _)| *target) {
            return None;
        }
        armed.take().map(|(_, trace, sender)| (trace, sender))
    }

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
#[cfg(test)]
pub(super) use tests::admission_test_peer;
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
        #[cfg(all(test, target_os = "linux"))]
        if let Some((trace, observe)) = self
            .shared
            .upgrade()
            .and_then(|shared| shared.hooks.take_gated_caller(self.target))
        {
            let result = match self
                .exchange(
                    Request::Append(r),
                    o,
                    Box::pin(std::future::pending()),
                    Some(trace.clone()),
                )
                .await
            {
                Ok(Reply::Append(reply)) => Ok(reply),
                Ok(_) => Err(ExchangeError::Refused(Refusal::Frame)),
                Err(error) => Err(error),
            };
            // This is the original caller's result after exchange checks the
            // RPC deadline. The spawned outbound task can finish earlier.
            let outcome = result.as_ref().map(|_| ()).map_err(|error| match error {
                ExchangeError::Refused(reason) => *reason,
                ExchangeError::Cancelled(_) => Refusal::Closed,
            });
            trace.caller("caller returned", trace_refusal(outcome.as_ref().err()));
            let _ = observe.send(GatedCallerOutcome {
                source: trace.source.get().copied(),
                result: outcome,
            });
            return result.map_err(unary_error);
        }
        match self
            .exchange(
                Request::Append(r),
                o,
                Box::pin(std::future::pending()),
                #[cfg(all(test, target_os = "linux"))]
                None,
            )
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
            .exchange(
                Request::Snapshot(v, s),
                o,
                Box::pin(cancel),
                #[cfg(all(test, target_os = "linux"))]
                None,
            )
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
            .exchange(
                Request::Ballot(r, p),
                o,
                Box::pin(std::future::pending()),
                #[cfg(all(test, target_os = "linux"))]
                None,
            )
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
    let registry = peer_registry::AppliedRegistry::new(sm);
    // PeerVerifier still checks the chain and cluster identity. The owned
    // inbound exchange performs the store-dependent check before framing.
    let verifier = PeerVerifier::new(
        credentials.root.clone(),
        cluster,
        Arc::new(|_, _, _| Ok(())),
    )
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
                #[cfg(any(test, target_os = "linux"))]
                voter_receiver: None,
                #[cfg(any(test, target_os = "linux"))]
                voter_transfers: [const { None }; 2],
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
    #[cfg(test)]
    pub(super) fn attach_founder(
        &mut self,
        sm: &SmAdapter,
        dispatch: Arc<dyn AppendDispatch>,
        address: &str,
        credentials: Credentials,
    ) -> Result<(), Refusal> {
        self.attach_founder_on(sm, dispatch, address, credentials, None)
    }

    #[cfg(any(test, target_os = "linux"))]
    pub(super) fn attach_founder_on(
        &mut self,
        sm: &SmAdapter,
        dispatch: Arc<dyn AppendDispatch>,
        address: &str,
        credentials: Credentials,
        inherited: Option<std::net::TcpListener>,
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
        let listener = activation_listener(endpoint(address)?, inherited)?;
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
        inherited: Option<std::net::TcpListener>,
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
        let listener = activation_listener(endpoint(&address.to_string())?, inherited)?;
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

    #[cfg(all(test, target_os = "linux"))]
    pub(super) fn observe_gated_inbound(&self, trace: &Arc<GatedCallTrace>) {
        assert!(
            self.shared
                .hooks
                .gated_inbound
                .set(Arc::downgrade(trace))
                .is_ok()
        );
    }

    #[cfg(all(test, target_os = "linux"))]
    pub(super) fn observe_gated_caller(
        &self,
        target: NodeId,
        trace: Arc<GatedCallTrace>,
    ) -> oneshot::Receiver<GatedCallerOutcome> {
        let (sender, receiver) = oneshot::channel();
        assert!(
            self.shared
                .hooks
                .gated_caller
                .lock()
                .unwrap()
                .replace((target, trace, sender))
                .is_none()
        );
        receiver
    }

    #[cfg(all(test, target_os = "linux"))]
    pub(super) fn arm_before_dispatch(&self) {
        let gate = Arc::new(Semaphore::new(0));
        assert!(
            self.shared
                .hooks
                .gates
                .lock()
                .unwrap()
                .insert(Stage::BeforeDispatch, gate)
                .is_none()
        );
    }

    #[cfg(all(test, target_os = "linux"))]
    pub(super) fn release_before_dispatch(&self) -> Option<Instant> {
        let mut gates = self.shared.hooks.gates.lock().unwrap();
        let gate = gates.remove(&Stage::BeforeDispatch)?;
        gate.add_permits(64);
        drop(gates);
        Some(Instant::now())
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

// A response deadline and the writer compete to settle an unsent request.
// Once the write can start, I/O failure cannot establish non-delivery.
#[derive(Default)]
struct AdmissionWrite(AtomicU8);

impl AdmissionWrite {
    const UNSENT: u8 = 0;
    const WITHHELD: u8 = 1;
    const POSSIBLE: u8 = 2;

    fn begin(&self, shared: &Shared, deadline: Instant) -> Result<(), Refusal> {
        // Use the same gate as close through the transition to a possible write.
        let state = shared.state.lock().map_err(|_| Refusal::Closed)?;
        if state.closed || *shared.stop.borrow() {
            return Err(Refusal::Closed);
        }
        if shared.failed.load(AtomicOrdering::Acquire) {
            return Err(Refusal::Dispatch);
        }
        if Instant::now() >= deadline {
            return Err(Refusal::Deadline);
        }
        self.0
            .compare_exchange(
                Self::UNSENT,
                Self::POSSIBLE,
                AtomicOrdering::AcqRel,
                AtomicOrdering::Acquire,
            )
            .map(|_| ())
            .map_err(|_| Refusal::Deadline)
    }

    fn unconfirmed(&self) -> AdmissionReply {
        match self.0.compare_exchange(
            Self::UNSENT,
            Self::WITHHELD,
            AtomicOrdering::AcqRel,
            AtomicOrdering::Acquire,
        ) {
            Err(Self::POSSIBLE) => AdmissionReply::Unknown,
            _ => AdmissionReply::Refused,
        }
    }
}

pub(super) struct AdmissionResponse {
    reply: oneshot::Receiver<Result<Result<AdmissionReply, Refusal>, tokio::time::error::Elapsed>>,
    deadline: Instant,
    write: Arc<AdmissionWrite>,
}

impl AdmissionResponse {
    pub(super) async fn reply(self) -> AdmissionReply {
        match timeout_at(self.deadline, self.reply).await {
            Ok(Ok(Ok(Ok(reply)))) if Instant::now() < self.deadline => reply,
            _ => self.write.unconfirmed(),
        }
    }
}

// Admission boundary probes belong to one exchange. They use private
// channels so the existing stage and dispatch observers keep their sequence.
#[cfg(test)]
enum AdmissionWriteBoundary {
    BeforeWrite,
    AfterPrefix,
}

#[cfg(test)]
struct AdmissionWritePause {
    boundary: AdmissionWriteBoundary,
    reached: oneshot::Sender<()>,
    resume: oneshot::Receiver<()>,
}

#[cfg(test)]
impl AdmissionWritePause {
    async fn wait(self) {
        if self.reached.send(()).is_ok() {
            let _ = self.resume.await;
        }
    }
}

impl AdmissionClient {
    pub(super) fn request(&self) -> Option<AdmissionResponse> {
        self.exchange(
            self.advertised,
            #[cfg(test)]
            None,
        )
    }

    #[cfg(all(test, target_os = "linux"))]
    pub(super) fn conflicting_address(&self, advertised: SocketAddr) -> Option<AdmissionResponse> {
        self.exchange(advertised, None)
    }

    fn exchange(
        &self,
        advertised: SocketAddr,
        #[cfg(test)] pause: Option<AdmissionWritePause>,
    ) -> Option<AdmissionResponse> {
        let deadline = Instant::now() + learner_admission::BUDGET;
        let shared = self.shared.upgrade()?;
        let (tx, rx) = oneshot::channel();
        let write = Arc::new(AdmissionWrite::default());
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
            let write = write.clone();
            state.tasks.spawn(async move {
                let reply = timeout_at(
                    deadline,
                    outbound_admission(
                        &shared,
                        admission,
                        issuer,
                        advertised,
                        deadline,
                        &write,
                        #[cfg(test)]
                        pause,
                    ),
                )
                .await;
                let _ = tx.send(reply);
            });
        }
        drop(join_errors);
        Some(AdmissionResponse {
            reply: rx,
            deadline,
            write,
        })
    }
}

async fn outbound_admission(
    shared: &Shared,
    admission: Admission,
    issuer: SocketAddr,
    advertised: SocketAddr,
    deadline: Instant,
    write: &AdmissionWrite,
    #[cfg(test)] pause: Option<AdmissionWritePause>,
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
    let frame = control::request(route, advertised);
    let frame = frame.as_slice();
    #[cfg(test)]
    let pause = match pause {
        Some(
            pause @ AdmissionWritePause {
                boundary: AdmissionWriteBoundary::BeforeWrite,
                ..
            },
        ) => {
            pause.wait().await;
            None
        }
        pause => pause,
    };
    write.begin(shared, deadline)?;
    #[cfg(test)]
    let frame = if let Some(pause) = pause {
        stream
            .write_all(&frame[..1])
            .await
            .map_err(|_| Refusal::Frame)?;
        stream.flush().await.map_err(|_| Refusal::Frame)?;
        pause.wait().await;
        before_dispatch(shared, deadline)?;
        &frame[1..]
    } else {
        frame
    };
    send_frame(&mut stream, frame).await?;
    let reply = control::read_reply(&mut stream, reverse(route), advertised).await?;
    clean_end(&mut stream).await?;
    gate.authorize_admission_anchor(peer.0, peer.1, peer.2)
        .map_err(|_| Refusal::Identity)?;
    before_dispatch(shared, deadline)?;
    Ok(reply)
}

#[cfg(any(test, target_os = "linux"))]
mod issuer_channel {
    use super::*;
    use crate::cluster::{
        consensus::issuer_transfer::{Claim, Offer},
        pki::{ca::fingerprint_of, custody::voter_transfer::Wipe},
    };
    use control::issuer as wire;
    use std::sync::atomic::{AtomicU8, Ordering};
    use tokio_rustls::server::TlsStream;

    pub(super) type ReceiveFuture<'a, T> =
        std::pin::Pin<Box<dyn Future<Output = Result<T, Refusal>> + Send + 'a>>;

    pub(super) trait Receiver: Send + Sync {
        fn check(&self, claim: Claim, sender: AuthenticatedPeer) -> ReceiveFuture<'_, Offer>;
        fn install<'a>(
            &'a self,
            claim: Claim,
            sender: AuthenticatedPeer,
            channel: (&'a [u8; 32], &'a [u8], Vec<u8>),
        ) -> ReceiveFuture<'a, ()>;
    }

    pub(in crate::cluster::consensus) trait Sender:
        Send + Sync
    {
        fn seal<'a>(
            &'a self,
            offer: Offer,
            key: &'a [u8; 32],
            transcript: &'a [u8],
        ) -> impl Future<Output = Result<Vec<u8>, Refusal>> + Send + 'a;
    }

    #[cfg(target_os = "linux")]
    impl Receiver for &'static super::super::bound_start::Job {
        fn check(&self, claim: Claim, sender: AuthenticatedPeer) -> ReceiveFuture<'_, Offer> {
            Box::pin(self.check_issuer_transfer(claim, sender))
        }

        fn install<'a>(
            &'a self,
            claim: Claim,
            sender: AuthenticatedPeer,
            channel: (&'a [u8; 32], &'a [u8], Vec<u8>),
        ) -> ReceiveFuture<'a, ()> {
            Box::pin(self.install_voter_custody(claim, sender, channel))
        }
    }

    #[cfg(target_os = "linux")]
    impl Sender for &'static super::super::bound_start::Job {
        async fn seal<'a>(
            &'a self,
            offer: Offer,
            key: &'a [u8; 32],
            transcript: &'a [u8],
        ) -> Result<Vec<u8>, Refusal> {
            self.seal_voter_transfer(offer, key, transcript).await
        }
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(in crate::cluster::consensus) enum Outcome {
        Installed,
        Unknown,
        Refused,
    }

    pub(super) async fn receive(
        shared: &Shared,
        resources: &Resources,
        mut stream: TlsStream<TcpStream>,
        sender: AuthenticatedPeer,
        source: SocketAddr,
        deadline: Instant,
    ) -> Result<(), Refusal> {
        let claim = wire::read_hello(&mut stream).await?;
        let peer = sender
            .authenticated_identity()
            .map_err(|_| Refusal::Identity)?;
        if claim.root != fingerprint_of(resources.credentials.root.as_ref())
            || claim.receiver.0 != resources.node
            || (peer.0, peer.1, *peer.2.as_bytes())
                != (claim.cluster, claim.sender.0, claim.sender.1)
        {
            return Err(Refusal::Identity);
        }
        let receiver = shared
            .state
            .lock()
            .map_err(|_| Refusal::Closed)?
            .voter_receiver
            .clone()
            .ok_or(Refusal::NotReady)?;
        check_receiver(shared, deadline, receiver.as_ref(), claim, sender, source).await?;
        let mut challenge = [0; 32];
        getrandom::fill(&mut challenge).map_err(|_| Refusal::Tls)?;
        let transcript = claim.transcript(&challenge);
        let mut key = Wipe([0; 32]);
        stream
            .get_ref()
            .1
            .export_keying_material(&mut key.0[..], ISSUER_EXPORTER_LABEL, Some(&transcript))
            .map_err(|_| Refusal::Tls)?;
        wire::challenge(&mut stream, &challenge).await?;
        let sealed = wire::read_sealed(&mut stream).await?;
        check_receiver(shared, deadline, receiver.as_ref(), claim, sender, source).await?;
        receiver
            .install(claim, sender, (&key.0, &transcript, sealed))
            .await?;
        drop(key);
        check_receiver(shared, deadline, receiver.as_ref(), claim, sender, source).await?;
        #[cfg(test)]
        if shared.hooks.drop_issuer_ack.swap(false, Ordering::AcqRel) {
            return Ok(());
        }
        wire::acknowledge(&mut stream, &challenge).await
    }

    async fn check_receiver(
        shared: &Shared,
        deadline: Instant,
        receiver: &dyn Receiver,
        claim: Claim,
        sender: AuthenticatedPeer,
        source: SocketAddr,
    ) -> Result<(), Refusal> {
        before_dispatch(shared, deadline)?;
        let offer = receiver.check(claim, sender).await?;
        before_dispatch(shared, deadline)?;
        if source.ip() != offer.sender_address.ip() {
            return Err(Refusal::Address);
        }
        Ok(())
    }

    const CHECKING: u8 = 0;
    const UNSPENT: u8 = 1;
    const RESERVED: u8 = 2;

    pub(super) struct AttemptState {
        disposition: AtomicU8,
    }

    impl AttemptState {
        fn available(&self) -> bool {
            self.disposition.load(Ordering::SeqCst) == UNSPENT
        }

        fn refuse_or_unknown(&self) -> Outcome {
            match self.disposition.compare_exchange(
                CHECKING,
                UNSPENT,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Err(RESERVED) => Outcome::Unknown,
                _ => Outcome::Refused,
            }
        }
    }

    struct ChallengeCheck(Arc<AttemptState>);

    impl ChallengeCheck {
        fn new() -> Self {
            Self(Arc::new(AttemptState {
                disposition: AtomicU8::new(CHECKING),
            }))
        }

        async fn build_once<T>(
            self,
            build: impl Future<Output = Result<T, Refusal>>,
        ) -> Result<T, Refusal> {
            self.0
                .disposition
                .compare_exchange(CHECKING, RESERVED, Ordering::SeqCst, Ordering::SeqCst)
                .map_err(|_| Refusal::Identity)?;
            build.await
        }
    }

    impl Drop for ChallengeCheck {
        fn drop(&mut self) {
            let _ = self.0.disposition.compare_exchange(
                CHECKING,
                UNSPENT,
                Ordering::SeqCst,
                Ordering::SeqCst,
            );
        }
    }

    pub(in crate::cluster::consensus) struct Transfer {
        reply: oneshot::Receiver<Result<(), Refusal>>,
        attempt: Arc<AttemptState>,
        deadline: Instant,
    }

    impl Transfer {
        pub(in crate::cluster::consensus) async fn outcome(self) -> Outcome {
            match timeout_at(self.deadline, self.reply).await {
                Ok(Ok(Ok(()))) if Instant::now() < self.deadline => Outcome::Installed,
                _ => self.attempt.refuse_or_unknown(),
            }
        }
    }

    #[cfg(test)]
    #[tokio::test]
    async fn c13_challenge_refusal_is_unspent_and_reserved_failure_is_terminal() {
        let check = ChallengeCheck::new();
        let (complete, reply) = oneshot::channel();
        let outcome = Transfer {
            reply,
            attempt: check.0.clone(),
            deadline: Instant::now(),
        };
        let mut built = 0;
        // Hold the sender at the reservation boundary while the caller expires.
        assert_eq!(outcome.outcome().await, Outcome::Refused);
        assert!(check.0.available());
        assert_eq!(
            check
                .build_once(async {
                    built += 1;
                    Ok(())
                })
                .await,
            Err(Refusal::Identity),
        );
        drop(complete);
        assert_eq!(built, 0);

        let check = ChallengeCheck::new();
        let state = check.0.clone();
        let (_complete, reply) = oneshot::channel();
        let outcome = Transfer {
            reply,
            attempt: state.clone(),
            deadline: Instant::now(),
        };
        // The sender wins reservation before the caller observes its deadline.
        let (started, entered) = oneshot::channel();
        let build = async {
            check
                .build_once(async {
                    built += 1;
                    started.send(()).unwrap();
                    std::future::pending::<Result<(), Refusal>>().await
                })
                .await
        };
        {
            tokio::pin!(build);
            tokio::select! {
                _ = &mut build => panic!("package construction must be retained"),
                result = entered => result.unwrap(),
            }
            assert_eq!(outcome.outcome().await, Outcome::Unknown);
        }
        assert!(!state.available());
        assert_eq!(built, 1);
    }

    impl TransportOwner {
        #[cfg(target_os = "linux")]
        pub(in crate::cluster::consensus) fn receive_issuer_transfer(
            &mut self,
            receiver: &'static super::super::bound_start::Job,
        ) -> Result<(), Refusal> {
            let mut state = self.shared.state.lock().map_err(|_| Refusal::Closed)?;
            if state.closed || state.voter_receiver.is_some() {
                return Err(Refusal::Closed);
            }
            if state
                .resources
                .as_ref()
                .is_none_or(|resources| resources.learner.is_none())
            {
                return Err(Refusal::NotReady);
            }
            state.voter_receiver = Some(Arc::new(receiver));
            Ok(())
        }

        pub(in crate::cluster::consensus) fn send_issuer_transfer(
            &self,
            sender: impl Sender + 'static,
            offer: Offer,
        ) -> Result<Transfer, Refusal> {
            let shared = &self.shared;
            let (tx, reply) = oneshot::channel();
            let deadline = Instant::now() + INBOUND_BUDGET;
            let check = ChallengeCheck::new();
            let attempt = check.0.clone();
            let mut state = shared.state.lock().map_err(|_| Refusal::Closed)?;
            for slot in &mut state.voter_transfers {
                if slot.as_ref().is_some_and(|(_, state)| state.available()) {
                    *slot = None;
                }
            }
            let target = offer.claim.receiver.0;
            if state
                .voter_transfers
                .iter()
                .flatten()
                .any(|(id, _)| *id == target)
            {
                return Err(Refusal::Identity);
            }
            let free = state
                .voter_transfers
                .iter()
                .position(Option::is_none)
                .ok_or(Refusal::Busy)?;
            let admission = shared.admission(&mut state);
            let join_errors = if state.closed {
                Vec::new()
            } else {
                std::mem::take(&mut state.join_errors)
            };
            if let Ok(admission) = admission {
                state.voter_transfers[free] = Some((target, attempt.clone()));
                let shared = shared.clone();
                state.tasks.spawn(async move {
                    let result = timeout_at(
                        deadline,
                        send(&shared, admission, sender, offer, deadline, check),
                    )
                    .await
                    .unwrap_or(Err(Refusal::Deadline));
                    let _ = tx.send(result);
                });
            } else {
                let refusal = admission.err().expect("refused admission");
                drop(state);
                drop(join_errors);
                return Err(refusal);
            }
            drop(state);
            drop(join_errors);
            Ok(Transfer {
                reply,
                attempt,
                deadline,
            })
        }
    }

    async fn send(
        shared: &Shared,
        admission: Admission,
        sender: impl Sender,
        offer: Offer,
        deadline: Instant,
        check: ChallengeCheck,
    ) -> Result<(), Refusal> {
        let Admission {
            resources,
            connection: _connection,
            handshake,
        } = admission;
        let claim = offer.claim;
        if resources.learner.is_some()
            || resources.node != claim.sender.0
            || resources.cluster != claim.cluster
            || fingerprint_of(resources.credentials.root.as_ref()) != claim.root
        {
            return Err(Refusal::Identity);
        }
        before_dispatch(shared, deadline)?;
        let config = resources.outbound_tls_config(claim.receiver.0)?;
        let socket = TcpStream::connect(offer.receiver_address)
            .await
            .map_err(|_| Refusal::Tls)?;
        if socket.local_addr().map_err(|_| Refusal::Address)?.ip() != offer.sender_address.ip()
            || socket.peer_addr().map_err(|_| Refusal::Address)? != offer.receiver_address
        {
            return Err(Refusal::Address);
        }
        let mut stream = TlsConnector::from(config)
            .connect_with(
                ServerName::try_from(verify::SERVER_NAME).map_err(|_| Refusal::Tls)?,
                socket,
                |connection| connection.set_buffer_limit(Some(SEND_BUFFER)),
            )
            .await
            .map_err(|_| Refusal::Tls)?;
        let peer = AuthenticatedPeer::from_client(stream.get_ref().1)
            .and_then(|peer| peer.authenticated_identity())
            .map_err(|_| Refusal::Identity)?;
        drop(handshake);
        if (peer.0, peer.1, *peer.2.as_bytes())
            != (claim.cluster, claim.receiver.0, claim.receiver.1)
        {
            return Err(Refusal::Identity);
        }
        resources
            .registry
            .authorize(peer.0, peer.1, peer.2)
            .await
            .map_err(|_| Refusal::Identity)?;
        before_dispatch(shared, deadline)?;
        wire::hello(&mut stream, claim).await?;
        let challenge = wire::read_challenge(&mut stream).await?;
        let transcript = claim.transcript(&challenge);
        let mut key = Wipe([0; 32]);
        stream
            .get_ref()
            .1
            .export_keying_material(&mut key.0[..], ISSUER_EXPORTER_LABEL, Some(&transcript))
            .map_err(|_| Refusal::Tls)?;
        before_dispatch(shared, deadline)?;
        let sealed = check
            .build_once(async { sender.seal(offer, &key.0, &transcript).await })
            .await?;
        drop(key);
        before_dispatch(shared, deadline)?;
        wire::sealed(&mut stream, &sealed).await?;
        wire::read_acknowledgement(&mut stream, &challenge).await?;
        resources
            .registry
            .authorize(peer.0, peer.1, peer.2)
            .await
            .map_err(|_| Refusal::Identity)?;
        before_dispatch(shared, deadline)
    }
}

#[cfg(any(test, target_os = "linux"))]
pub(super) use issuer_channel::Outcome as TransferOutcome;
