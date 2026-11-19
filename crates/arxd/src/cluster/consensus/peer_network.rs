//! Private composition: the caller retains it through explicit two-phase close.
use super::peer_transport::{
    AppendDispatch, BallotFuture, Credentials, DispatchFuture, Health, PeerFactory, Refusal,
    SnapshotFuture, TransportOwner,
};
use super::*;
use crate::cluster::enrollment::{ClusterId, NodeId};
use openraft::type_config::alias::{SnapshotOf, VoteOf};
pub(super) struct EngineDispatch {
    pub(super) raft: Engine,
}
impl AppendDispatch for EngineDispatch {
    fn append(&self, r: openraft::raft::AppendEntriesRequest<Arx>) -> DispatchFuture {
        let e = self.raft.clone();
        Box::pin(async move { e.append_entries(r).await.map_err(|_| Refusal::Dispatch) })
    }
    fn ballot(&self, r: openraft::raft::VoteRequest<Arx>, pre: bool) -> BallotFuture {
        let e = self.raft.clone();
        Box::pin(async move {
            if pre {
                e.pre_vote(r).await
            } else {
                e.vote(r).await
            }
            .map_err(|_| Refusal::Dispatch)
        })
    }
    fn snapshot(&self, v: VoteOf<Arx>, s: SnapshotOf<Arx, Vec<u8>>) -> SnapshotFuture {
        let e = self.raft.clone();
        Box::pin(async move {
            e.install_full_snapshot(v, s)
                .await
                .map_err(|_| Refusal::Dispatch)
        })
    }
}
#[must_use = "retain through explicit transport-before-node shutdown"]
pub(super) struct NetworkedNode {
    transport: TransportOwner,
    factory: PeerFactory,
    node: Option<ConsensusNode>,
    cluster: ClusterId,
    id: NodeId,
    attempted: bool,
}
#[derive(Debug)]
pub(super) enum NetworkError {
    Start(StartError, CloseReport),
    Transport(Refusal, CloseReport),
    Identity,
}
#[derive(Debug)]
pub(super) struct CloseReport {
    pub transport: Result<(), Refusal>,
    pub node: Option<Result<(), Orphaned>>,
    pub engine_unavailable: bool,
}
impl NetworkedNode {
    pub(super) fn pending(cluster: ClusterId, id: NodeId, credentials: Credentials) -> Self {
        let (transport, factory) = TransportOwner::pending(cluster, id, credentials);
        Self {
            transport,
            factory,
            node: None,
            cluster,
            id,
            attempted: false,
        }
    }
    pub(super) fn factory(&self) -> PeerFactory {
        self.factory.clone()
    }
    pub(super) fn health(&self) -> Health {
        use openraft::type_config::async_runtime::watch::WatchReceiver;
        if self
            .node
            .as_ref()
            .and_then(|n| n.raft.as_ref())
            .is_some_and(|r| r.metrics().borrow_watched().running_state.is_err())
        {
            self.transport.observe_failure();
        }
        self.transport.health()
    }
    // On an attach/bind failure the node stays owned here for the same close route.
    // Cancellation before ConsensusNode::start returns retains its original API limitation.
    pub(super) async fn start(
        &mut self,
        dir: &Path,
        init: Init,
        params: Params,
        address: &str,
    ) -> Result<(), NetworkError> {
        if self.attempted || init.cluster != self.cluster || self.health() != Health::Pending {
            return Err(NetworkError::Identity);
        }
        self.attempted = true;
        match ConsensusNode::start(self.id.get(), dir, init, params, self.factory.clone()).await {
            Ok(node) => self.node = Some(node),
            Err(reason) => {
                let cleanup = self.close().await;
                return Err(NetworkError::Start(reason, cleanup));
            }
        }
        let attached = self.attach_node(address).await;
        match attached {
            Ok(()) => Ok(()),
            Err(reason) => {
                let cleanup = self.close().await;
                Err(NetworkError::Transport(reason, cleanup))
            }
        }
    }
    async fn attach_node(&mut self, address: &str) -> Result<(), Refusal> {
        let node = self.node.as_ref().ok_or(Refusal::Identity)?;
        let sm = node.sm.as_ref().ok_or(Refusal::Identity)?;
        if node.id != self.id.get() {
            return Err(Refusal::Identity);
        }
        {
            let state = sm.0.try_lock().map_err(|_| Refusal::Identity)?;
            StateStore::inspect_snapshot(&state.snapshot(), self.cluster)
                .map_err(|_| Refusal::Identity)?;
            if let Some((_, members)) = state.membership() {
                let members = super::decode(members).map_err(|_| Refusal::Identity)?;
                if members
                    .get_node(&self.id.get())
                    .is_none_or(|n| n.addr != address)
                {
                    return Err(Refusal::Address);
                }
            }
        }
        let raft = node.raft.clone().ok_or(Refusal::Identity)?;
        self.transport
            .attach(sm, Arc::new(EngineDispatch { raft }), address)
            .await
    }
    pub(super) async fn close(&mut self) -> CloseReport {
        let mut engine_unavailable = self.health() == Health::EngineUnavailable;
        let transport = self.transport.close().await;
        engine_unavailable |= self.health() == Health::EngineUnavailable;
        // Cancellation or poisoned ownership cannot be mistaken for a completed join.
        let node = if self.transport.joined() {
            match self.node.as_mut() {
                Some(n) => Some(n.shutdown().await),
                None => None,
            }
        } else {
            None
        };
        CloseReport {
            transport,
            node,
            engine_unavailable,
        }
    }
}
