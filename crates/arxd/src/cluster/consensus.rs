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
type LeaderId = openraft::impls::leader_id_adv::LeaderId<u64, u64>;
const READ_BUDGET: Duration = Duration::from_millis(1500);
const READ_PERMITS: usize = 8;
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
    Busy,
    NotLeader,
    NoQuorum,
    Unavailable,
}
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Orphaned(pub ShutdownReport);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EngineEnd {
    Absent,
    Joined,
    JoinFailed,
    Deadline,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Phase {
    Lock,
    Engine,
    Owners,
    StoreRelease,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ShutdownReport {
    pub first: Phase,
    pub engine: EngineEnd,
    pub engine_ms: u16,
    pub owners_done_at_start: u8,
    pub owners_waited: u8,
    pub owners_left: u8,
    pub owners_panicked: u8,
    pub last_owner_join_ms: u16,
    pub propose_held: u8,
    // Captured before abort, alongside proposal units.
    pub read_held: u8,
    pub log_strong: u8,
    pub sm_strong: u8,
    pub total_ms: u16,
}
impl ShutdownReport {
    fn fail(&mut self, failed: &mut bool, phase: Phase) {
        if !*failed {
            self.first = phase;
        }
        *failed = true;
    }
}
#[derive(Default)]
struct Owners {
    closed: bool,
    set: JoinSet<()>,
}
pub(super) struct ConsensusNode {
    id: u64,
    read_permits: Arc<Semaphore>,
    #[cfg(test)]
    read_blocking: Arc<std::sync::atomic::AtomicUsize>,
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
            id: node,
            read_permits: Arc::new(Semaphore::new(READ_PERMITS)),
            #[cfg(test)]
            read_blocking: Arc::default(),
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
    /// `f` is a pure store read: no await and no calls back into Raft.
    /// The caller deadline does not terminate queued engine or blocking work.
    async fn fresh_read<T: Send + 'static>(
        &self,
        f: impl FnOnce(&StateStore) -> Result<T, StoreError> + Send + 'static,
    ) -> Result<(LeaderId, T), ReadError> {
        let deadline = Instant::now() + READ_BUDGET;
        let permit = self
            .read_permits
            .clone()
            .try_acquire_owned()
            .map_err(|_| ReadError::Busy)?;
        let (tx, rx) = oneshot::channel();
        {
            let mut owners = self.owners.lock().map_err(|_| ReadError::Unavailable)?;
            if owners.closed {
                return Err(ReadError::Unavailable);
            }
            while owners.set.try_join_next().is_some() {}
            let raft = self.raft.as_ref().ok_or(ReadError::Unavailable)?.clone();
            let sm = self.sm.as_ref().ok_or(ReadError::Unavailable)?.clone();
            #[cfg(test)]
            let read_blocking = self.read_blocking.clone();
            owners.set.spawn(async move {
                let result = async {
                    if Instant::now() >= deadline || tx.is_closed() {
                        return Err(ReadError::Unavailable);
                    }
                    // Never cancel this wait: only its reply proves that an
                    // unwithdrawable engine request no longer owns this unit.
                    let linearizer = raft
                        .get_read_linearizer(
                            LinearizerOption::new(Some(Duration::ZERO), true).with_wait_timeout(
                                deadline.saturating_duration_since(Instant::now()),
                            ),
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
                    if Instant::now() >= deadline || tx.is_closed() {
                        return Err(ReadError::Unavailable);
                    }
                    let ready = match linearizer
                        .try_await_ready(
                            &raft,
                            Some(deadline.saturating_duration_since(Instant::now())),
                        )
                        .await
                    {
                        Ok(Ok(ready)) => ready,
                        _ => return Err(ReadError::Unavailable),
                    };
                    if Instant::now() >= deadline || tx.is_closed() {
                        return Err(ReadError::Unavailable);
                    }
                    let read = ready.read_log_id();
                    let leader = *read.committed_leader_id();
                    let index = read.index();
                    // Cancellation of the async owner cannot release either
                    // resource while this non-abortable closure still runs.
                    blocking(move || {
                        #[cfg(test)]
                        read_blocking.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        let result = (|| {
                            let s = sm.0.lock().map_err(|_| StoreError::Poisoned)?;
                            if s.applied().is_none_or(|p| p.index < index) {
                                Err(StoreError::Corrupt)
                            } else {
                                f(&s).map(|value| (leader, value))
                            }
                        })();
                        // Guard, store Arc, permit, then completion (#147).
                        drop(sm);
                        drop(permit);
                        result
                    })
                    .await
                    .map_err(|_| ReadError::Unavailable)
                }
                .await;
                if Instant::now() < deadline && !tx.is_closed() {
                    let _ = tx.send(result);
                }
            });
        }
        match timeout_at(deadline, rx).await {
            Ok(Ok(result)) if Instant::now() < deadline => result,
            _ => Err(ReadError::Unavailable),
        }
    }
    pub async fn linearizable_state(&self) -> Result<(Option<LogPos>, Vec<u8>), ReadError> {
        self.fresh_read(|s| Ok((s.applied(), s.snapshot())))
            .await
            .map(|(_, state)| state)
    }
    pub async fn shutdown(&mut self) -> Result<(), Orphaned> {
        let start = Instant::now();
        let deadline = start + Duration::from_secs(5);
        let elapsed = || start.elapsed().as_millis().min(u16::MAX as u128) as u16;
        let count = |n: usize| n.min(64 + READ_PERMITS) as u8;
        let mut report = ShutdownReport {
            first: Phase::Lock,
            engine: EngineEnd::Absent,
            engine_ms: 0,
            owners_done_at_start: 0,
            owners_waited: 0,
            owners_left: 0,
            owners_panicked: 0,
            last_owner_join_ms: 0,
            propose_held: 0,
            read_held: 0,
            log_strong: 0,
            sm_strong: 0,
            total_ms: 0,
        };
        let mut failed = false;
        self.owners.lock().map_err(|_| Orphaned(report))?.closed = true;
        report.engine = match &self.raft {
            Some(raft) => match timeout_at(deadline, raft.try_shutdown()).await {
                Ok(Ok(())) => EngineEnd::Joined,
                Ok(Err(_)) => EngineEnd::JoinFailed,
                Err(_) => EngineEnd::Deadline,
            },
            None => EngineEnd::Absent,
        };
        report.engine_ms = elapsed();
        if matches!(report.engine, EngineEnd::JoinFailed | EngineEnd::Deadline) {
            report.fail(&mut failed, Phase::Engine);
        }
        let mut set = {
            let mut o = self.owners.lock().map_err(|_| {
                report.fail(&mut failed, Phase::Lock);
                report.total_ms = elapsed();
                Orphaned(report)
            })?;
            std::mem::take(&mut o.set)
        };
        while let Some(r) = set.try_join_next() {
            report.owners_done_at_start = count(report.owners_done_at_start as usize + 1);
            report.last_owner_join_ms = elapsed();
            if r.is_err() {
                report.owners_panicked = count(report.owners_panicked as usize + 1);
                report.fail(&mut failed, Phase::Owners);
            }
        }
        report.owners_waited = count(set.len());
        if timeout_at(deadline, async {
            while let Some(r) = set.join_next().await {
                report.last_owner_join_ms = elapsed();
                if r.is_err() {
                    report.owners_panicked = count(report.owners_panicked as usize + 1);
                    report.fail(&mut failed, Phase::Owners);
                }
            }
        })
        .await
        .is_err()
        {
            report.fail(&mut failed, Phase::Owners);
            // Capture owners and held permits BEFORE abort/join releases them.
            report.owners_left = count(set.len());
            report.propose_held = count(64usize.saturating_sub(self.permits.available_permits()));
            report.read_held =
                count(READ_PERMITS.saturating_sub(self.read_permits.available_permits()));
            set.abort_all();
            while set.join_next().await.is_some() {}
        } else {
            report.propose_held = count(64usize.saturating_sub(self.permits.available_permits()));
            report.read_held =
                count(READ_PERMITS.saturating_sub(self.read_permits.available_permits()));
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
            report.fail(&mut failed, Phase::StoreRelease);
        }
        report.log_strong = count(self.log_weak.strong_count());
        report.sm_strong = count(self.sm_weak.strong_count());
        report.total_ms = elapsed();
        if failed {
            Err(Orphaned(report))
        } else {
            Ok(())
        }
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
