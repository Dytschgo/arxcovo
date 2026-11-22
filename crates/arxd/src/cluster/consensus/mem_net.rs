//! Bounded in-process RPC test double. No production transport.
use super::diagnostics::Sample;
use super::*;
use openraft::{
    errors::{RPCError, ReplicationClosed, StreamingError, Unreachable},
    network::{RPCOption, RaftNetworkFactory, RaftNetworkV2},
    raft::{
        AppendEntriesRequest, AppendError, AppendResult, SnapshotResponse, VoteRequest,
        VoteResponse,
    },
    type_config::alias::{SnapshotOf, VoteOf},
};
use std::collections::VecDeque;
use std::fmt::Write as _;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

const RPC_RECORDS: usize = 64;
const TRACE_BYTES: usize = 4096;

#[derive(Clone, Copy, Debug)]
pub(super) enum ElectionPhase {
    MismatchInstalled,
    ReplacementObserved,
    MismatchDetected,
    OldTermBarrierExited,
    IdentityRestored,
    ConvergeSelected,
    FreshReadEntered,
    FreshReadReturned,
}

#[derive(Clone, Copy, Debug)]
enum RpcKind {
    Vote,
    PreVote,
    Append,
}

#[derive(Clone, Copy, Debug)]
enum RpcStage {
    Send,
    Deliver,
    Drop,
    Reply,
}

#[derive(Clone, Copy)]
struct RpcRecord {
    kind: RpcKind,
    from: u64,
    to: u64,
    term: u64,
}

#[derive(Clone, Copy)]
enum Event {
    Phase(
        ElectionPhase,
        usize,
        Sample,
        Option<super::diagnostics::Fail>,
    ),
    Rpc(RpcRecord, RpcStage, &'static str, Option<u64>),
}

#[derive(Clone, Copy)]
struct TraceRecord {
    seq: u64,
    ms: u32,
    event: Event,
}

impl TraceRecord {
    fn line(self) -> String {
        let mut line = format!("#{} {}ms ", self.seq, self.ms);
        let number = |n: Option<u64>| n.map_or_else(|| "-".to_owned(), |v| v.to_string());
        match self.event {
            Event::Phase(phase, node, sample, fail) => {
                let outcome = fail.map_or_else(
                    || {
                        if matches!(phase, ElectionPhase::FreshReadReturned) {
                            "ok"
                        } else {
                            "-"
                        }
                        .to_owned()
                    },
                    |f| format!("{f:?}"),
                );
                let role = if sample.state.is_ascii_alphabetic() {
                    char::from(sample.state)
                } else {
                    '?'
                };
                let _ = writeln!(
                    line,
                    "{phase:?} n{node} t{} role={role} leader={} ack_age_ms={} result={outcome}",
                    sample.term,
                    number(sample.leader),
                    number(sample.ack_ms),
                );
            }
            Event::Rpc(rpc, stage, outcome, reply_term) => {
                let _ = writeln!(
                    line,
                    "{stage:?} {:?} {}>{} t{} {outcome} reply_t={}",
                    rpc.kind,
                    rpc.from,
                    rpc.to,
                    rpc.term,
                    number(reply_term),
                );
            }
        }
        line
    }
}

#[derive(Default)]
struct TraceBuffer {
    overwritten: usize,
    rpcs: VecDeque<TraceRecord>,
}

impl TraceBuffer {
    fn push(&mut self, record: TraceRecord) {
        if self.rpcs.len() == RPC_RECORDS {
            self.rpcs.pop_front();
            self.overwritten += 1;
        }
        self.rpcs.push_back(record);
    }
}

/// Opt-in companion to the existing metrics ring. Only the TLS mismatch case arms it.
#[derive(Default)]
pub(super) struct ElectionTrace {
    active: AtomicBool,
    complete: AtomicBool,
    clock: OnceLock<(Instant, u32)>,
    seq: AtomicU64,
    contended: AtomicUsize,
    buffer: Mutex<TraceBuffer>,
    // Reserve one slot per fixture phase, independent of RPC lock contention.
    phases: [OnceLock<TraceRecord>; ElectionPhase::FreshReadReturned as usize + 1],
}

impl ElectionTrace {
    pub(super) fn begin(&self, elapsed_ms: u32) {
        if let Ok(mut buffer) = self.buffer.try_lock() {
            buffer.rpcs.reserve_exact(RPC_RECORDS);
            let _ = self.clock.set((Instant::now(), elapsed_ms));
            self.active.store(true, Ordering::Release);
        }
    }

