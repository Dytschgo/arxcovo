//! Test-only bounded three-node diagnostics (D1). Never compiled into production.
//!
//! Records integers and enums only: no paths, SQL, payloads, keys, store addresses
//! or error strings. Store addresses key the registry internally and are never
//! rendered. Metrics arrive through a watch channel that coalesces updates, so a
//! ring is a sampled history, not a complete engine trace; a missing transition
//! in a ring proves nothing. Classifications are diagnostic candidates, not
//! verdicts about production behavior.
use super::{Engine, ShutdownReport};
use openraft::{
    Instant as _, RaftMetrics, ServerState, type_config::async_runtime::watch::WatchReceiver,
};
use std::{
    collections::VecDeque,
    fmt::Write as _,
    io::Write as _,
    sync::{
        Arc, Mutex, PoisonError, TryLockError, Weak,
        atomic::{AtomicU32, AtomicU64, Ordering::Relaxed},
    },
    time::{Duration, Instant},
};
use tokio::{task::JoinHandle, time::MissedTickBehavior};

pub(super) const RING: usize = 128;
pub(super) const OUT_MAX: usize = 8192;
pub(super) const HIGH_MS: u32 = 200;
const LAG_TICK_MS: u64 = 50;
const LAG_EVENT_MS: u32 = 100;
const LAG_EVENTS: usize = 32;
const EDGES: [u32; 4] = [10, 50, 200, 1000];
const TAIL: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Op {
    Vote,
    Append,
    Apply,
    Snapshot,
    Other,
}
const OPS: [Op; 5] = [Op::Vote, Op::Append, Op::Apply, Op::Snapshot, Op::Other];

