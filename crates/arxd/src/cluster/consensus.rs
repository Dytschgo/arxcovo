//! Real OpenRaft boundary over durable local stores; no production ingress.
#![allow(dead_code)]
use super::{
    enrollment::{Command, Outcome, codec::Reader},
    log_store::StoredVote,
    state_store::{Entry as StoredEntry, LogPos, MembershipBlob, StoreError},
};
use openraft::{BasicNode, Entry, EntryPayload, LogId, Membership, Vote};
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
mod tests;
openraft::declare_raft_types!(pub Arx: D = Command, R = Applied);
#[derive(Clone, Debug)]
pub struct Applied(pub Option<Outcome>);
type Id = LogId<openraft::impls::leader_id_adv::LeaderId<u64, u64>>;
type Members = Membership<u64, BasicNode>;
fn id(p: LogPos) -> Id {
    LogId::new(
        openraft::impls::leader_id_adv::LeaderId {
            term: p.term,
            node_id: p.leader,
        },
        p.index,
    )
}
fn pos(p: Id) -> LogPos {
    LogPos {
        term: p.leader_id.term,
        leader: p.leader_id.node_id,
        index: p.index,
    }
}
fn vote(v: StoredVote) -> Vote<openraft::impls::leader_id_adv::LeaderId<u64, u64>> {
    Vote {
        leader_id: openraft::impls::leader_id_adv::LeaderId {
            term: v.term,
            node_id: v.node,
        },
        committed: v.committed,
    }
}
fn stored_vote(v: Vote<openraft::impls::leader_id_adv::LeaderId<u64, u64>>) -> StoredVote {
    StoredVote {
        term: v.leader_id.term,
        node: v.leader_id.node_id,
        committed: v.committed,
    }
}
fn encode(m: &Members) -> Result<MembershipBlob, StoreError> {
    let configs = m.get_joint_config();
    let nodes: Vec<_> = m.nodes().collect();
    if !(1..=2).contains(&configs.len()) || !(1..=16).contains(&nodes.len()) {
        return Err(StoreError::Membership);
    }
    let mut b = vec![1, configs.len() as u8];
    for c in configs {
        if !(1..=16).contains(&c.len()) {
            return Err(StoreError::Membership);
        }
        b.push(c.len() as u8);
        for n in c {
            if *n == 0 || !nodes.iter().any(|(i, _)| *i == n) {
                return Err(StoreError::Membership);
            }
            b.extend(n.to_be_bytes());
        }
    }
    b.push(nodes.len() as u8);
    for (n, node) in nodes {
        let a = node.addr.as_bytes();
        if *n == 0
            || !(1..=200).contains(&a.len())
            || !a
                .iter()
                .all(|c| c.is_ascii_alphanumeric() || b"._:[]-".contains(c))
        {
            return Err(StoreError::Membership);
        }
        b.extend(n.to_be_bytes());
        b.push(a.len() as u8);
        b.extend(a);
    }
    MembershipBlob::new(b).map_err(|_| StoreError::Membership)
}
fn decode(b: &MembershipBlob) -> Result<Members, StoreError> {
    fn inner(b: &MembershipBlob) -> Result<Members, StoreError> {
        let mut r = Reader(b.bytes());
        if r.bytes(1)?[0] != 1 {
            return Err(StoreError::Membership);
        }
        let count = r.bytes(1)?[0];
        if !(1..=2).contains(&count) {
            return Err(StoreError::Membership);
        }
        let mut configs = Vec::new();
        for _ in 0..count {
            let n = r.bytes(1)?[0];
            if !(1..=16).contains(&n) {
                return Err(StoreError::Membership);
            }
            let mut c = BTreeSet::new();
            let mut prev = 0;
            for _ in 0..n {
                let v = r.u64()?;
                if v <= prev {
                    return Err(StoreError::Membership);
                }
                prev = v;
                c.insert(v);
            }
            configs.push(c);
        }
        let n = r.bytes(1)?[0];
        if !(1..=16).contains(&n) {
            return Err(StoreError::Membership);
        }
        let mut nodes = BTreeMap::new();
        let mut prev = 0;
        for _ in 0..n {
            let v = r.u64()?;
            if v <= prev {
                return Err(StoreError::Membership);
            }
            prev = v;
            let len = r.bytes(1)?[0] as usize;
            let a = std::str::from_utf8(r.bytes(len)?).map_err(|_| StoreError::Membership)?;
            nodes.insert(v, BasicNode::new(a));
        }
        if !r.0.is_empty() || configs.iter().flatten().any(|v| !nodes.contains_key(v)) {
            return Err(StoreError::Membership);
        }
        let m = Membership::new(configs, nodes).map_err(|_| StoreError::Membership)?;
        if encode(&m)?.bytes() != b.bytes() {
            return Err(StoreError::Membership);
        }
        Ok(m)
    }
    inner(b).map_err(|_| StoreError::Membership)
}
fn to_stored(
    e: openraft::type_config::alias::EntryOf<Arx>,
) -> Result<(LogPos, StoredEntry), StoreError> {
    Ok((
        pos(e.log_id),
        match e.payload {
            EntryPayload::Blank => StoredEntry::Blank,
            EntryPayload::Normal(c) => StoredEntry::Enrollment(c),
            EntryPayload::Membership(m) => StoredEntry::Membership(encode(&m)?),
        },
    ))
}
fn from_stored(
    (p, e): (LogPos, StoredEntry),
) -> Result<openraft::type_config::alias::EntryOf<Arx>, StoreError> {
    Ok(Entry {
        log_id: id(p),
        payload: match e {
            StoredEntry::Blank => EntryPayload::Blank,
            StoredEntry::Enrollment(c) => EntryPayload::Normal(c),
            StoredEntry::Membership(m) => EntryPayload::Membership(decode(&m)?),
        },
    })
}