    pub(super) fn active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }

    pub(super) fn phase(
        &self,
        phase: ElectionPhase,
        node: usize,
        sample: Sample,
        fail: Option<super::diagnostics::Fail>,
    ) {
        if self.active()
            && let Some(record) = self.stamp(Event::Phase(phase, node, sample, fail))
        {
            let _ = self.phases[phase as usize].set(record);
        }
    }

    fn record(&self, event: Event) {
        if self.active() {
            match self.buffer.try_lock() {
                Ok(mut buffer) => {
                    if self.active()
                        && let Some(record) = self.stamp(event)
                    {
                        buffer.push(record);
                    }
                }
                Err(_) => {
                    self.contended.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }

    fn stamp(&self, event: Event) -> Option<TraceRecord> {
        let (start, offset_ms) = self.clock.get()?;
        let seq = self.seq.fetch_add(1, Ordering::Relaxed) + 1;
        let elapsed_ms = start.elapsed().as_millis().min(u32::MAX as u128) as u32;
        Some(TraceRecord {
            seq,
            ms: offset_ms.saturating_add(elapsed_ms),
            event,
        })
    }

    pub(super) fn stop_capture(&self) {
        self.active.store(false, Ordering::Release);
    }

    pub(super) fn complete(&self) {
        self.complete.store(true, Ordering::Relaxed);
    }

    pub(super) fn dump_if_incomplete(&self) {
        self.stop_capture();
        if !self.complete.load(Ordering::Relaxed) && self.clock.get().is_some() {
            use std::io::Write as _;
            let out = self.render();
            let _ = std::io::stderr().write_all(out.as_bytes());
        }
    }

    fn render(&self) -> String {
        // Reserve space for the summary and all lifecycle markers, then retain
        // the newest complete RPC lines that fit. Merge by capture sequence.
        const HEADER_BYTES: usize = 128;
        let mut budget = TRACE_BYTES - HEADER_BYTES;
        let mut lines = Vec::new();
        for record in self.phases.iter().filter_map(OnceLock::get) {
            let line = record.line();
            budget -= line.len();
            lines.push((record.seq, line));
        }
        let mut omitted = 0;
        let mut overwritten = 0;
        let rpc_busy = match self.buffer.try_lock() {
            Ok(buffer) => {
                overwritten = buffer.overwritten;
                omitted = buffer.rpcs.len();
                for record in buffer.rpcs.iter().rev() {
                    let line = record.line();
                    if line.len() > budget {
                        break;
                    }
                    budget -= line.len();
                    omitted -= 1;
                    lines.push((record.seq, line));
                }
                false
            }
            Err(_) => true,
        };
        lines.sort_unstable_by_key(|(seq, _)| *seq);
        let contended = self.contended.load(Ordering::Relaxed);
        let dropped = overwritten.saturating_add(contended);
        let mut out = format!(
            "[DIAG-election] dropped={dropped} contended={contended} omitted={omitted} rpc_busy={rpc_busy}\n"
        );
        for (_, line) in lines {
            out.push_str(&line);
        }
        out
    }
}
#[derive(Default)]
pub(super) struct MemRouter {
    pub election_trace: ElectionTrace,
    pub nodes: Mutex<BTreeMap<u64, Engine>>,
    pub isolated: Mutex<BTreeSet<u64>>,
    pub authenticated: Mutex<BTreeMap<u64, crate::cluster::pki::verify::AuthenticatedPeer>>,
    pub mismatch: AtomicUsize,
    pub append_max: AtomicUsize,
    pub snapshot_max: AtomicUsize,
    pub snapshots: AtomicUsize,
    pub snapshots_to: Mutex<BTreeMap<u64, usize>>,
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
    fn record_rpc(
        &self,
        rpc: RpcRecord,
        stage: RpcStage,
        outcome: &'static str,
        reply: Option<u64>,
    ) {
        if (1..=3).contains(&rpc.from) && (1..=3).contains(&rpc.to) {
            self.router
                .election_trace
                .record(Event::Rpc(rpc, stage, outcome, reply));
        }
    }

    fn send(&self, kind: RpcKind, term: u64) -> RpcRecord {
        let rpc = RpcRecord {
            kind,
            from: self.source,
            to: self.target,
            term,
        };
        self.record_rpc(rpc, RpcStage::Send, "pending", None);
        rpc
    }

    fn dispatch(&self, sender: u64, rpc: RpcRecord) -> Result<Engine, Unreachable<Arx>> {
        match self.remote(sender) {
            Ok(remote) => {
                self.record_rpc(rpc, RpcStage::Deliver, "ok", None);
                Ok(remote)
            }
            Err((error, reason)) => {
                self.record_rpc(rpc, RpcStage::Drop, reason, None);
                Err(error)
            }
        }
    }

    fn remote(&self, sender: u64) -> Result<Engine, (Unreachable<Arx>, &'static str)> {
        let identity = self
            .router
            .authenticated
            .lock()
            .unwrap()
            .get(&self.source)
            .copied();
        let mismatch = match identity {
            Some(peer) => crate::cluster::pki::verify::check_sender(peer.node(), sender).is_err(),
            None => sender != self.source,
        };
        if mismatch {
            self.router.mismatch.fetch_add(1, Ordering::Relaxed);
            return Err((unreachable(), "mismatch-drop"));
        }
        let isolated = self.router.isolated.lock().unwrap();
        if isolated.contains(&self.source) || isolated.contains(&self.target) {
            return Err((unreachable(), "isolated"));
        }
        self.router
            .nodes
            .lock()
            .unwrap()
            .get(&self.target)
            .cloned()
            .ok_or_else(|| (unreachable(), "missing"))
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
        let record = self.send(RpcKind::Append, rpc.vote.leader_id.term);
        self.router
            .append_max
            .fetch_max(rpc.entries.len(), Ordering::Relaxed);
        if rpc.entries.len() > 64 {
            self.record_rpc(record, RpcStage::Drop, "oversize", None);
            return Err(RPCError::Unreachable(unreachable()));
        }
        let result = self
            .dispatch(rpc.vote.leader_id.node_id, record)?
            .append_entries(rpc)
            .await;
        let (outcome, reply) = match &result {
            Ok(Ok(_)) => ("ok", None),
            Ok(Err(AppendError::Conflict { .. })) => ("conflict", None),
            Ok(Err(AppendError::HigherVote(vote))) => ("higher-vote", Some(vote.leader_id.term)),
            Err(_) => ("err", None),
        };
        self.record_rpc(record, RpcStage::Reply, outcome, reply);
        result.map_err(|_| RPCError::Unreachable(unreachable()))
    }
    async fn vote(
        &mut self,
        rpc: VoteRequest<Arx>,
        _option: RPCOption,
    ) -> Result<VoteResponse<Arx>, RPCError<Arx>> {
        let record = self.send(RpcKind::Vote, rpc.vote.leader_id.term);
        let result = self
            .dispatch(rpc.vote.leader_id.node_id, record)?
            .vote(rpc)
            .await;
        let (outcome, reply) = match &result {
            Ok(response) => (
                if response.vote_granted {
                    "granted"
                } else {
                    "denied"
                },
                Some(response.vote.leader_id.term),
            ),
            Err(_) => ("err", None),
        };
        self.record_rpc(record, RpcStage::Reply, outcome, reply);
        result.map_err(|_| RPCError::Unreachable(unreachable()))
    }
    async fn pre_vote(
        &mut self,
        rpc: VoteRequest<Arx>,
        _option: RPCOption,
    ) -> Result<VoteResponse<Arx>, RPCError<Arx>> {
        // Preserve RaftNetworkV2's default local grant (no dispatch).
        let record = self.send(RpcKind::PreVote, rpc.vote.leader_id.term);
        let response = VoteResponse::<Arx>::new(rpc.vote, None, true);
        self.record_rpc(
            record,
            RpcStage::Reply,
            "local-grant",
            Some(response.vote.leader_id.term),
        );
        Ok(response)
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
        let remote = self
            .remote(vote.leader_id.node_id)
            .map_err(|(error, _)| error)?;
        self.router.snapshots.fetch_add(1, Ordering::Relaxed);
        *self
            .router
            .snapshots_to
            .lock()
            .unwrap()
            .entry(self.target)
            .or_default() += 1;
        remote
            .install_full_snapshot(vote, snapshot)
            .await
            .map_err(|_| StreamingError::Unreachable(unreachable()))
    }
}

#[cfg(test)]
mod election_trace_tests {
    use super::*;

    const RPC: RpcRecord = RpcRecord {
        kind: RpcKind::Vote,
        from: 1,
        to: 3,
        term: 3,
    };

    #[test]
    fn election_trace_orders_phase_and_rpc_records() {
        let trace = ElectionTrace::default();
        trace.record(Event::Rpc(RPC, RpcStage::Send, "pending", None));
        trace.begin(100);
        let sample = Sample {
            term: 2,
            state: b'L',
            leader: Some(3),
            ack_ms: Some(12),
            ..Sample::default()
        };
        trace.phase(ElectionPhase::IdentityRestored, 3, sample, None);
        trace.record(Event::Rpc(RPC, RpcStage::Send, "pending", None));
        trace.record(Event::Rpc(RPC, RpcStage::Deliver, "ok", None));
        trace.record(Event::Rpc(RPC, RpcStage::Reply, "granted", Some(3)));
        trace.phase(
            ElectionPhase::FreshReadReturned,
            3,
            sample,
            Some(super::super::diagnostics::Fail::NotLeader),
        );
        trace.stop_capture();
        trace.record(Event::Rpc(RPC, RpcStage::Drop, "mismatch-drop", None));

        let output = trace.render();
        let records: Vec<_> = output.lines().skip(1).collect();
        assert_eq!(records.len(), 5);
        let mut prev_ms = 100;
        for (seq, line) in records.iter().enumerate() {
            let (stamp, _) = line.split_once("ms ").unwrap();
            let (index, ms) = stamp.split_once(' ').unwrap();
            assert_eq!(index, format!("#{}", seq + 1));
            let ms = ms.parse::<u32>().unwrap();
            assert!(ms >= prev_ms);
            prev_ms = ms;
        }
        assert!(
            records[0].ends_with("IdentityRestored n3 t2 role=L leader=3 ack_age_ms=12 result=-")
        );
        assert!(records[1].ends_with("Send Vote 1>3 t3 pending reply_t=-"));
        assert!(records[2].ends_with("Deliver Vote 1>3 t3 ok reply_t=-"));
        assert!(records[3].ends_with("Reply Vote 1>3 t3 granted reply_t=3"));
        assert!(
            records[4].ends_with(
                "FreshReadReturned n3 t2 role=L leader=3 ack_age_ms=12 result=NotLeader"
            )
        );
    }

    #[test]
    fn election_trace_bounds_ring_and_preserves_tail_within_byte_cap() {
        let trace = ElectionTrace::default();
        trace.begin(u32::MAX);
        trace.phase(ElectionPhase::MismatchInstalled, 1, Sample::default(), None);
        {
            let _busy = trace.buffer.lock().unwrap();
            trace.record(Event::Rpc(RPC, RpcStage::Drop, "mismatch-drop", None));
            trace.phase(ElectionPhase::MismatchDetected, 1, Sample::default(), None);
        }
        let append = RpcRecord {
            kind: RpcKind::Append,
            term: u64::MAX,
            ..RPC
        };
        for _ in 0..RPC_RECORDS + 3 {
            trace.record(Event::Rpc(
                append,
                RpcStage::Reply,
                "higher-vote",
                Some(u64::MAX),
            ));
        }
        trace.phase(ElectionPhase::FreshReadReturned, 3, Sample::default(), None);
        assert_eq!(trace.buffer.lock().unwrap().rpcs.len(), RPC_RECORDS);

        let output = trace.render();
        let mut lines = output.lines();
        let header = lines.next().unwrap();
        assert!(header.contains("dropped=4 contended=1"));
        assert!(!header.contains("omitted=0"));
        assert!(output.len() <= TRACE_BYTES);
        assert!(output.ends_with('\n'));
        assert!(lines.next().unwrap().contains("MismatchInstalled"));
        assert!(lines.next().unwrap().contains("MismatchDetected"));
        let mut tail = lines.rev();
        assert!(tail.next().unwrap().contains("FreshReadReturned"));
        assert!(
            tail.next()
                .unwrap()
                .starts_with(&format!("#{} ", RPC_RECORDS + 5))
        );
    }

    #[test]
    fn election_trace_formats_only_static_categories_and_synthetic_metadata() {
        let record = TraceRecord {
            seq: 7,
            ms: 37,
            event: Event::Rpc(
                RpcRecord {
                    kind: RpcKind::PreVote,
                    ..RPC
                },
                RpcStage::Reply,
                "local-grant",
                Some(3),
            ),
        };
        assert_eq!(
            record.line(),
            "#7 37ms Reply PreVote 1>3 t3 local-grant reply_t=3\n"
        );
        let record = TraceRecord {
            event: Event::Rpc(RPC, RpcStage::Drop, "mismatch-drop", None),
            ..record
        };
        assert_eq!(
            record.line(),
            "#7 37ms Drop Vote 1>3 t3 mismatch-drop reply_t=-\n"
        );
    }
}
