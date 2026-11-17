//! Single-voter bootstrap. No CA custody or production constructor.
use super::*;
use crate::cluster::enrollment::{FoundMembers, codec};
use openraft::errors::InitializeError;
pub const FOUNDER: NodeId = match NodeId::new(1) {
    Some(n) => n,
    None => panic!("founder"),
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FoundError {
    Busy,
    Unavailable,
    NotFounder,
    AlreadyInitialized,
    FounderAllocation,
    Rng,
    BadCsr(CsrError),
    Rejected(Reject),
    WrongNode,
    StaleIssuer,
    Revoked,
    ClockRollback,
    CaExpiring,
    Sign,
}
impl std::fmt::Display for FoundError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Busy => "founder busy",
            Self::Unavailable => "founder unavailable",
            Self::NotFounder => "not founder",
            Self::AlreadyInitialized => "already initialized",
            Self::FounderAllocation => "invalid founder allocation",
            Self::Rng => "randomness refused",
            Self::BadCsr(_) => "CSR refused",
            Self::Rejected(_) => "founding rejected",
            Self::WrongNode => "wrong issuing node",
            Self::StaleIssuer => "stale issuer",
            Self::Revoked => "member revoked",
            Self::ClockRollback => "clock rollback",
            Self::CaExpiring => "CA expiring",
            Self::Sign => "signing failed",
        })
    }
}
impl std::error::Error for FoundError {}
pub fn generate_cluster_id<E>(
    fill: impl FnOnce(&mut [u8]) -> Result<(), E>,
) -> Result<ClusterId, FoundError> {
    let mut id = [0; 16];
    fill(&mut id).map_err(|_| FoundError::Rng)?;
    if id == [0; 16] {
        return Err(FoundError::Rng);
    }
    Ok(id)
}
pub fn founder_init(cluster: ClusterId) -> Init {
    Init {
        cluster,
        first_free: NodeId::new(2).unwrap(),
    }
}
struct FounderTicket {
    cluster: ClusterId,
    key: KeyDigest,
    leader: LeaderId,
    index: u64,
}
impl sealed::Sealed for FounderTicket {}
impl LeafSubject for FounderTicket {
    fn cluster(&self) -> ClusterId {
        self.cluster
    }
    fn node(&self) -> NodeId {
        FOUNDER
    }
    fn key(&self) -> KeyDigest {
        self.key
    }
}
#[derive(Clone, Copy)]
struct Pre {
    cluster: ClusterId,
    founded: bool,
    next: NodeId,
    applied: bool,
    membership: bool,
}
fn init_refusal(e: RaftError<Arx, InitializeError<Arx>>) -> FoundError {
    match e {
        RaftError::APIError(InitializeError::NotAllowed(_)) => FoundError::AlreadyInitialized,
        _ => FoundError::Unavailable,
    }
}
// Temporary F6 observation: production expansions are expression identities.
#[cfg(not(test))]
macro_rules! found_phase {
    ($deadline:expr, $phase:literal, $work:expr) => {
        $work
    };
}
#[cfg(test)]
macro_rules! found_phase {
    ($deadline:expr, $phase:literal, $work:expr) => {{
        observation::enter($deadline, $phase);
        let result = $work;
        observation::exit($deadline, $phase, &result);
        result
    }};
}
#[cfg(not(test))]
macro_rules! found_final {
    ($deadline:expr, $work:expr) => {
        $work
    };
}
#[cfg(test)]
macro_rules! found_final {
    ($deadline:expr, $work:expr) => {{
        observation::deadline_entry($deadline);
        let result = $work;
        observation::deadline_exit($deadline, &result);
        result
    }};
}
#[cfg(test)]
mod observation {
    use super::{FoundError, ISSUE_BUDGET, Instant};
    use std::cell::RefCell;