// Exclusive startup-only read: no adapter or engine exists yet.
fn preflight(log: &mut VoteLogStore, sm: &StateStore) -> Result<(), StoreError> {
    let state = log.log_state()?;
    let valid = if let Some(a) = sm.applied() {
        if a.index > i64::MAX as u64 {
            return Err(StoreError::Corrupt);
        }
        let end = a.index.checked_add(1).ok_or(StoreError::Corrupt)?;
        match state.purged {
            Some(p) if p.index > a.index => false,
            Some(p) if p.index == a.index => p == a,
            _ if state.last.is_none_or(|l| l.index < a.index) => state.last < Some(a),
            _ => {
                let rows = log.entries(a.index, end, 1)?;
                rows.len() == 1 && rows[0].0 == a
            }
        }
    } else {
        state.purged.is_none()
    };
    valid.then_some(()).ok_or(StoreError::Corrupt)
}
#[derive(Debug)]
pub(super) enum StartError {
    Open(StoreError),
    Preflight(StoreError),
    Fatal,
}

impl std::fmt::Display for StartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Open(_) => "consensus store open failed",
            Self::Preflight(_) => "consensus preflight failed",
            Self::Fatal => "consensus startup failed",
        })
    }
}
impl std::error::Error for StartError {}
mod adapter;

