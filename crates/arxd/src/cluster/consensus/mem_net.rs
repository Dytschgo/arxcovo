//! Bounded in-process RPC test double. No production transport.
use super::*;
use openraft::{
    errors::{RPCError, ReplicationClosed, StreamingError, Unreachable},
    network::{RPCOption, RaftNetworkFactory, RaftNetworkV2},
    raft::{AppendEntriesRequest, AppendResult, SnapshotResponse, VoteRequest, VoteResponse},
    type_config::alias::{SnapshotOf, VoteOf},
};
use std::sync::atomic::{AtomicUsize, Ordering};
#[derive(Default)]
pub(super) struct MemRouter {
    pub nodes: Mutex<BTreeMap<u64, Engine>>,
    pub isolated: Mutex<BTreeSet<u64>>,
    pub mismatch: AtomicUsize,
    pub append_max: AtomicUsize,
    pub snapshot_max: AtomicUsize,
    pub snapshots: AtomicUsize,
}
#[derive(Clone)]
pub(super) struct Factory {
    pub source: u64,
    pub router: Arc<MemRouter>,
}
pub(super) struct MemNetwork {
    source: u64,
    target: u64,
    router: Arc<MemRouter>,
}
fn unreachable() -> Unreachable<Arx> {
    Unreachable::new(&std::io::Error::other("in-memory peer unavailable"))
}
impl MemNetwork {
    fn remote(&self, sender: u64) -> Result<Engine, Unreachable<Arx>> {
        if sender != self.source {
            self.router.mismatch.fetch_add(1, Ordering::Relaxed);
            return Err(unreachable());
        }
        let isolated = self.router.isolated.lock().unwrap();
        if isolated.contains(&self.source) || isolated.contains(&self.target) {
            return Err(unreachable());
        }
        self.router
            .nodes
            .lock()
            .unwrap()
            .get(&self.target)
            .cloned()
            .ok_or_else(unreachable)
    }
}
impl RaftNetworkFactory<Arx> for Factory {
    type Network = MemNetwork;
    async fn new_client(&mut self, target: u64, _node: &BasicNode) -> MemNetwork {
        MemNetwork {
            source: self.source,
            target,
            router: self.router.clone(),
        }
    }
}
impl RaftNetworkV2<Arx> for MemNetwork {
    type SnapshotData = Vec<u8>;
    async fn append_entries(
        &mut self,
        rpc: AppendEntriesRequest<Arx>,
        _option: RPCOption,
    ) -> Result<AppendResult<Arx>, RPCError<Arx>> {
        self.router
            .append_max
            .fetch_max(rpc.entries.len(), Ordering::Relaxed);
        if rpc.entries.len() > 64 {
            return Err(RPCError::Unreachable(unreachable()));
        }
        self.remote(rpc.vote.leader_id.node_id)?
            .append_entries(rpc)
            .await
            .map_err(|_| RPCError::Unreachable(unreachable()))
    }
    async fn vote(
        &mut self,
        rpc: VoteRequest<Arx>,
        _option: RPCOption,
    ) -> Result<VoteResponse<Arx>, RPCError<Arx>> {
        self.remote(rpc.vote.leader_id.node_id)?
            .vote(rpc)
            .await
            .map_err(|_| RPCError::Unreachable(unreachable()))
    }
    async fn full_snapshot(
        &mut self,
        vote: VoteOf<Arx>,
        snapshot: SnapshotOf<Arx, Vec<u8>>,
        _cancel: impl std::future::Future<Output = ReplicationClosed> + Send + 'static,
        _option: RPCOption,
    ) -> Result<SnapshotResponse<Arx>, StreamingError<Arx>> {
        self.router
            .snapshot_max
            .fetch_max(snapshot.snapshot.len(), Ordering::Relaxed);
        if snapshot.snapshot.len() > 16384 {
            return Err(StreamingError::Unreachable(unreachable()));
        }
        self.router.snapshots.fetch_add(1, Ordering::Relaxed);
        self.remote(vote.leader_id.node_id)?
            .install_full_snapshot(vote, snapshot)
            .await
            .map_err(|_| StreamingError::Unreachable(unreachable()))
    }
}