    const CAP: usize = 24;
    #[derive(Clone, Copy)]
    struct Event {
        elapsed_us: u128,
        remaining_us: u128,
        phase: &'static str,
        edge: &'static str,
        result: &'static str,
        pending: Option<&'static str>,
    }
    struct Buffer {
        events: [Option<Event>; CAP],
        len: usize,
        overflow: bool,
        pending: Option<&'static str>,
    }
    tokio::task_local! {
        static CAPTURE: RefCell<Buffer>;
    }
    fn variant<T>(r: &Result<T, FoundError>) -> &'static str {
        match r {
            Ok(_) => "Ok",
            Err(FoundError::Busy) => "Busy",
            Err(FoundError::Unavailable) => "Unavailable",
            Err(FoundError::NotFounder) => "NotFounder",
            Err(FoundError::AlreadyInitialized) => "AlreadyInitialized",
            Err(FoundError::FounderAllocation) => "FounderAllocation",
            Err(FoundError::Rng) => "Rng",
            Err(FoundError::BadCsr(_)) => "BadCsr",
            Err(FoundError::Rejected(_)) => "Rejected",
            Err(FoundError::WrongNode) => "WrongNode",
            Err(FoundError::StaleIssuer) => "StaleIssuer",
            Err(FoundError::Revoked) => "Revoked",
            Err(FoundError::ClockRollback) => "ClockRollback",
            Err(FoundError::CaExpiring) => "CaExpiring",
            Err(FoundError::Sign) => "Sign",
        }
    }
    fn record(deadline: Instant, phase: &'static str, edge: &'static str, result: &'static str) {
        let _ = CAPTURE.try_with(|cell| {
            let mut b = cell.borrow_mut();
            if phase != "finaldeadline" && edge == "enter" {
                b.pending = Some(phase);
            }
            if phase != "finaldeadline" && edge == "exit" {
                b.pending = None;
            }
            let now = Instant::now();
            let event = Event {
                elapsed_us: now
                    .saturating_duration_since(deadline - ISSUE_BUDGET)
                    .as_micros(),
                remaining_us: deadline.saturating_duration_since(now).as_micros(),
                phase,
                edge,
                result,
                pending: b.pending,
            };
            let i = b.len;
            if i < CAP {
                b.events[i] = Some(event);
                b.len += 1;
            } else {
                b.overflow = true;
            }
        });
    }
    pub(super) fn enter(d: Instant, p: &'static str) {
        record(d, p, "enter", "Pending");
    }
    pub(super) fn exit<T>(d: Instant, p: &'static str, r: &Result<T, FoundError>) {
        record(d, p, "exit", variant(r));
    }
    pub(super) fn deadline_entry(d: Instant) {
        record(d, "finaldeadline", "enter", "Pending");
    }
    pub(super) fn deadline_exit<T>(d: Instant, r: &Result<T, FoundError>) {
        record(d, "finaldeadline", "exit", variant(r));
    }
    // Only F6's first issue is scoped; no spawn, yield, sleep, global lock or node ID.
    pub(super) async fn capture<T>(work: impl std::future::Future<Output = T>) -> T {
        let buffer = RefCell::new(Buffer {
            events: [None; CAP],
            len: 0,
            overflow: false,
            pending: None,
        });
        CAPTURE
            .scope(buffer, async {
                let result = work.await;
                // found has returned: its work and issue permit have already dropped.
                // One bounded emission; never Debug-format the issuance result.
                CAPTURE.with(|cell| {
                    let b = cell.borrow();
                    let mut text =
                        format!("[DEBUG-F6-OBS] overflow={} count={}", b.overflow, b.len);
                    use std::fmt::Write;
                    for e in b.events[..b.len].iter().flatten() {
                        let _ = write!(
                            text,
                            "\nelapsed_us={} remaining_us={} phase={} edge={} result={} pending={}",
                            e.elapsed_us,
                            e.remaining_us,
                            e.phase,
                            e.edge,
                            e.result,
                            e.pending.unwrap_or("none")
                        );
                    }
                    eprintln!("{text}");
                });
                result
            })
            .await
    }
}
impl ConsensusNode {
    // Local admission snapshot only; never an authorization read.
    async fn preflight(&self) -> Result<Pre, FoundError> {
        let permit = self
            .read_permits
            .clone()
            .try_acquire_owned()
            .map_err(|_| FoundError::Busy)?;
        let (tx, rx) = oneshot::channel();
        {
            let mut owners = self.owners.lock().map_err(|_| FoundError::Unavailable)?;
            if owners.closed {
                return Err(FoundError::Unavailable);
            }
            while owners.set.try_join_next().is_some() {}
            let sm = self.sm.as_ref().ok_or(FoundError::Unavailable)?.clone();
            owners.set.spawn(async move {
                let result = blocking(move || {
                    let result = (|| {
                        let s = sm.0.lock().map_err(|_| StoreError::Poisoned)?;
                        Ok(Pre {
                            cluster: codec::cluster(s.state()),
                            founded: s.state().founded(),
                            next: s.state().next_node_id(),
                            applied: s.applied().is_some(),
                            membership: s.membership().is_some(),
                        })
                    })();
                    drop(sm);
                    drop(permit);
                    result
                })
                .await
                .map_err(|_| FoundError::Unavailable);
                let _ = tx.send(result);
            });
        }
        rx.await.unwrap_or(Err(FoundError::Unavailable))
    }
    fn committed_founder(
        &self,
        resp: openraft::raft::ClientWriteResponse<Arx>,
        cluster: ClusterId,
        key: KeyDigest,
    ) -> Result<FounderTicket, FoundError> {
        if resp.log_id.leader_id.node_id != self.id {
            return Err(FoundError::WrongNode);
        }
        match resp.data.0 {
            Some(Outcome::Founded { replay: false }) => Ok(FounderTicket {
                cluster,
                key,
                leader: resp.log_id.leader_id,
                index: resp.log_id.index,
            }),
            Some(Outcome::Founded { replay: true }) => {
                Err(FoundError::Rejected(Reject::AlreadyFounded))
            }
            Some(Outcome::Rejected(r)) => Err(FoundError::Rejected(r)),
            _ => Err(FoundError::Rejected(Reject::UnknownCode)),
        }
    }
    async fn founder_guard(&self, ticket: &FounderTicket) -> Result<(), FoundError> {
        let (cluster, key) = (ticket.cluster, ticket.key);
        let (leader, result) = self
            .fresh_read(move |s| Ok(s.state().authorize_peer(cluster, FOUNDER, key)))
            .await
            .map_err(|e| match e {
                ReadError::Busy => FoundError::Busy,
                _ => FoundError::Unavailable,
            })?;
        if leader != ticket.leader {
            return Err(FoundError::StaleIssuer);
        }
        result.map_err(|e| match e {
            PeerRefusal::WrongCluster => FoundError::Rejected(Reject::WrongCluster),
            _ => FoundError::Revoked,
        })
    }
    pub async fn found(
        &self,
        ca: &ClusterCa,
        clock: &dyn SignerClock,
        addr: BasicNode,
        csr: &[u8],
    ) -> Result<IssuedLeaf, FoundError> {
        let deadline = Instant::now() + ISSUE_BUDGET;
        let _permit = self
            .issue_permits
            .try_acquire()
            .map_err(|_| FoundError::Busy)?;
        let work = async {
            if self.id != FOUNDER.get() {
                return Err(FoundError::NotFounder);
            }
            let pre = found_phase!(deadline, "preflight", self.preflight().await)?;
            if pre.founded || pre.applied || pre.membership {
                return Err(FoundError::AlreadyInitialized);
            }
            if pre.next != NodeId::new(2).unwrap() {
                return Err(FoundError::FounderAllocation);
            }
            let csr = ProvenCsr::parse(csr).map_err(FoundError::BadCsr)?;
            if !ca.leaf_fits(clock.now_ms()) {
                return Err(FoundError::CaExpiring);
            }
            let raft = self.raft.as_ref().ok_or(FoundError::Unavailable)?;
            found_phase!(
                deadline,
                "initialize",
                raft.initialize(BTreeMap::from([(1, addr)]))
                    .await
                    .map_err(init_refusal)
            )?;
            found_phase!(
                deadline,
                "leaderwait",
                raft.wait(Some(deadline.saturating_duration_since(Instant::now())))
                    .metrics(
                        |m| m.current_leader == Some(1) && m.state == openraft::ServerState::Leader,
                        "founder leader",
                    )
                    .await
                    .map_err(|_| FoundError::Unavailable)
            )?;
            let t0 = clock.now_ms();
            let resp = found_phase!(
                deadline,
                "proposal",
                self.propose_response(Command::Found {
                    cluster: pre.cluster,
                    members: FoundMembers::new([(FOUNDER, csr.digest())])
                        .map_err(|_| FoundError::Unavailable)?,
                    proposed_at_ms: t0,
                })
                .await
                .map_err(|e| match e {
                    ProposeError::Busy => FoundError::Busy,
                    _ => FoundError::Unavailable,
                })
            )?;
            let ticket = self.committed_founder(resp, pre.cluster, csr.digest())?;
            found_phase!(deadline, "guard1", self.founder_guard(&ticket).await)?;
            let pre_now = clock.now_ms();
            if pre_now < t0 {
                return Err(FoundError::ClockRollback);
            }
            let der = ca.sign_leaf(&ticket, &csr, pre_now).map_err(|e| match e {
                IssueError::CaExpiring => FoundError::CaExpiring,
                _ => FoundError::Sign,
            })?;
            found_phase!(deadline, "guard2", self.founder_guard(&ticket).await)?;
            let post_now = clock.now_ms();
            if post_now < pre_now {
                return Err(FoundError::ClockRollback);
            }
            Ok(IssuedLeaf {
                der,
                node: FOUNDER,
                identity_commit: LogPos {
                    term: ticket.leader.term,
                    leader: ticket.leader.node_id,
                    index: ticket.index,
                },
            })
        };
        found_final!(
            deadline,
            match timeout_at(deadline, work).await {
                Ok(r) if Instant::now() < deadline => r,
                _ => Err(FoundError::Unavailable),
            }
        )
    }
}
#[cfg(test)]
#[path = "found_tests.rs"]
mod found_tests;