use super::{
    log_store::VoteLogStore,
    state_store::{Init, StateStore},
};
use adapter::{LogAdapter, SmAdapter, blocking};
use openraft::{
    Config, LinearizerOption, Raft, SnapshotPolicy,
    errors::{ClientWriteError, LinearizableReadError, RaftError},
    impls::ProgressResponder,
    network::{NetSnapshot, RaftNetworkFactory},
};
use std::{
    path::Path,
    sync::{Arc, Mutex, Weak},
    time::Duration,
};
use tokio::{
    sync::{Semaphore, oneshot},
    task::JoinSet,
    time::{Instant, timeout_at},
};
type Engine = Raft<Arx, SmAdapter>;
#[derive(Clone, Copy)]
pub(super) struct Params {
    pub snapshot_logs: u64,
    pub keep: u64,
    pub purge_batch: u64,
    pub lag: u64,
    pub election_min: u64,
    pub election_max: u64,
}
#[derive(Debug, PartialEq, Eq)]
pub(super) enum ProposeError {
    Busy,
    Stopped,
    NotLeader(Option<u64>),
    Unknown,
}
#[derive(Debug, PartialEq, Eq)]
pub(super) enum ReadError {
    NotLeader,
    NoQuorum,
    Unavailable,
}
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Orphaned;
#[derive(Default)]
struct Owners {
    closed: bool,
    set: JoinSet<()>,
}
pub(super) struct ConsensusNode {
    raft: Option<Engine>,
    sm: Option<SmAdapter>,
    log_weak: Weak<Mutex<VoteLogStore>>,
    sm_weak: Weak<Mutex<StateStore>>,
    pub permits: Arc<Semaphore>,
    owners: Mutex<Owners>,
}
impl ConsensusNode {
    pub async fn start<N>(
        node: u64,
        dir: &Path,
        init: Init,
        params: Params,
        network: N,
    ) -> Result<Self, StartError>
    where
        N: RaftNetworkFactory<Arx>,
        N::Network: NetSnapshot<Arx, SnapshotData = Vec<u8>>,
    {
        let dir = dir.to_owned();
        let (ls, sm) = tokio::task::spawn_blocking(move || {
            let mut ls = VoteLogStore::open(&dir, init.cluster).map_err(StartError::Open)?;
            let sm = StateStore::open(&dir, init).map_err(StartError::Open)?;
            preflight(&mut ls, &sm).map_err(StartError::Preflight)?;
            Ok((ls, sm))
        })
        .await
        .map_err(|_| StartError::Open(StoreError::Poisoned))??;
        let ls = LogAdapter(Arc::new(Mutex::new(ls)));
        let sm = SmAdapter(Arc::new(Mutex::new(sm)));
        let log_weak = Arc::downgrade(&ls.0);
        let sm_weak = Arc::downgrade(&sm.0);
        let config = Arc::new(params.config().map_err(StartError::Open)?);
        let raft = Engine::new(node, config, network, ls, sm.clone())
            .await
            .map_err(|_| StartError::Fatal)?;
        Ok(Self {
            raft: Some(raft),
            sm: Some(sm),
            log_weak,
            sm_weak,
            permits: Arc::new(Semaphore::new(64)),
            owners: Mutex::default(),
        })
    }
    pub async fn propose(&self, cmd: Command) -> Result<(LogPos, Option<Outcome>), ProposeError> {
        let permit = self
            .permits
            .clone()
            .try_acquire_owned()
            .map_err(|_| ProposeError::Busy)?;
        let (tx, rx) = oneshot::channel();
        {
            let mut owners = self.owners.lock().map_err(|_| ProposeError::Stopped)?;
            if owners.closed {
                return Err(ProposeError::Stopped);
            }
            while owners.set.try_join_next().is_some() {}
            let raft = self.raft.as_ref().ok_or(ProposeError::Stopped)?.clone();
            owners.set.spawn(async move {
                let (responder, reply) = ProgressResponder::<Arx, _>::complete_only();
                let result = match raft.client_write_ff(cmd, Some(responder)).await {
                    Err(_) => Err(ProposeError::Unknown),
                    Ok(()) => match reply.await {
                        Ok(Ok(r)) => Ok((pos(r.log_id), r.data.0)),
                        Ok(Err(ClientWriteError::ForwardToLeader(f))) => {
                            Err(ProposeError::NotLeader(f.leader_id))
                        }
                        _ => Err(ProposeError::Unknown),
                    },
                };
                drop(permit);
                let _ = tx.send(result);
            });
        }
        rx.await.unwrap_or(Err(ProposeError::Unknown))
    }
    pub async fn linearizable_state(&self) -> Result<(Option<LogPos>, Vec<u8>), ReadError> {
        let raft = self.raft.as_ref().ok_or(ReadError::Unavailable)?;
        let read = raft
            .ensure_linearizable(
                LinearizerOption::new(Some(Duration::ZERO), true)
                    .with_wait_timeout(Duration::from_millis(1500)),
            )
            .await
            .map_err(|e| match e {
                RaftError::APIError(LinearizableReadError::ForwardToLeader(_)) => {
                    ReadError::NotLeader
                }
                RaftError::APIError(LinearizableReadError::QuorumNotEnough(_)) => {
                    ReadError::NoQuorum
                }
                _ => ReadError::Unavailable,
            })?;
        let sm = self.sm.as_ref().ok_or(ReadError::Unavailable)?.clone();
        blocking(move || {
            let s = sm.0.lock().map_err(|_| StoreError::Poisoned)?;
            if s.applied().is_none_or(|p| p.index < read.index()) {
                return Err(StoreError::Corrupt);
            }
            Ok((s.applied(), s.snapshot()))
        })
        .await
        .map_err(|_| ReadError::Unavailable)
    }
    pub async fn shutdown(&mut self) -> Result<(), Orphaned> {
        let deadline = Instant::now() + Duration::from_secs(5);
        self.owners.lock().map_err(|_| Orphaned)?.closed = true;
        let mut failed = match &self.raft {
            Some(raft) => !matches!(timeout_at(deadline, raft.try_shutdown()).await, Ok(Ok(()))),
            None => false,
        };
        let mut set = {
            let mut o = self.owners.lock().map_err(|_| Orphaned)?;
            std::mem::take(&mut o.set)
        };
        if timeout_at(deadline, async {
            while let Some(r) = set.join_next().await {
                failed |= r.is_err();
            }
        })
        .await
        .is_err()
        {
            set.abort_all();
            while set.join_next().await.is_some() {}
            failed = true;
        }
        self.raft.take();
        self.sm.take();
        if timeout_at(deadline, async {
            while self.log_weak.upgrade().is_some() || self.sm_weak.upgrade().is_some() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .is_err()
        {
            failed = true;
        }
        if failed { Err(Orphaned) } else { Ok(()) }
    }
}

#[cfg(test)]
mod mem_net;
#[cfg(test)]
mod three_node;

#[cfg(test)]
fn registry_covers(m: &Members, state: &super::enrollment::EnrollmentState) -> bool {
    m.nodes().all(|(n, _)| {
        super::enrollment::NodeId::new(*n).is_some_and(|n| {
            // A key mismatch still proves that the node is allocated in the registry.
            matches!(
                state.authorize_peer(
                    super::enrollment::codec::cluster(state),
                    n,
                    super::enrollment::KeyDigest::of_spki(b"probe").unwrap(),
                ),
                Ok(()) | Err(super::enrollment::PeerRefusal::KeyMismatch)
            )
        })
    })
}
impl ConsensusNode {
    #[cfg(test)]
    async fn local_authorize(
        &self,
        cluster: super::enrollment::ClusterId,
        node: super::enrollment::NodeId,
        key: super::enrollment::KeyDigest,
    ) -> Result<(), super::enrollment::PeerRefusal> {
        let sm = self.sm.as_ref().unwrap().clone();
        blocking(move || {
            let store = sm.0.lock().unwrap();
            Ok(store.state().authorize_peer(cluster, node, key))
        })
        .await
        .unwrap()
    }
}