fn sat(a: &AtomicU32) {
    let _ = a.fetch_update(Relaxed, Relaxed, |v| v.checked_add(1));
}
fn ms(d: Duration) -> u32 {
    d.as_millis().min(u32::MAX as u128) as u32
}
/// Latency buckets <10, <50, <200, <1000, >=1000 ms plus maximum.
#[derive(Default)]
pub(super) struct Buckets {
    n: [AtomicU32; 5],
    max_ms: AtomicU32,
}
impl Buckets {
    pub fn record(&self, d: Duration) {
        let v = ms(d);
        sat(&self.n[EDGES.iter().position(|e| v < *e).unwrap_or(4)]);
        self.max_ms.fetch_max(v, Relaxed);
    }
    pub fn counts(&self) -> [u32; 5] {
        std::array::from_fn(|i| self.n[i].load(Relaxed))
    }
    pub fn max_ms(&self) -> u32 {
        self.max_ms.load(Relaxed)
    }
    /// Operations at or above `HIGH_MS`.
    pub fn high(&self) -> u32 {
        let c = self.counts();
        c[3].saturating_add(c[4])
    }
    fn render(&self, out: &mut String) {
        let c = self.counts();
        let _ = write!(
            out,
            "{}:{}:{}:{}:{}^{}",
            c[0],
            c[1],
            c[2],
            c[3],
            c[4],
            self.max_ms()
        );
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Sample {
    pub t_ms: u32,
    pub term: u64,
    pub state: u8,
    pub leader: Option<u64>,
    pub last: Option<u64>,
    pub committed: Option<u64>,
    pub applied: Option<u64>,
    pub ack_ms: Option<u64>,
}
impl Sample {
    pub fn of(m: &RaftMetrics<super::Arx>, t_ms: u32) -> Self {
        Self {
            t_ms,
            term: m.current_term,
            state: match m.state {
                ServerState::Leader => b'L',
                ServerState::Follower => b'F',
                ServerState::Candidate => b'C',
                ServerState::Learner => b'l',
                ServerState::Shutdown => b'S',
            },
            leader: m.current_leader,
            last: m.last_log_index,
            committed: m.local_committed.as_ref().map(|p| p.index),
            applied: m.last_applied.as_ref().map(|p| p.index),
            // Age of the quorum ack when sampled, not when metrics were published.
            ack_ms: m
                .last_quorum_acked
                .as_ref()
                .map(|a| (**a).elapsed().as_millis().min(u64::MAX as u128) as u64),
        }
    }
    fn render(&self, out: &mut String) {
        let o = |v: Option<u64>| v.map_or_else(|| "-".to_owned(), |v| v.to_string());
        let state = if self.state.is_ascii_alphabetic() {
            self.state as char
        } else {
            '?'
        };
        let _ = write!(
            out,
            "{},{},{},{},{},{},{},{}",
            self.t_ms,
            self.term,
            state,
            o(self.leader),
            o(self.last),
            o(self.committed),
            o(self.applied),
            o(self.ack_ms)
        );
    }
}

/// Bounded per-node history; counters span the whole run, samples the last `RING`.
#[derive(Default)]
pub(super) struct Ring {
    buf: VecDeque<Sample>,
    pub seen: u32,
    pub overwritten: u32,
    pub term_changes: u32,
    pub leader_changes: u32,
    pub state_changes: u32,
    last: Option<Sample>,
}
impl Ring {
    pub fn push(&mut self, s: Sample) {
        if let Some(p) = self.last {
            if p.term != s.term {
                self.term_changes = self.term_changes.saturating_add(1);
            }
            if p.leader != s.leader {
                self.leader_changes = self.leader_changes.saturating_add(1);
            }
            if p.state != s.state {
                self.state_changes = self.state_changes.saturating_add(1);
            }
        }
        self.last = Some(s);
        self.seen = self.seen.saturating_add(1);
        if self.buf.len() == RING {
            self.buf.pop_front();
            self.overwritten = self.overwritten.saturating_add(1);
        }
        self.buf.push_back(s);
    }
    pub fn len(&self) -> usize {
        self.buf.len()
    }
    /// Retained samples at or after `t_ms` that leave `term` or the Leader state.
    fn left_leadership_since(&self, t_ms: u32, term: u64) -> bool {
        self.buf
            .iter()
            .any(|s| s.t_ms >= t_ms && (s.term != term || s.state != b'L'))
    }
    /// Retained samples at or after `t_ms` that observe a term above `term`.
    fn term_above_since(&self, t_ms: u32, term: u64) -> bool {
        self.buf.iter().any(|s| s.t_ms >= t_ms && s.term > term)
    }
}

pub(super) struct NodeStats {
    pub node: u8,
    ops: [Buckets; 5],
    queue: Buckets,
    /// Controlled per-apply delay in the blocking worker (positive controls only).
    pub apply_delay_ms: AtomicU32,
    pub ring: Mutex<Ring>,
}
impl NodeStats {
    fn new(node: u8) -> Self {
        Self {
            node,
            ops: Default::default(),
            queue: Buckets::default(),
            apply_delay_ms: AtomicU32::new(0),
            ring: Mutex::default(),
        }
    }
    pub fn op(&self, op: Op) -> &Buckets {
        &self.ops[op as usize]
    }
    pub fn queue(&self) -> &Buckets {
        &self.queue
    }
    pub fn high_total(&self) -> u32 {
        self.ops
            .iter()
            .chain([&self.queue])
            .fold(0u32, |a, b| a.saturating_add(b.high()))
    }
    fn op_max(&self) -> u32 {
        self.ops.iter().map(Buckets::max_ms).max().unwrap_or(0)
    }
}

// Store address -> node stats. Addresses never leave this registry.
static REGISTRY: Mutex<Vec<(usize, Weak<NodeStats>)>> = Mutex::new(Vec::new());
fn lookup(store: usize) -> Option<Arc<NodeStats>> {
    let r = REGISTRY.lock().unwrap_or_else(PoisonError::into_inner);
    r.iter()
        .rev()
        .find(|(k, _)| *k == store)
        .and_then(|(_, w)| w.upgrade())
}
pub(super) fn register(store: usize, stats: &Arc<NodeStats>) {
    let mut r = REGISTRY.lock().unwrap_or_else(PoisonError::into_inner);
    r.retain(|(k, w)| *k != store && w.strong_count() > 0);
    r.push((store, Arc::downgrade(stats)));
}
pub(super) fn unregister(stores: &[usize]) {
    let mut r = REGISTRY.lock().unwrap_or_else(PoisonError::into_inner);
    r.retain(|(k, w)| !stores.contains(k) && w.strong_count() > 0);
}
fn unregister_nonblocking(stores: &[usize]) {
    let mut r = match REGISTRY.try_lock() {
        Ok(r) => r,
        Err(TryLockError::Poisoned(p)) => p.into_inner(),
        // Dead weak entries are inert and pruned by the next register.
        Err(TryLockError::WouldBlock) => return,
    };
    r.retain(|(k, w)| !stores.contains(k) && w.strong_count() > 0);
}

/// Adapter-boundary latency: submit to completion, including blocking-pool
/// queueing and store-mutex wait. Recorded on drop, also when cancelled.
pub(super) struct Span {
    stats: Arc<NodeStats>,
    op: Op,
    start: Instant,
}
impl Drop for Span {
    fn drop(&mut self) {
        self.stats.op(self.op).record(self.start.elapsed());
    }
}
pub(super) fn span(store: usize, op: Op) -> Option<Span> {
    lookup(store).map(|stats| Span {
        stats,
        op,
        start: Instant::now(),
    })
}
/// Called first inside a blocking closure: records blocking-pool queue delay and,
/// for applies only, sleeps any controlled delay before the store lock.
pub(super) fn started(store: usize, submitted: Instant, apply: bool) {
    let Some(s) = lookup(store) else { return };
    s.queue.record(submitted.elapsed());
    let d = s.apply_delay_ms.load(Relaxed);
    if apply && d > 0 {
        std::thread::sleep(Duration::from_millis(d as u64));
    }
}

#[derive(Default)]
pub(super) struct Lag {
    pub ticks: AtomicU32,
    pub worst_ms: AtomicU32,
    pub worst_at_ms: AtomicU32,
    pub over: AtomicU32,
    events: Mutex<VecDeque<(u32, u32)>>,
}
impl Lag {
    pub fn record(&self, late: Duration, at_ms: u32) {
        let v = ms(late);
        sat(&self.ticks);
        if v > self.worst_ms.fetch_max(v, Relaxed) {
            self.worst_at_ms.store(at_ms, Relaxed);
        }
        if v >= HIGH_MS {
            sat(&self.over);
        }
        if v >= LAG_EVENT_MS {
            let mut e = self.events.lock().unwrap_or_else(PoisonError::into_inner);
            if e.len() == LAG_EVENTS {
                e.pop_front();
            }
            e.push_back((at_ms, v));
        }
    }
    /// Worst retained lag observed at or after `t_ms`.
    pub fn max_since(&self, t_ms: u32) -> u32 {
        let e = self.events.lock().unwrap_or_else(PoisonError::into_inner);
        e.iter()
            .filter(|(t, _)| *t >= t_ms)
            .map(|(_, v)| *v)
            .max()
            .unwrap_or(0)
    }
}

#[derive(Default)]
pub(super) struct Helpers {
    pub leader_calls: AtomicU32,
    pub leader_polls: AtomicU64,
    pub leader_polls_max: AtomicU32,
    pub write_calls: AtomicU32,
    pub write_retries: AtomicU32,
    pub write_retries_max: AtomicU32,
    pub converge_calls: AtomicU32,
    pub stops: AtomicU32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Helper {
    Leader,
    Write,
    Converge,
    ConvergeWait,
    Stop,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Fail {
    Elapsed,
    NotLeader,
    NoQuorum,
    Unavailable,
    Busy,
    Stopped,
    Unknown,
    Orphaned,
    Wait,
}
/// S3 split. `SameTermCandidate` only means no churn was retained in bounded,
/// coalesced history; it is not a production-defect verdict.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum S3 {
    Churn,
    SameTermCandidate,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Candidate {
    None,
    Cpu,
    Store,
    Both,
}
/// Coarse lead only: runtime lag or blocking-pool queueing points at CPU,
/// slow adapter operations at the store. Both may share one upstream cause.
pub(super) fn classify(lag_ms: u32, queue_ms: u32, op_ms: u32) -> Candidate {
    match (lag_ms >= HIGH_MS || queue_ms >= HIGH_MS, op_ms >= HIGH_MS) {
        (false, false) => Candidate::None,
        (true, false) => Candidate::Cpu,
        (false, true) => Candidate::Store,
        (true, true) => Candidate::Both,
    }
}
pub(super) fn classify_read(before: Sample, after: Sample, history_churn: bool) -> S3 {
    let steady = before.state == b'L'
        && after.state == b'L'
        && before.term == after.term
        && before.leader == after.leader;
    if steady && !history_churn {
        S3::SameTermCandidate
    } else {
        S3::Churn
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Attribution {
    pub helper: Helper,
    pub fail: Fail,
    pub t_ms: u32,
    pub node: u8,
    pub polls: u32,
    pub retries: u32,
    pub read: Option<(Sample, Sample, S3)>,
    pub report: Option<ShutdownReport>,
    pub at: [Option<Sample>; 3],
}

pub(super) struct Diag {
    t0: Instant,
    pub nodes: [Arc<NodeStats>; 3],
    pub lag: Arc<Lag>,
    pub helpers: Helpers,
    first: Mutex<Option<Attribution>>,
    tasks: Vec<JoinHandle<()>>,
    stores: [[usize; 2]; 3],
    finished: bool,
    dumped: bool,
}
impl Diag {
    pub fn new() -> Self {
        Self {
            t0: Instant::now(),
            nodes: std::array::from_fn(|i| Arc::new(NodeStats::new(i as u8 + 1))),
            lag: Arc::default(),
            helpers: Helpers::default(),
            first: Mutex::new(None),
            tasks: Vec::new(),
            stores: [[0; 2]; 3],
            finished: false,
            dumped: false,
        }
    }
    pub fn t_ms(&self) -> u32 {
        ms(self.t0.elapsed())
    }
    /// One owned 50 ms timer-lag probe per fixture.
    pub fn start_probe(&mut self) {
        let lag = self.lag.clone();
        let t0 = self.t0;
        self.tasks.push(tokio::spawn(async move {
            let mut iv = tokio::time::interval(Duration::from_millis(LAG_TICK_MS));
            iv.set_missed_tick_behavior(MissedTickBehavior::Delay);
            loop {
                let due = iv.tick().await;
                let late = tokio::time::Instant::now().saturating_duration_since(due);
                lag.record(late, ms(t0.elapsed()));
            }
        }));
    }
    /// Registers a node's stores and spawns its owned event-driven metrics watcher.
    pub fn watch(&mut self, i: usize, raft: &Engine, stores: [usize; 2]) {
        self.tasks.retain(|t| !t.is_finished());
        self.forget(i);
        for s in stores {
            register(s, &self.nodes[i]);
        }
        self.stores[i] = stores;
        let stats = self.nodes[i].clone();
        let mut rx = raft.metrics();
        let t0 = self.t0;
        self.tasks.push(tokio::spawn(async move {
            loop {
                let s = Sample::of(&rx.borrow_and_update(), ms(t0.elapsed()));
                stats
                    .ring
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(s);
                if rx.changed().await.is_err() {
                    break;
                }
            }
        }));
    }
    pub fn forget(&mut self, i: usize) {
        let s = std::mem::take(&mut self.stores[i]);
        if s != [0; 2] {
            unregister(&s);
        }
    }
    pub fn sample(&self, raft: &Engine) -> Sample {
        Sample::of(&raft.metrics().borrow_watched(), self.t_ms())
    }
    pub fn leader_polls(&self, polls: u32) {
        let h = &self.helpers;
        sat(&h.leader_calls);
        h.leader_polls.fetch_add(polls as u64, Relaxed);
        h.leader_polls_max.fetch_max(polls, Relaxed);
    }
    pub fn write_retries(&self, retries: u32) {
        let h = &self.helpers;
        sat(&h.write_calls);
        let _ = h
            .write_retries
            .fetch_update(Relaxed, Relaxed, |v| v.checked_add(retries));
        h.write_retries_max.fetch_max(retries, Relaxed);
    }
    pub fn attribution(&self, helper: Helper, fail: Fail, node: usize) -> Attribution {
        Attribution {
            helper,
            fail,
            t_ms: self.t_ms(),
            node: node.min(254) as u8 + 1,
            polls: 0,
            retries: 0,
            read: None,
            report: None,
            at: [None; 3],
        }
    }
    /// First recorded failure wins; later ones are consequences.
    pub fn record(&self, a: Attribution) {
        let mut f = self.first.lock().unwrap_or_else(PoisonError::into_inner);
        if f.is_none() {
            *f = Some(a);
        }
    }
    pub fn recorded(&self) -> Option<Attribution> {
        *self.first.lock().unwrap_or_else(PoisonError::into_inner)
    }
    /// Correlates a failed read with bounded history on every node; it does not
    /// require the reading node itself to have observed a new term.
    pub fn read_class(&self, i: usize, before: Sample, after: Sample) -> S3 {
        let history = self.nodes.iter().enumerate().any(|(j, n)| {
            let r = n.ring.lock().unwrap_or_else(PoisonError::into_inner);
            if j == i {
                r.left_leadership_since(before.t_ms, before.term)
            } else {
                r.term_above_since(before.t_ms, before.term)
            }
        });
        classify_read(before, after, history)
    }
    /// Aborts and joins every owned task. Idempotent.
    pub async fn close(&mut self) {
        for t in &self.tasks {
            t.abort();
        }
        for t in self.tasks.drain(..) {
            let _ = t.await;
        }
        for i in 0..3 {
            self.forget(i);
        }
        self.finished = true;
    }
    fn maxima(&self) -> (u32, u32, u32) {
        let q = self
            .nodes
            .iter()
            .map(|n| n.queue.max_ms())
            .max()
            .unwrap_or(0);
        let o = self.nodes.iter().map(|n| n.op_max()).max().unwrap_or(0);
        (self.lag.worst_ms.load(Relaxed), q, o)
    }
    /// One line for passing runs.
    pub fn summary(&self) -> String {
        let mut s = String::from("[DIAG-3n] pass");
        let (lag, q, o) = self.maxima();
        let _ = write!(
            s,
            " t={} lag={}@{} over={} ticks={}",
            self.t_ms(),
            lag,
            self.lag.worst_at_ms.load(Relaxed),
            self.lag.over.load(Relaxed),
            self.lag.ticks.load(Relaxed)
        );
        for n in &self.nodes {
            let _ = write!(s, " n{}", n.node);
            match peek(&n.ring, |r| (r.term_changes, r.leader_changes, r.seen)) {
                Some((t, l, seen)) => {
                    let _ = write!(s, " term={t} ldr={l} seen={seen}");
                }
                None => s.push_str(" ring=busy"),
            }
            let _ = write!(s, " high={}", n.high_total());
        }
        let maxes: Vec<String> = OPS
            .iter()
            .map(|op| {
                let m = self.nodes.iter().map(|n| n.op(*op).max_ms()).max();
                format!("{op:?}={}", m.unwrap_or(0))
            })
            .collect();
        let h = &self.helpers;
        let _ = write!(
            s,
            " max {} queue={} leader={}:{}:{} write={}:{}:{} cand={:?}",
            maxes.join(","),
            q,
            h.leader_calls.load(Relaxed),
            h.leader_polls.load(Relaxed),
            h.leader_polls_max.load(Relaxed),
            h.write_calls.load(Relaxed),
            h.write_retries.load(Relaxed),
            h.write_retries_max.load(Relaxed),
            classify(lag, q, o)
        );
        s.truncate(OUT_MAX);
        s
    }
    /// Bounded failure dump. Nonblocking: held locks render as `busy`.
    pub fn render(&self) -> String {
        let mut out = Out::default();
        let (lag, q, o) = self.maxima();
        out.line(format_args!(
            "[DIAG-3n] v1 fail t={} lag={}@{} over={} ticks={} cand={:?}",
            self.t_ms(),
            lag,
            self.lag.worst_at_ms.load(Relaxed),
            self.lag.over.load(Relaxed),
            self.lag.ticks.load(Relaxed),
            classify(lag, q, o)
        ));
        match self.lag.events.try_lock() {
            Ok(e) => out.line(format_args!(
                "[DIAG-3n] lagev {:?}",
                e.iter().rev().take(8).collect::<Vec<_>>()
            )),
            Err(_) => out.line(format_args!("[DIAG-3n] lagev busy")),
        }
        let h = &self.helpers;
        out.line(format_args!(
            "[DIAG-3n] helpers leader={}:{}:{} write={}:{}:{} converge={} stop={}",
            h.leader_calls.load(Relaxed),
            h.leader_polls.load(Relaxed),
            h.leader_polls_max.load(Relaxed),
            h.write_calls.load(Relaxed),
            h.write_retries.load(Relaxed),
            h.write_retries_max.load(Relaxed),
            h.converge_calls.load(Relaxed),
            h.stops.load(Relaxed)
        ));
        match peek(&self.first, |a| *a) {
            None => out.line(format_args!("[DIAG-3n] attr busy")),
            Some(None) => out.line(format_args!("[DIAG-3n] attr none")),
            Some(Some(a)) => {
                out.line(format_args!(
                    "[DIAG-3n] attr helper={:?} fail={:?} node={} t={} polls={} retries={}",
                    a.helper, a.fail, a.node, a.t_ms, a.polls, a.retries
                ));
                if let Some((b, f, c)) = a.read {
                    let mut s = String::new();
                    s.push_str("[DIAG-3n] s3 before=");
                    b.render(&mut s);
                    s.push_str(" after=");
                    f.render(&mut s);
                    let _ = write!(s, " class={c:?}");
                    out.line(format_args!("{s}"));
                }
                if let Some(r) = a.report {
                    out.line(format_args!("[DIAG-3n] report {r:?}"));
                }
                let mut s = String::from("[DIAG-3n] at");
                for (i, x) in a.at.iter().enumerate() {
                    let _ = write!(s, " n{}=", i + 1);
                    match x {
                        Some(x) => x.render(&mut s),
                        None => s.push('-'),
                    }
                }
                out.line(format_args!("{s}"));
            }
        }
        for n in &self.nodes {
            let mut s = format!("[DIAG-3n] n{} ops", n.node);
            for op in OPS {
                let _ = write!(s, " {op:?}=");
                n.op(op).render(&mut s);
            }
            s.push_str(" queue=");
            n.queue.render(&mut s);
            let _ = write!(s, " delay={}", n.apply_delay_ms.load(Relaxed));
            match peek(&n.ring, |r| {
                (
                    r.seen,
                    r.overwritten,
                    r.term_changes,
                    r.leader_changes,
                    r.state_changes,
                )
            }) {
                Some((a, b, c, d, e)) => {
                    let _ = write!(s, " seen={a} ovw={b} term={c} ldr={d} st={e}");
                }
                None => s.push_str(" ring=busy"),
            }
            out.line(format_args!("{s}"));
        }
        // Remaining budget split across rings, newest samples first.
        for (k, n) in self.nodes.iter().enumerate() {
            let budget = OUT_MAX.saturating_sub(TAIL + out.s.len()) / (3 - k);
            let mut s = format!("[DIAG-3n] n{} ring", n.node);
            let full = peek(&n.ring, |r| {
                let mut full = true;
                for x in r.buf.iter().rev() {
                    let mut e = String::from(" ");
                    x.render(&mut e);
                    if s.len() + e.len() + 1 > budget {
                        full = false;
                        break;
                    }
                    s.push_str(&e);
                }
                full
            });
            match full {
                None => s.push_str(" busy"),
                Some(false) => out.trunc = true,
                Some(true) => {}
            }
            out.line(format_args!("{s}"));
        }
        let trunc = out.trunc as u8;
        let end = format!("[DIAG-3n] end bytes={} trunc={trunc}\n", out.s.len());
        out.s.push_str(&end);
        out.s.truncate(OUT_MAX);
        out.s
    }
    fn should_dump(&self, panicking: bool) -> bool {
        !self.dumped && (panicking || !self.finished)
    }
    /// Drop-time teardown: never awaits, blocks on a held lock or panics.
    pub fn on_drop(&mut self) {
        for t in &self.tasks {
            t.abort();
        }
        let stores: Vec<usize> = self
            .stores
            .iter()
            .flatten()
            .copied()
            .filter(|s| *s != 0)
            .collect();
        unregister_nonblocking(&stores);
        self.stores = [[0; 2]; 3];
        if self.should_dump(std::thread::panicking()) {
            self.dumped = true;
            let s = self.render();
            let _ = std::io::stderr().write_all(s.as_bytes());
        }
    }
}
impl Drop for Diag {
    fn drop(&mut self) {
        self.on_drop();
    }
}
fn peek<T, R>(m: &Mutex<T>, f: impl FnOnce(&T) -> R) -> Option<R> {
    match m.try_lock() {
        Ok(g) => Some(f(&g)),
        Err(TryLockError::Poisoned(p)) => Some(f(&p.into_inner())),
        Err(TryLockError::WouldBlock) => None,
    }
}
#[derive(Default)]
struct Out {
    s: String,
    trunc: bool,
}
impl Out {
    fn line(&mut self, f: std::fmt::Arguments<'_>) {
        let mut l = String::new();
        let _ = l.write_fmt(f);
        l.push('\n');
        if self.s.len() + l.len() > OUT_MAX - TAIL {
            self.trunc = true;
        } else {
            self.s.push_str(&l);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(t_ms: u32, term: u64, state: u8, leader: u64) -> Sample {
        Sample {
            t_ms,
            term,
            state,
            leader: Some(leader),
            last: Some(u64::MAX),
            committed: Some(u64::MAX),
            applied: Some(u64::MAX),
            ack_ms: Some(u64::MAX),
        }
    }
    // Finished fixtures do not dump on drop.
    fn diag() -> Diag {
        let mut d = Diag::new();
        d.finished = true;
        d
    }
    fn clean(s: &str) -> bool {
        s.is_ascii()
            && !s.contains(['/', '\\', '"'])
            && s.lines().all(|l| l.starts_with("[DIAG-3n] "))
    }

    #[test]
    fn buckets_use_fixed_edges_and_maximum() {
        let b = Buckets::default();
        for v in [0, 9, 10, 49, 50, 199, 200, 999, 1000, 70_000] {
            b.record(Duration::from_millis(v));
        }
        assert_eq!(b.counts(), [2, 2, 2, 2, 2]);
        assert_eq!(b.max_ms(), 70_000);
        assert_eq!(b.high(), 4);
    }

    #[test]
    fn ring_is_bounded_and_counts_changes_over_the_whole_run() {
        let mut r = Ring::default();
        for i in 0..300u32 {
            r.push(sample(
                i,
                (i / 100) as u64,
                if i % 100 == 0 { b'C' } else { b'L' },
                1,
            ));
        }
        assert_eq!(r.len(), RING);
        assert_eq!(r.seen, 300);
        assert_eq!(r.overwritten, 300 - RING as u32);
        assert_eq!(r.term_changes, 2);
        assert_eq!(r.state_changes, 5);
        assert_eq!(r.leader_changes, 0);
    }

    #[test]
    fn failure_dump_is_bounded_redacted_and_marks_truncation() {
        let d = diag();
        for n in &d.nodes {
            let mut r = n.ring.lock().unwrap();
            for i in 0..(RING as u32 + 7) {
                r.push(sample(u32::MAX - i, u64::MAX, b'F', u64::MAX));
            }
            for op in OPS {
                n.op(op).record(Duration::from_secs(u64::MAX / 4));
            }
        }
        let mut a = d.attribution(Helper::Converge, Fail::NotLeader, 1);
        let s = sample(1, 2, b'L', 2);
        a.read = Some((s, s, S3::SameTermCandidate));
        a.at = [Some(s); 3];
        d.record(a);
        let out = d.render();
        assert!(out.len() <= OUT_MAX, "{}", out.len());
        assert!(out.ends_with("trunc=1\n"));
        assert!(clean(&out), "{out}");
        assert!(out.contains("helper=Converge fail=NotLeader node=2"));
        assert!(out.contains("class=SameTermCandidate"));
        // Small runs fit completely.
        let small = diag();
        small.nodes[0]
            .ring
            .lock()
            .unwrap()
            .push(sample(5, 1, b'L', 1));
        let out = small.render();
        assert!(
            out.ends_with("trunc=0\n") && out.contains(" 5,1,L,1,"),
            "{out}"
        );
        assert!(clean(&small.summary()));
    }

    #[test]
    fn render_never_blocks_on_held_or_poisoned_locks() {
        let d = Arc::new(diag());
        let held = d.nodes[0].ring.lock().unwrap();
        let attr = d.first.lock().unwrap();
        let out = d.render();
        assert!(
            out.contains("n1 ring busy") && out.contains("attr busy"),
            "{out}"
        );
        assert!(d.summary().contains("ring=busy"));
        drop((held, attr));
        let p = d.clone();
        let _ = std::thread::spawn(move || {
            let _g = p.nodes[1].ring.lock().unwrap();
            panic!("poison ring");
        })
        .join();
        assert!(d.nodes[1].ring.is_poisoned());
        assert!(d.render().contains("n2 ops"));
    }

    #[test]
    fn dump_trigger_covers_panics_and_unfinished_fixtures_once() {
        let mut d = Diag::new();
        assert!(
            d.should_dump(false),
            "dropped before finish, e.g. outer timeout"
        );
        d.finished = true;
        assert!(!d.should_dump(false));
        assert!(d.should_dump(true));
        d.dumped = true;
        assert!(!d.should_dump(true));
    }

    #[test]
    fn registry_attributes_spans_only_to_registered_stores() {
        let d = diag();
        let (a, b) = (Box::new(0u8), Box::new(0u8));
        let (ka, kb) = (&*a as *const u8 as usize, &*b as *const u8 as usize);
        register(ka, &d.nodes[2]);
        drop(span(ka, Op::Apply));
        drop(span(kb, Op::Apply));
        started(ka, Instant::now(), false);
        assert_eq!(d.nodes[2].op(Op::Apply).counts().iter().sum::<u32>(), 1);
        assert_eq!(d.nodes[2].queue().counts().iter().sum::<u32>(), 1);
        unregister(&[ka]);
        drop(span(ka, Op::Apply));
        assert_eq!(d.nodes[2].op(Op::Apply).counts().iter().sum::<u32>(), 1);
        // A dead fixture's entry is inert.
        let gone = diag();
        register(kb, &gone.nodes[0]);
        drop(gone);
        assert!(span(kb, Op::Vote).is_none());
    }

    #[test]
    fn lag_window_and_candidates() {
        let l = Lag::default();
        l.record(Duration::from_millis(5), 10);
        l.record(Duration::from_millis(350), 20);
        l.record(Duration::from_millis(120), 30);
        assert_eq!((l.over.load(Relaxed), l.worst_at_ms.load(Relaxed)), (1, 20));
        assert_eq!(
            (l.max_since(0), l.max_since(21), l.max_since(31)),
            (350, 120, 0)
        );
        assert_eq!(classify(0, 0, 0), Candidate::None);
        assert_eq!(classify(350, 0, 10), Candidate::Cpu);
        assert_eq!(classify(0, 250, 10), Candidate::Cpu);
        assert_eq!(classify(10, 10, 250), Candidate::Store);
        assert_eq!(classify(300, 0, 250), Candidate::Both);
    }

    #[test]
    fn read_classification_needs_steady_snapshots_and_quiet_history() {
        let b = sample(10, 4, b'L', 2);
        assert_eq!(classify_read(b, b, false), S3::SameTermCandidate);
        assert_eq!(classify_read(b, b, true), S3::Churn);
        assert_eq!(classify_read(b, sample(11, 5, b'F', 3), false), S3::Churn);
        assert_eq!(classify_read(b, sample(11, 4, b'F', 2), false), S3::Churn);
        let d = diag();
        d.nodes[0].ring.lock().unwrap().push(sample(12, 5, b'L', 1));
        // Another node's retained higher term counts as churn for node 2's read.
        assert_eq!(d.read_class(1, b, b), S3::Churn);
        assert_eq!(
            d.read_class(0, sample(13, 5, b'L', 1), sample(13, 5, b'L', 1)),
            S3::SameTermCandidate
        );
    }
}

#[cfg(test)]
mod controls {
    //! Positive controls: each induces one seam and checks its own attribution.
    //! Opt-in (`--run-ignored`), so CI time and the Windows quarantine are unchanged.
    use super::super::three_node::{Cluster, report_cleanup, report_take_node, unknown};
    use super::super::*;
    use super::*;
    use std::time::Instant;
    use tokio::time::timeout;

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "D1 diagnostic positive control; run explicitly with --run-ignored"]
    async fn d1_control_s4_queued_apply_exceeds_shutdown_budget() {
        const DELAY_MS: u32 = 250;
        const QUEUED: u8 = 40;
        timeout(Duration::from_secs(45), async {
            let mut c = Cluster::new(Params {
                election_min: 3000,
                election_max: 6000,
                ..Params::default()
            })
            .await;
            c.converge().await;
            let l = c.leader().await;
            let stats = c.diag.nodes[l].clone();
            let high0 = stats.op(Op::Apply).high();
            let raft = c.node(l).raft.as_ref().unwrap().clone();
            let base = raft.metrics().borrow_watched().last_log_index.unwrap();
            stats.apply_delay_ms.store(DELAY_MS, Relaxed);
            let mut callers = JoinSet::new();
            for r in 0..QUEUED {
                let node = c.node(l).clone();
                callers.spawn(async move { node.propose(unknown(r)).await });
            }
            let m = raft
                .wait(Some(Duration::from_secs(5)))
                .metrics(
                    |m| m.local_committed.is_some_and(|p| p.index >= base + QUEUED as u64),
                    "control proposals committed",
                )
                .await
                .unwrap();
            let backlog = base + QUEUED as u64 - m.last_applied.map_or(0, |p| p.index);
            callers.abort_all();
            while callers.join_next().await.is_some() {}
            drop(raft);
            // The control must really exceed the unchanged 5 s budget, with margin.
            assert!(backlog * DELAY_MS as u64 >= 6000, "backlog={backlog}");
            let mut n = report_take_node(&mut c, l);
            let start = Instant::now();
            let result = timeout(Duration::from_millis(5500), n.shutdown())
                .await
                .unwrap();
            let elapsed = start.elapsed();
            // Let the real worker drain before any assertion can panic.
            stats.apply_delay_ms.store(0, Relaxed);
            report_cleanup(&n).await;
            let Err(Orphaned(r)) = result else {
                panic!("queued apply backlog must fail shutdown: {result:?}")
            };
            c.attribute_stop(l, r);
            let a = c.diag.recorded().unwrap();
            let high = stats.op(Op::Apply).high() - high0;
            let lag = c.diag.lag.max_since(0);
            let cand = classify(lag, 0, if high > 0 { HIGH_MS } else { 0 });
            eprintln!(
                "D1 S4 control: delay_ms={DELAY_MS} queued={QUEUED} backlog={backlog} apply_high={high} lag={lag} cand={cand:?} elapsed={elapsed:?} report={r:?}"
            );
            assert_eq!((a.helper, a.fail), (Helper::Stop, Fail::Orphaned));
            assert_eq!(r.first, Phase::Owners);
            assert!(r.owners_left >= 1, "{r:?}");
            assert!(high >= 1);
            assert!(matches!(cand, Candidate::Store | Candidate::Both));
            assert!(c.diag.render().contains("first: Owners"));
            c.finish().await;
        })
        .await
        .unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "D1 diagnostic positive control; run explicitly with --run-ignored"]
    async fn d1_control_s3_isolated_leader_read_attributes_churn() {
        timeout(Duration::from_secs(45), async {
            let mut c = Cluster::new(Params::default()).await;
            c.converge().await;
            let old = c.leader().await;
            let term = c.diag.sample(c.node(old).raft.as_ref().unwrap()).term;
            c.router.isolated.lock().unwrap().insert(old as u64 + 1);
            let new = timeout(Duration::from_secs(5), async {
                loop {
                    for i in (0..3).filter(|i| *i != old) {
                        let m = c.diag.sample(c.node(i).raft.as_ref().unwrap());
                        if m.state == b'L' && m.term > term {
                            return i;
                        }
                    }
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            })
            .await
            .expect("replacement leader");
            c.router.isolated.lock().unwrap().remove(&(old as u64 + 1));
            let result = c.attributed_read(old).await;
            let a = c.diag.recorded();
            eprintln!(
                "D1 S3 control: old={} new={} term={term} result={result:?} attr={a:?}",
                old + 1,
                new + 1
            );
            assert!(result.is_err(), "stale leader read must fail: {result:?}");
            let a = a.unwrap();
            assert_eq!((a.helper, a.node), (Helper::Converge, old as u8 + 1));
            assert_eq!(a.read.unwrap().2, S3::Churn);
            assert!(c.diag.render().contains("class=Churn"));
            // The deposed node may still publish Leader metrics; converge only after
            // it steps down so cleanup does not re-enter the seam under test.
            c.node(old)
                .raft
                .as_ref()
                .unwrap()
                .wait(Some(Duration::from_secs(5)))
                .metrics(
                    |m| m.current_term > term,
                    "deposed leader observes new term",
                )
                .await
                .unwrap();
            c.converge().await;
            c.finish().await;
        })
        .await
        .unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "D1 diagnostic positive control; run explicitly with --run-ignored"]
    async fn d1_control_timer_lag_is_distinct_from_store_latency() {
        timeout(Duration::from_secs(45), async {
            let mut c = Cluster::new(Params {
                election_min: 3000,
                election_max: 6000,
                ..Params::default()
            })
            .await;
            c.converge().await;
            let t = c.diag.t_ms();
            let high0: u32 = c.diag.nodes.iter().map(|n| n.high_total()).sum();
            // Occupy all four runtime workers at once; bounded rendezvous, no deadlock.
            let arrived = Arc::new(AtomicU32::new(0));
            let mut burn = JoinSet::new();
            for _ in 0..4 {
                let arrived = arrived.clone();
                burn.spawn(async move {
                    arrived.fetch_add(1, Relaxed);
                    let gate = Instant::now();
                    while arrived.load(Relaxed) < 4 && gate.elapsed() < Duration::from_secs(2) {
                        std::hint::spin_loop();
                    }
                    let s = Instant::now();
                    while s.elapsed() < Duration::from_millis(400) {
                        std::hint::spin_loop();
                    }
                });
            }
            while burn.join_next().await.is_some() {}
            tokio::time::sleep(Duration::from_millis(150)).await;
            let lag = c.diag.lag.max_since(t);
            let high: u32 = c.diag.nodes.iter().map(|n| n.high_total()).sum::<u32>() - high0;
            let cand = classify(lag, 0, if high > 0 { HIGH_MS } else { 0 });
            eprintln!("D1 lag control: burn_ms=400 workers=4 lag={lag} store_high_delta={high} cand={cand:?}");
            assert!(lag >= 250, "lag={lag}");
            assert_eq!(high, 0);
            assert_eq!(cand, Candidate::Cpu);
            c.finish().await;
        })
        .await
        .unwrap();
    }
}
