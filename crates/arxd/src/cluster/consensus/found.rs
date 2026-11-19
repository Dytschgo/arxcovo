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
    #[cfg(test)]
    if observation::active() {
        observation::phase_reply(
            10,
            match &e {
                RaftError::APIError(InitializeError::NotAllowed(_)) => {
                    observation::Reason::InitNotAllowed
                }
                RaftError::APIError(_) => observation::Reason::ApiOther,
                RaftError::Fatal(_) => observation::Reason::Fatal,
            },
        );
    }
    match e {
        RaftError::APIError(InitializeError::NotAllowed(_)) => FoundError::AlreadyInitialized,
        _ => FoundError::Unavailable,
    }
}
// Temporary F6 observation; cfg(test) statements add only bounded scalar events.
macro_rules! found_phase {
    ($deadline:expr, $phase:literal, $work:expr) => {{
        #[cfg(test)]
        observation::enter($deadline, $phase);
        let result = $work;
        #[cfg(test)]
        observation::exit($deadline, $phase, &result);
        result
    }};
}
macro_rules! found_final {
    ($deadline:expr, $work:expr) => {{
        #[cfg(test)]
        observation::deadline_entry($deadline);
        let result = $work;
        #[cfg(test)]
        observation::deadline_exit($deadline, &result);
        result
    }};
}
#[cfg(test)]
pub(in crate::cluster::consensus) mod observation {
    use super::{FoundError, ISSUE_BUDGET, Instant};
    use openraft::type_config::async_runtime::watch::WatchReceiver;
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
        slots: Option<std::sync::Arc<Slots>>,
        decision: Decision,
        reason: Reason,
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
        if active() && p == "initialize" && r.is_ok() {
            phase_reply(10, Reason::Ok);
        }
        if active() && p == "leaderwait" && r.is_ok() {
            phase_reply(11, Reason::Ok);
        }
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
            slots: None,
            decision: Decision::Unknown,
            reason: Reason::Unknown,
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
    use std::fmt::Write;
    use std::sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    };
    const MAX_US: u128 = (1u128 << 48) - 2;
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(in crate::cluster::consensus) enum Call {
        QueryInitial,
        ProofInitial,
        F10Final,
    }
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(in crate::cluster::consensus) enum Reason {
        Pending,
        Ok,
        Busy,
        OwnersPoison,
        OwnersClosed,
        MissingRaft,
        MissingStore,
        MutexPoison,
        StoreCorrupt,
        StoreOther,
        BlockingError,
        ReceiverClosed,
        SenderClosed,
        Expired,
        InitNotAllowed,
        ApiOther,
        Fatal,
        Forward,
        NoQuorum,
        ReadyInnerError,
        ReadyOuterError,
        SubmitError,
        ReplyError,
        WaitTimeout,
        ShuttingDown,
        Unknown,
    }
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(in crate::cluster::consensus) enum Decision {
        Unknown,
        InnerOnTime,
        ReadyLate,
        WaitElapsed,
    }
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Lane {
        Preflight,
        Proposal,
        Guard1,
        Guard2,
    }
    #[derive(Clone, Copy)]
    pub(in crate::cluster::consensus) enum Edge {
        Admission,
        OwnersGate,
        OwnerEnter,
        BlockingSubmit,
        ClosureStart,
        MutexAcquired,
        StoreReply,
        BlockingJoin,
        OwnerSend,
        CallerReply,
        Submission,
        SubmissionReply,
        EngineReply,
        LinearizerEnter,
        LinearizerReply,
        AppliedReady,
        CallerExpiry,
    }
    impl Lane {
        fn slot(self, edge: Edge) -> usize {
            use Edge::*;
            match self {
                Self::Preflight => match edge {
                    Admission => 0,
                    OwnersGate => 1,
                    OwnerEnter => 2,
                    BlockingSubmit => 3,
                    ClosureStart => 4,
                    MutexAcquired => 5,
                    StoreReply => 6,
                    BlockingJoin => 7,
                    OwnerSend => 8,
                    CallerReply => 9,
                    _ => unreachable!(),
                },
                Self::Proposal => match edge {
                    Admission => 12,
                    OwnersGate => 13,
                    OwnerEnter => 14,
                    Submission => 15,
                    SubmissionReply => 16,
                    EngineReply => 17,
                    OwnerSend => 18,
                    CallerReply => 19,
                    _ => unreachable!(),
                },
                Self::Guard1 | Self::Guard2 => {
                    let relative = match edge {
                        Admission => 0,
                        OwnersGate => 1,
                        OwnerEnter => 2,
                        LinearizerEnter => 3,
                        LinearizerReply => 4,
                        AppliedReady => 5,
                        BlockingSubmit => 6,
                        ClosureStart => 7,
                        MutexAcquired => 8,
                        StoreReply => 9,
                        BlockingJoin => 10,
                        OwnerSend => 11,
                        CallerReply => 12,
                        CallerExpiry => 13,
                        _ => unreachable!(),
                    };
                    (if self == Self::Guard1 { 20 } else { 34 }) + relative
                }
            }
        }
    }
    struct Slots {
        words: [AtomicU64; 48],
        trace: [AtomicU64; 20],
        start: Instant,
    }
    impl Slots {
        fn new() -> Self {
            Self {
                words: std::array::from_fn(|_| AtomicU64::new(0)),
                trace: std::array::from_fn(|_| AtomicU64::new(0)),
                start: Instant::now(),
            }
        }
        fn freeze(&self) -> [u64; 48] {
            std::array::from_fn(|i| self.words[i].load(Ordering::Acquire))
        }
    }
    #[derive(Clone)]
    pub(in crate::cluster::consensus) struct Handle {
        slots: Arc<Slots>,
        lane: Lane,
    }
    pub(in crate::cluster::consensus) fn worker() -> Option<Handle> {
        CAPTURE
            .try_with(|cell| {
                let b = cell.borrow();
                let lane = match b.pending? {
                    "preflight" => Lane::Preflight,
                    "proposal" => Lane::Proposal,
                    "guard1" => Lane::Guard1,
                    "guard2" => Lane::Guard2,
                    _ => return None,
                };
                Some(Handle {
                    slots: b.slots.clone()?,
                    lane,
                })
            })
            .ok()
            .flatten()
    }
    // Bounded weak registry: fixture identity is never emitted or retained as ownership.
    type Binding = (usize, usize, std::sync::Weak<Slots>);
    static BINDINGS: std::sync::Mutex<Vec<Binding>> = std::sync::Mutex::new(Vec::new());
    pub(super) fn bind(node: &super::ConsensusNode, deadline: Instant) {
        let _ = CAPTURE.try_with(|cell| {
            if let Some(slots) = &cell.borrow().slots {
                slots.trace[0].store(
                    pack(
                        deadline.duration_since(slots.start).as_micros(),
                        Reason::Pending,
                    ),
                    Ordering::Release,
                );
                register(
                    &BINDINGS,
                    node.log_weak.as_ptr() as usize,
                    node.sm_weak.as_ptr() as usize,
                    slots,
                );
            }
        });
    }
    fn register(
        registry: &std::sync::Mutex<Vec<Binding>>,
        log: usize,
        sm: usize,
        slots: &Arc<Slots>,
    ) {
        // Contention and poison omit evidence; neither is permission to wait.
        let Ok(mut bindings) = registry.try_lock() else {
            return;
        };
        bindings.retain(|(old_log, old_sm, w)| {
            w.strong_count() != 0 && *old_log != log && *old_sm != sm
        });
        if bindings.len() < 64 {
            bindings.push((log, sm, Arc::downgrade(slots)));
        }
    }
    pub(in crate::cluster::consensus) fn entry(store: usize, p: super::LogPos, stage: usize) {
        // Sample the actual boundary before any recorder lookup.
        let at = Instant::now();
        entry_at(&BINDINGS, at, store, p, stage);
    }
    fn entry_at(
        registry: &std::sync::Mutex<Vec<Binding>>,
        at: Instant,
        store: usize,
        p: super::LogPos,
        stage: usize,
    ) {
        let slots = registry.try_lock().ok().and_then(|bindings| {
            bindings
                .iter()
                .find(|(log, sm, _)| *log == store || *sm == store)
                .and_then(|(_, _, w)| w.upgrade())
        });
        if let Some(slots) = slots {
            let base = if stage == 4 { 1 } else { 17 };
            for (i, v) in [p.term, p.leader, p.index].into_iter().enumerate() {
                let _ = slots.trace[base + i].compare_exchange(
                    0,
                    v.saturating_add(1),
                    Ordering::Release,
                    Ordering::Relaxed,
                );
            }
            let _ = slots.trace[stage].compare_exchange(
                0,
                pack(
                    at.saturating_duration_since(slots.start).as_micros(),
                    Reason::Ok,
                ),
                Ordering::Release,
                Ordering::Relaxed,
            );
        }
    }
    pub(in crate::cluster::consensus) fn sent(h: &Option<Handle>, ok: bool) {
        if let Some(h) = h {
            h.slots.trace[8].store(
                pack(
                    h.slots.start.elapsed().as_micros(),
                    if ok { Reason::Ok } else { Reason::SenderClosed },
                ),
                Ordering::Release,
            );
        }
    }
    pub(super) fn metrics(node: &super::ConsensusNode, base: usize) {
        let _ = CAPTURE.try_with(|cell| {
            if let (Some(slots), Some(raft)) = (&cell.borrow().slots, &node.raft) {
                let rx = raft.metrics();
                // Published borrow is a synchronous read lock, not a strict try-read.
                let scalars = {
                    let m = rx.borrow_watched();
                    [
                        m.last_log_index,
                        m.local_committed.map(|p| p.index),
                        m.last_applied.map(|p| p.index),
                    ]
                };
                for (i, v) in scalars.into_iter().enumerate() {
                    slots.trace[base + 1 + i]
                        .store(v.map_or(0, |v| v.saturating_add(1)), Ordering::Release);
                }
                slots.trace[base].store(
                    pack(slots.start.elapsed().as_micros(), Reason::Ok),
                    Ordering::Release,
                );
            }
        });
    }
    fn pack(us: u128, reason: Reason) -> u64 {
        (((us > MAX_US) as u64) << 56) | (((us.min(MAX_US) as u64) + 1) << 8) | reason as u64
    }
    pub(in crate::cluster::consensus) fn mark(h: &Option<Handle>, edge: Edge, reason: Reason) {
        if let Some(h) = h {
            h.slots.words[h.lane.slot(edge)].store(
                pack(h.slots.start.elapsed().as_micros(), reason),
                Ordering::Release,
            );
        }
    }
    pub(in crate::cluster::consensus) fn caller(reason: Reason) {
        let _ = CAPTURE.try_with(|cell| {
            let mut buffer = cell.borrow_mut();
            if buffer.slots.is_some() {
                buffer.reason = reason;
            }
        });
    }
    pub(in crate::cluster::consensus) fn decision(tag: Decision) {
        let _ = CAPTURE.try_with(|cell| {
            let mut buffer = cell.borrow_mut();
            if buffer.slots.is_some() {
                buffer.decision = tag;
            }
        });
    }
    pub(super) fn active() -> bool {
        CAPTURE
            .try_with(|cell| cell.borrow().slots.is_some())
            .unwrap_or(false)
    }
    pub(super) fn phase_reply(index: usize, reason: Reason) {
        let _ = CAPTURE.try_with(|cell| {
            if let Some(slots) = &cell.borrow().slots {
                slots.words[index].store(
                    pack(slots.start.elapsed().as_micros(), reason),
                    Ordering::Release,
                );
            }
        });
    }
    pub(super) fn leader_wait_reason(error: &openraft::metrics::WaitError) -> Reason {
        match error {
            openraft::metrics::WaitError::Timeout(_, _) => Reason::WaitTimeout,
            openraft::metrics::WaitError::ShuttingDown => Reason::ShuttingDown,
        }
    }
    pub(in crate::cluster::consensus) fn store_reason<T>(
        r: &Result<T, super::StoreError>,
    ) -> Reason {
        match r {
            Ok(_) => Reason::Ok,
            Err(super::StoreError::Poisoned) => Reason::MutexPoison,
            Err(super::StoreError::Corrupt) => Reason::StoreCorrupt,
            Err(_) => Reason::StoreOther,
        }
    }
    #[derive(Clone, Copy)]
    pub(in crate::cluster::consensus) struct Marker {
        wall_ms: Option<u64>,
        elapsed_us: u64,
    }
    fn marker(start: Instant) -> Marker {
        Marker {
            wall_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .ok()
                .and_then(|d| u64::try_from(d.as_millis()).ok()),
            elapsed_us: start.elapsed().as_micros().min(u64::MAX as u128) as u64,
        }
    }
    pub(in crate::cluster::consensus) struct Snapshot {
        call: Call,
        events: [Option<Event>; CAP],
        len: usize,
        overflow: bool,
        words: [u64; 48],
        trace: [u64; 20],
        decision: Decision,
        reason: Reason,
        result: &'static str,
        start: Marker,
        end: Marker,
        // Only ProofInitial Err retains this scalar recorder; never an engine/store handle.
        postshutdown: Option<Arc<Slots>>,
    }
    // Sequential Acquire loads at capture return, before cleanup; no atomic worker snapshot.
    pub(in crate::cluster::consensus) async fn capture_call<T>(
        call: Call,
        work: impl std::future::Future<Output = Result<T, FoundError>>,
    ) -> (Result<T, FoundError>, Snapshot) {
        let slots = Arc::new(Slots::new());
        let start = marker(slots.start);
        let buffer = RefCell::new(Buffer {
            events: [None; CAP],
            len: 0,
            overflow: false,
            pending: None,
            slots: Some(slots.clone()),
            decision: Decision::Unknown,
            reason: Reason::Unknown,
        });
        CAPTURE
            .scope(buffer, async {
                let result = work.await;
                let snapshot = CAPTURE.with(|cell| {
                    let b = cell.borrow();
                    Snapshot {
                        call,
                        events: b.events,
                        len: b.len,
                        overflow: b.overflow,
                        words: slots.freeze(),
                        trace: std::array::from_fn(|i| slots.trace[i].load(Ordering::Acquire)),
                        decision: b.decision,
                        reason: b.reason,
                        result: variant(&result),
                        start,
                        end: marker(slots.start),
                        postshutdown: (call == Call::ProofInitial && result.is_err())
                            .then(|| slots.clone()),
                    }
                });
                (result, snapshot)
            })
            .await
    }
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(in crate::cluster::consensus) enum Status {
        Unknown,
        Ok,
        Failed,
        Timeout,
    }
    #[derive(Clone, Copy, Debug)]
    pub(in crate::cluster::consensus) struct Cleanup {
        pub(in crate::cluster::consensus) router: Status,
        pub(in crate::cluster::consensus) shutdown: Status,
        pub(in crate::cluster::consensus) store: Status,
        pub(in crate::cluster::consensus) log: Status,
    }
    impl Cleanup {
        pub(in crate::cluster::consensus) fn unknown() -> Self {
            Self {
                router: Status::Unknown,
                shutdown: Status::Unknown,
                store: Status::Unknown,
                log: Status::Unknown,
            }
        }
    }
    pub(in crate::cluster::consensus) fn status(ok: bool) -> Status {
        if ok { Status::Ok } else { Status::Failed }
    }
    pub(in crate::cluster::consensus) fn shutdown_status<T, E>(
        r: &Result<Result<T, E>, tokio::time::error::Elapsed>,
    ) -> Status {
        match r {
            Ok(Ok(_)) => Status::Ok,
            Ok(Err(_)) => Status::Failed,
            Err(_) => Status::Timeout,
        }
    }
    struct Sink<const N: usize> {
        bytes: [u8; N],
        len: usize,
        truncated: bool,
    }
    impl<const N: usize> Sink<N> {
        fn new() -> Self {
            Self {
                bytes: [0; N],
                len: 0,
                truncated: false,
            }
        }
        fn finish(&mut self) -> &str {
            let footer = if self.truncated {
                " truncated=1\n"
            } else {
                "\n"
            };
            self.bytes[self.len..self.len + footer.len()].copy_from_slice(footer.as_bytes());
            self.len += footer.len();
            std::str::from_utf8(&self.bytes[..self.len]).expect("ASCII sink")
        }
    }
    impl<const N: usize> std::fmt::Write for Sink<N> {
        fn write_str(&mut self, s: &str) -> std::fmt::Result {
            if self.truncated || !s.is_ascii() || self.len + s.len() > N.saturating_sub(13) {
                self.truncated = true;
                return Err(std::fmt::Error);
            }
            self.bytes[self.len..self.len + s.len()].copy_from_slice(s.as_bytes());
            self.len += s.len();
            Ok(())
        }
    }
    fn format_snapshot(s: &Snapshot, c: Cleanup, out: &mut Sink<8192>) {
        let _ = write!(
            out,
            "[FOUNDER-OBS] primary={:?} result={} reason={:?} call={:?} cutoff=capture_return sequential=1 overlap=Unknown cleanup_router={:?} cleanup_shutdown={:?} cleanup_store={:?} cleanup_log={:?}",
            s.decision, s.result, s.reason, s.call, c.router, c.shutdown, c.store, c.log
        );
        let _ = write!(
            out,
            " start_wall_ms={:?} end_wall_ms={:?} start_elapsed_us={} end_elapsed_us={} overflow={} count={}",
            s.start.wall_ms, s.end.wall_ms, s.start.elapsed_us, s.end.elapsed_us, s.overflow, s.len
        );
        // Time words use pack encoding; positions are value+1 (0 absent).
        let _ = write!(
            out,
            "\ntrace_schema=deadline,append_term,append_leader,append_index,append_ok,apply_arrival,store_ok,responder_boundary,owner_send,entry_metrics_time,last_log,local_committed,last_applied,decision_metrics_time,last_log,local_committed,last_applied,apply_term,apply_leader,apply_index"
        );
        for (i, w) in s.trace.iter().enumerate() {
            let _ = write!(out, "\ntrace={} scalar={}", i, w);
        }
        for (i, w) in s.words.iter().enumerate() {
            let _ = write!(
                out,
                "\nslot={} packed={} absent={} saturated={}",
                i,
                w,
                *w == 0,
                w >> 56 != 0
            );
        }
        for e in s.events[..s.len].iter().flatten() {
            let _ = write!(
                out,
                "\nelapsed_us={} remaining_us={} phase={} edge={} result={} pending={}",
                e.elapsed_us.min(u64::MAX as u128),
                e.remaining_us.min(u64::MAX as u128),
                e.phase,
                e.edge,
                e.result,
                e.pending.unwrap_or("none")
            );
        }
    }
    pub(in crate::cluster::consensus) fn emit(s: &Snapshot, c: Cleanup) {
        let mut out = Sink::<8192>::new();
        format_snapshot(s, c, &mut out);
        eprint!("{}", out.finish());
    }
    #[derive(Clone, Copy)]
    struct Postshutdown {
        words: [u64; 2],
    }
    // One sequential pair of Acquire loads, no clock sample or full freeze.
    fn postshutdown_cut(slots: &Slots) -> Postshutdown {
        Postshutdown {
            words: [
                slots.words[17].load(Ordering::Acquire),
                slots.words[18].load(Ordering::Acquire),
            ],
        }
    }
    fn format_postshutdown(cut: Option<Postshutdown>, c: Cleanup, out: &mut Sink<8192>) {
        let eligible = c.router == Status::Ok
            && c.shutdown == Status::Ok
            && c.store == Status::Ok
            && c.log == Status::Ok;
        let _ = write!(
            out,
            "\n[FOUNDER-POSTSHUTDOWN] cutoff=aftershutdown sequential=1 cleanup_eligible={} interpretation=Unknown present={}",
            eligible,
            cut.is_some()
        );
        if let Some(cut) = cut {
            for (i, w) in cut.words.iter().enumerate() {
                let _ = write!(
                    out,
                    "\nslot={} packed={} absent={} saturated={}",
                    17 + i,
                    w,
                    *w == 0,
                    w >> 56 != 0
                );
            }
        }
    }
    // Sole live caller: ProofInitial refusal AFTER existing shutdown/weak checks.
    // Keep the original cut and primary error even when cleanup is not Ok.
    pub(in crate::cluster::consensus) fn emit_proof_refusal(s: &Snapshot, c: Cleanup) {
        let cut = s.postshutdown.as_ref().map(|slots| postshutdown_cut(slots));
        let mut out = Sink::<8192>::new();
        format_snapshot(s, c, &mut out);
        format_postshutdown(cut, c, &mut out);
        eprint!("{}", out.finish());
    }
    pub(in crate::cluster::consensus) fn fourth_start() -> (Instant, Marker) {
        let start = Instant::now();
        (start, marker(start))
    }
    pub(in crate::cluster::consensus) fn fourth_end(pair: (Instant, Marker)) {
        let end = marker(pair.0);
        let mut out = Sink::<512>::new();
        let _ = write!(
            out,
            "[FOUNDER-CO-RUNNER] start_wall_ms={:?} end_wall_ms={:?} start_elapsed_us={} end_elapsed_us={} overlap=Unknown",
            pair.1.wall_ms, end.wall_ms, pair.1.elapsed_us, end.elapsed_us
        );
        eprint!("{}", out.finish());
    }

    #[cfg(test)]
    mod observer_tests {
        use super::*;
        #[test]
        fn postshutdown_pair_preserves_return_cut_and_scalar_shape() {
            // Exhaustive destructuring fails compilation if hidden fields are added.
            let Slots {
                words,
                trace,
                start,
            }: Slots = Slots::new();
            let _: [AtomicU64; 48] = words;
            let _: [AtomicU64; 20] = trace;
            let _: Instant = start;
            let slots = Slots::new();
            slots.words[16].store(pack(1, Reason::Ok), Ordering::Release);
            let returned = slots.freeze();
            slots.words[17].store(pack(2, Reason::Ok), Ordering::Release);
            slots.words[18].store(pack(3, Reason::Pending), Ordering::Release);
            slots.words[19].store(u64::MAX, Ordering::Release);
            let after = postshutdown_cut(&slots);
            assert_eq!(returned[17..19], [0, 0]);
            assert_eq!(after.words, [pack(2, Reason::Ok), pack(3, Reason::Pending)]);
            assert_eq!(returned[16], pack(1, Reason::Ok));
        }
        #[tokio::test]
        async fn postshutdown_retention_only_proof_refusal() {
            for call in [Call::QueryInitial, Call::ProofInitial, Call::F10Final] {
                for refused in [false, true] {
                    let mut weak = None;
                    let (_, snapshot) = capture_call(call, async {
                        CAPTURE.with(|cell| {
                            weak = Some(Arc::downgrade(cell.borrow().slots.as_ref().unwrap()));
                        });
                        if refused {
                            Err(FoundError::Unavailable)
                        } else {
                            Ok(())
                        }
                    })
                    .await;
                    let retained = call == Call::ProofInitial && refused;
                    assert_eq!(snapshot.postshutdown.is_some(), retained);
                    let weak = weak.unwrap();
                    assert_eq!(weak.upgrade().is_some(), retained);
                    drop(snapshot);
                    assert!(weak.upgrade().is_none());
                }
            }
        }
        #[tokio::test]
        async fn postshutdown_formatter_two_cuts_cleanup_unknown_and_total_cap() {
            let (_, snapshot) = capture_call(Call::ProofInitial, async {
                decision(Decision::WaitElapsed);
                Err::<(), _>(FoundError::Unavailable)
            })
            .await;
            let before = snapshot.words;
            let slots = snapshot.postshutdown.as_ref().unwrap();
            slots.words[17].store(pack(2, Reason::Ok), Ordering::Release);
            slots.words[18].store(pack(3, Reason::Pending), Ordering::Release);
            for field in 0..4 {
                for state in [Status::Ok, Status::Failed, Status::Timeout, Status::Unknown] {
                    let mut cleanup = Cleanup {
                        router: Status::Ok,
                        shutdown: Status::Ok,
                        store: Status::Ok,
                        log: Status::Ok,
                    };
                    match field {
                        0 => cleanup.router = state,
                        1 => cleanup.shutdown = state,
                        2 => cleanup.store = state,
                        3 => cleanup.log = state,
                        _ => unreachable!(),
                    }
                    let mut out = Sink::<8192>::new();
                    format_snapshot(&snapshot, cleanup, &mut out);
                    format_postshutdown(Some(postshutdown_cut(slots)), cleanup, &mut out);
                    let text = out.finish();
                    assert!(
                        text.starts_with("[FOUNDER-OBS] primary=WaitElapsed result=Unavailable")
                    );
                    let (original, after) = text.split_once("[FOUNDER-POSTSHUTDOWN]").unwrap();
                    assert!(original.contains("slot=17 packed=0 absent=true"));
                    assert!(original.contains("slot=18 packed=0 absent=true"));
                    assert!(after.contains("cutoff=aftershutdown sequential=1"));
                    assert!(after.contains("slot=17 packed=769 absent=false saturated=false"));
                    assert!(after.contains("slot=18 packed=1024 absent=false saturated=false"));
                    assert!(!after.contains("slot=16") && !after.contains("slot=19"));
                    assert!(after.contains("interpretation=Unknown"));
                    assert_eq!(after.contains("cleanup_eligible=true"), state == Status::Ok);
                    assert!(text.len() <= 8192 && text.is_ascii() && text.ends_with('\n'));
                }
            }
            assert_eq!(snapshot.words, before);
            let mut out = Sink::<8192>::new();
            // Same sink must reject an appended cut if the original consumes capacity.
            for _ in 0..8179 {
                out.write_str("x").unwrap();
            }
            format_postshutdown(Some(postshutdown_cut(slots)), Cleanup::unknown(), &mut out);
            let text = out.finish();
            assert!(text.len() <= 8192 && text.ends_with(" truncated=1\n"));
        }
        #[tokio::test]
        async fn immutable_lane_isolation_and_optin() {
            assert!(worker().is_none());
            mark(&None, Edge::Admission, Reason::Ok);
            let (result, snapshot) = capture_call(Call::QueryInitial, async {
                enter(Instant::now() + ISSUE_BUDGET, "guard1");
                let first = worker().unwrap();
                enter(Instant::now() + ISSUE_BUDGET, "guard2");
                let second = worker().unwrap();
                mark(&Some(first.clone()), Edge::ClosureStart, Reason::Ok);
                mark(&Some(second.clone()), Edge::ClosureStart, Reason::Pending);
                Ok::<_, FoundError>((first, second))
            })
            .await;
            let (first, second) = result.unwrap();
            assert_eq!(first.lane, Lane::Guard1);
            assert_eq!(second.lane, Lane::Guard2);
            assert_ne!(snapshot.words[27], 0);
            assert_ne!(snapshot.words[41], 0);
            let (next, isolated) = capture_call(Call::ProofInitial, async {
                enter(Instant::now() + ISSUE_BUDGET, "guard1");
                let next = worker().unwrap();
                mark(&Some(first.clone()), Edge::ClosureStart, Reason::Pending);
                Ok::<_, FoundError>(next)
            })
            .await;
            assert_eq!(isolated.words, [0; 48]);
            assert!(!Arc::ptr_eq(&first.slots, &next.unwrap().slots));
            assert!(worker().is_none());
            capture(async {
                enter(Instant::now() + ISSUE_BUDGET, "guard1");
                assert!(worker().is_none());
                caller(Reason::MissingRaft);
                decision(Decision::ReadyLate);
                CAPTURE.with(|cell| {
                    let buffer = cell.borrow();
                    assert_eq!(buffer.reason, Reason::Unknown);
                    assert_eq!(buffer.decision, Decision::Unknown);
                });
            })
            .await;
        }
        #[test]
        fn found_entry_markers_are_scoped_once_and_do_not_retain_stores() {
            let slots = Arc::new(Slots::new());
            let registry = std::sync::Mutex::new(Vec::new());
            // Synthetic fixture identities are internal and never rendered.
            registry
                .lock()
                .unwrap()
                .push((usize::MAX, usize::MAX - 1, Arc::downgrade(&slots)));
            let p = super::super::LogPos {
                term: 3,
                leader: 1,
                index: 7,
            };
            entry_at(&registry, Instant::now(), usize::MAX, p, 4);
            entry_at(&registry, Instant::now(), usize::MAX - 1, p, 5);
            entry_at(&registry, Instant::now(), usize::MAX - 1, p, 6);
            entry_at(&registry, Instant::now(), usize::MAX - 1, p, 7);
            let frozen: [u64; 20] = std::array::from_fn(|i| slots.trace[i].load(Ordering::Acquire));
            assert_eq!(&frozen[1..4], &[4, 2, 8]);
            assert_eq!(&frozen[17..20], &[4, 2, 8]);
            assert!(frozen[4..8].windows(2).all(|v| v[0] != 0 && v[0] <= v[1]));
            entry_at(&registry, Instant::now(), usize::MAX, p, 4);
            assert_eq!(slots.trace[4].load(Ordering::Acquire), frozen[4]);
            let unrelated = Arc::new(Slots::new());
            assert!(
                unrelated
                    .trace
                    .iter()
                    .all(|v| v.load(Ordering::Acquire) == 0)
            );
            let weak = Arc::downgrade(&slots);
            drop(slots);
            assert!(weak.upgrade().is_none());
            registry
                .lock()
                .unwrap()
                .retain(|(log, _, _)| *log != usize::MAX);
        }
        #[test]
        fn registry_contention_and_poison_omit_evidence() {
            let slots = Arc::new(Slots::new());
            let registry = std::sync::Mutex::new(Vec::new());
            register(&registry, 1, 2, &slots);
            let p = super::super::LogPos {
                term: 1,
                leader: 1,
                index: 2,
            };
            let at = slots.start + std::time::Duration::from_micros(123);
            {
                let held = registry.lock().unwrap();
                // Same-thread calls while holding the lock deterministically exercise skip.
                register(&registry, 3, 4, &slots);
                entry_at(&registry, at, 1, p, 4);
                assert_eq!(held.len(), 1);
                assert!(slots.trace.iter().all(|w| w.load(Ordering::Acquire) == 0));
            }
            entry_at(&registry, at, 1, p, 4);
            assert_eq!(
                slots.trace[4].load(Ordering::Acquire),
                pack(123, Reason::Ok)
            );
            // Poison a private registry, never the process-global registry.
            let poisoned = std::sync::Mutex::new(Vec::new());
            let _ = std::panic::catch_unwind(|| {
                let _held = poisoned.lock().unwrap();
                panic!("private recorder registry poison");
            });
            register(&poisoned, 1, 2, &slots);
            entry_at(&poisoned, at, 1, p, 5);
            assert_eq!(slots.trace[5].load(Ordering::Acquire), 0);
        }
        #[test]
        fn slot_packing_absence_saturation_cutoff() {
            let slots = Arc::new(Slots::new());
            assert_eq!(slots.freeze(), [0; 48]);
            assert_eq!(pack(0, Reason::Ok), 256 | Reason::Ok as u64);
            assert_eq!(
                (pack(MAX_US, Reason::Ok) >> 8) & ((1u64 << 48) - 1),
                MAX_US as u64 + 1
            );
            assert_eq!(pack(MAX_US, Reason::Ok) >> 56, 0);
            assert_eq!(pack(MAX_US + 1, Reason::Ok) >> 56, 1);
            let h = Some(Handle {
                slots: slots.clone(),
                lane: Lane::Guard1,
            });
            mark(&h, Edge::StoreReply, Reason::StoreCorrupt);
            let frozen = slots.freeze();
            mark(&h, Edge::OwnerSend, Reason::Pending);
            assert_ne!(frozen[29], 0);
            assert_eq!(frozen[31], 0);
            assert_ne!(slots.freeze()[31], 0);
            assert_eq!(frozen[43], 0);
        }
        #[tokio::test]
        async fn formatter_cap_ascii_primary_newline() {
            let (_, mut snapshot) = capture_call(Call::F10Final, async {
                decision(Decision::ReadyLate);
                caller(Reason::MissingRaft);
                Err::<(), _>(FoundError::Unavailable)
            })
            .await;
            snapshot.events = [Some(Event {
                elapsed_us: u128::MAX,
                remaining_us: u128::MAX,
                phase: "preflight",
                edge: "enter",
                result: "Pending",
                pending: Some("preflight"),
            }); CAP];
            snapshot.len = CAP;
            snapshot.words = [u64::MAX; 48];
            let mut out = Sink::<8192>::new();
            format_snapshot(&snapshot, Cleanup::unknown(), &mut out);
            while out.write_str("0123456789").is_ok() {}
            let text = out.finish();
            assert!(text.starts_with(
                "[FOUNDER-OBS] primary=ReadyLate result=Unavailable reason=MissingRaft"
            ));
            assert!(text.len() <= 8192 && text.is_ascii() && text.ends_with(" truncated=1\n"));
            let mut fourth = Sink::<512>::new();
            assert!(fourth.write_str("marker").is_ok());
            assert_eq!(fourth.finish(), "marker\n");
            let mut ascii = Sink::<512>::new();
            assert!(ascii.write_str("\u{e9}").is_err());
            assert_eq!(ascii.finish(), " truncated=1\n");
        }
        #[tokio::test]
        async fn error_cleanup_scalar_precedence() {
            let (result, snapshot) = capture_call(Call::ProofInitial, async {
                caller(Reason::MissingRaft);
                decision(Decision::InnerOnTime);
                Err::<(), _>(FoundError::Unavailable)
            })
            .await;
            let mut cleanup = Cleanup::unknown();
            cleanup.shutdown = Status::Failed;
            let mut out = Sink::<8192>::new();
            format_snapshot(&snapshot, cleanup, &mut out);
            let text = out.finish();
            assert!(
                text.find("result=Unavailable").unwrap()
                    < text.find("cleanup_shutdown=Failed").unwrap()
            );
            assert_eq!(result, Err(FoundError::Unavailable));
            assert_eq!(
                store_reason(&Err::<(), _>(super::super::StoreError::Poisoned)),
                Reason::MutexPoison
            );
            assert_eq!(
                store_reason(&Err::<(), _>(super::super::StoreError::Corrupt)),
                Reason::StoreCorrupt
            );
            assert_eq!(
                store_reason(&Ok::<_, super::super::StoreError>(())),
                Reason::Ok
            );
            // Every closed scalar is ASCII; no engine error payload is formatted.
            for reason in [Reason::SenderClosed, Reason::Unknown] {
                let mut sink = Sink::<512>::new();
                write!(sink, "{reason:?}").unwrap();
                assert!(sink.finish().is_ascii());
            }
        }
    }
}
impl ConsensusNode {
    // Local admission snapshot only; never an authorization read.
    async fn preflight(&self) -> Result<Pre, FoundError> {
        #[cfg(test)]
        let obs = observation::worker();
        let permit = self.read_permits.clone().try_acquire_owned().map_err(|_| {
            #[cfg(test)]
            observation::mark(
                &obs,
                observation::Edge::Admission,
                observation::Reason::Busy,
            );
            FoundError::Busy
        })?;
        #[cfg(test)]
        observation::mark(&obs, observation::Edge::Admission, observation::Reason::Ok);
        let (tx, rx) = oneshot::channel();
        {
            let mut owners = self.owners.lock().map_err(|_| {
                #[cfg(test)]
                observation::mark(
                    &obs,
                    observation::Edge::OwnersGate,
                    observation::Reason::OwnersPoison,
                );
                FoundError::Unavailable
            })?;
            if owners.closed {
                #[cfg(test)]
                observation::mark(
                    &obs,
                    observation::Edge::OwnersGate,
                    observation::Reason::OwnersClosed,
                );
                return Err(FoundError::Unavailable);
            }
            while owners.set.try_join_next().is_some() {}
            let sm = match self.sm.as_ref() {
                Some(value) => value,
                None => {
                    #[cfg(test)]
                    observation::mark(
                        &obs,
                        observation::Edge::OwnersGate,
                        observation::Reason::MissingStore,
                    );
                    return Err(FoundError::Unavailable);
                }
            }
            .clone();
            #[cfg(test)]
            let owner_obs = obs.clone();
            owners.set.spawn(async move {
                #[cfg(test)]
                observation::mark(
                    &owner_obs,
                    observation::Edge::OwnerEnter,
                    observation::Reason::Ok,
                );
                #[cfg(test)]
                observation::mark(
                    &owner_obs,
                    observation::Edge::BlockingSubmit,
                    observation::Reason::Pending,
                );
                #[cfg(test)]
                let blocking_obs = owner_obs.clone();
                let result = blocking(move || {
                    #[cfg(test)]
                    observation::mark(
                        &blocking_obs,
                        observation::Edge::ClosureStart,
                        observation::Reason::Ok,
                    );
                    let result = (|| {
                        let s = sm.0.lock().map_err(|_| StoreError::Poisoned)?;
                        #[cfg(test)]
                        observation::mark(
                            &blocking_obs,
                            observation::Edge::MutexAcquired,
                            observation::Reason::Ok,
                        );
                        Ok(Pre {
                            cluster: codec::cluster(s.state()),
                            founded: s.state().founded(),
                            next: s.state().next_node_id(),
                            applied: s.applied().is_some(),
                            membership: s.membership().is_some(),
                        })
                    })();
                    #[cfg(test)]
                    if blocking_obs.is_some() {
                        observation::mark(
                            &blocking_obs,
                            observation::Edge::StoreReply,
                            observation::store_reason(&result),
                        );
                    }
                    drop(sm);
                    drop(permit);
                    result
                })
                .await
                .map_err(|_| {
                    #[cfg(test)]
                    observation::mark(
                        &owner_obs,
                        observation::Edge::BlockingJoin,
                        observation::Reason::BlockingError,
                    );
                    FoundError::Unavailable
                });
                #[cfg(test)]
                observation::mark(
                    &owner_obs,
                    observation::Edge::OwnerSend,
                    observation::Reason::Pending,
                );
                let _ = tx.send(result);
            });
        }
        {
            #[cfg(test)]
            let rx = async {
                rx.await.inspect_err(|_| {
                    observation::mark(
                        &obs,
                        observation::Edge::CallerReply,
                        observation::Reason::ReceiverClosed,
                    );
                })
            };
            rx.await.unwrap_or(Err(FoundError::Unavailable))
        }
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
        #[cfg(test)]
        observation::bind(self, deadline);
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
            let raft = match self.raft.as_ref() {
                Some(value) => value,
                None => {
                    #[cfg(test)]
                    observation::caller(observation::Reason::MissingRaft);
                    return Err(FoundError::Unavailable);
                }
            };
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
                    .map_err(|_error| {
                        #[cfg(test)]
                        if observation::active() {
                            observation::phase_reply(11, observation::leader_wait_reason(&_error));
                        }
                        FoundError::Unavailable
                    })
            )?;
            #[cfg(test)]
            observation::metrics(self, 9);
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
                Ok(r) if Instant::now() < deadline => {
                    #[cfg(test)]
                    observation::metrics(self, 13);
                    #[cfg(test)]
                    observation::decision(observation::Decision::InnerOnTime);
                    r
                }
                #[cfg(test)]
                Ok(_) => {
                    observation::metrics(self, 13);
                    #[cfg(test)]
                    observation::decision(observation::Decision::ReadyLate);
                    Err(FoundError::Unavailable)
                }
                _ => {
                    #[cfg(test)]
                    observation::metrics(self, 13);
                    #[cfg(test)]
                    observation::decision(observation::Decision::WaitElapsed);
                    Err(FoundError::Unavailable)
                }
            }
        )
    }
}
#[cfg(test)]
#[path = "found_tests.rs"]
mod found_tests;
