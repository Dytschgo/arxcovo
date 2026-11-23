//! Session-locked tickets, admissions and outbound frame commits.
use super::{ApiError, Data, Key, Session, Sessions, cookie_key, random_token};
use crate::{development::Cancellation, vm::Clock};
use axum::extract::ws::Message;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
    time::{Duration, Instant},
};
use subtle::ConstantTimeEq;
use tokio::sync::{Notify, watch};

const TICKET_TTL: Duration = Duration::from_secs(30);
const CLOSE_TIMEOUT: Duration = Duration::from_secs(5);
const QUEUE_LIMIT: usize = 8;
const TERMINAL_IDLE: Duration = Duration::from_secs(15 * 60);
const TERMINAL_LIFETIME: Duration = Duration::from_secs(4 * 60 * 60);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::dashboard) enum Target {
    Terminal(String),
    Console { vm: [u8; 16], generation: u64 },
}

pub(super) struct Ticket {
    digest: Key,
    target: Target,
    origin: String,
    generation: u64,
    expires: Instant,
}

pub(in crate::dashboard) struct Bucket {
    capacity: u16,
    rate: u16,
    credit: Duration,
    last: Option<Instant>,
}

impl Bucket {
    pub(in crate::dashboard) const fn new(capacity: u16, rate: u16) -> Self {
        Self {
            capacity,
            rate,
            credit: Duration::from_secs(capacity as u64),
            last: None,
        }
    }

    pub(in crate::dashboard) fn take(&mut self, now: Instant) -> bool {
        if let Some(last) = self.last {
            self.credit = self
                .credit
                .saturating_add(now.saturating_duration_since(last) * u32::from(self.rate))
                .min(Duration::from_secs(self.capacity.into()));
        }
        self.last = Some(now);
        if self.credit < Duration::from_secs(1) {
            return false;
        }
        self.credit -= Duration::from_secs(1);
        true
    }
}

#[derive(Clone, Copy)]
pub(in crate::dashboard) struct Close {
    pub code: u16,
    pub reason: &'static str,
}

impl Close {
    pub const SESSION: Self = Self {
        code: 4401,
        reason: "session ended",
    };
    pub const TARGET: Self = Self {
        code: 4403,
        reason: "target unavailable",
    };
    pub const TICKET: Self = Self {
        code: 4408,
        reason: "ticket rejected",
    };
    pub const STOP: Self = Self {
        code: 1001,
        reason: "server shutting down",
    };
    pub const BACKPRESSURE: Self = Self {
        code: 1008,
        reason: "stream backpressure",
    };

    /// The one close-precedence order shared by terminal and console streams:
    ///
    /// 1. session revoked or expired (4401)
    /// 2. server shutdown (1001)
    /// 3. target unavailable or replaced (4403)
    /// 4. every ordinary close: VM stopped or capture fault, terminal expiry,
    ///    audit failure, protocol, size, rate, capacity and backpressure.
    ///
    /// A later close replaces a pending one only when it ranks strictly
    /// higher and the writer has not yet taken the pending closing notice or
    /// close frame. A session end replaces any pending close regardless,
    /// because nothing may be delivered after revocation. Equal or lower
    /// ranks never replace.
    fn rank(self) -> u8 {
        match self.code {
            4401 => 3,
            1001 => 2,
            4403 => 1,
            _ => 0,
        }
    }
}

pub(super) struct Registry {
    slots: HashMap<u64, Slot>,
    captures: HashSet<[u8; 16]>,
    next: u64,
    stopped: bool,
    mint_rate: Bucket,
    drained: Arc<Notify>,
}

impl Default for Registry {
    fn default() -> Self {
        Self {
            slots: HashMap::new(),
            captures: HashSet::new(),
            next: 0,
            stopped: false,
            mint_rate: Bucket::new(8, 4),
            drained: Arc::new(Notify::new()),
        }
    }
}

struct Slot {
    key: Key,
    generation: u64,
    target: Option<Target>,
    revision: watch::Sender<()>,
    close: Option<(Close, Instant)>,
    /// The writer has taken the closing message or close frame.
    close_committed: bool,
    closing_message: Option<Message>,
    queue: VecDeque<Message>,
    control: Option<Message>,
    in_flight: bool,
    command: Option<Cancellation>,
    terminal: Option<TerminalLimit>,
}

struct TerminalLimit {
    absolute: Instant,
    idle: Instant,
}

impl Slot {
    fn terminal_deadline(&self) -> Option<Instant> {
        let limit = self.terminal.as_ref()?;
        Some(if self.command.is_some() {
            limit.absolute
        } else {
            limit.absolute.min(limit.idle)
        })
    }

    fn expire_terminal(&mut self, now: Instant) {
        if self
            .terminal_deadline()
            .is_some_and(|deadline| deadline <= now)
        {
            self.close(
                Close {
                    code: 1000,
                    reason: "terminal expired",
                },
                now,
                None,
            );
        }
    }

    fn close(&mut self, close: Close, now: Instant, message: Option<Message>) {
        if let Some((current, _)) = self.close {
            // See `Close::rank` for the shared precedence order.
            let replaces = close.rank() > current.rank()
                && (close.code == Close::SESSION.code || !self.close_committed);
            if !replaces {
                return;
            }
        }
        let deadline = self
            .close
            .map_or(now + CLOSE_TIMEOUT, |(_, deadline)| deadline);
        self.close = Some((close, deadline));
        self.closing_message = message;
        self.queue.clear();
        self.control = None;
        if let Some(command) = &self.command {
            command.cancel();
        }
        self.revision.send_replace(());
    }
}

impl Registry {
    fn notify_drained(&self) {
        if self.stopped && self.slots.is_empty() && self.captures.is_empty() {
            self.drained.notify_one();
        }
    }

    pub(super) fn wake(&self, key: Key) {
        for slot in self.slots.values().filter(|slot| slot.key == key) {
            slot.revision.send_replace(());
        }
    }
}

impl Data {
    pub(super) fn live(&mut self, key: Key, now: Instant) -> Result<&mut Session, ApiError> {
        if self
            .sessions
            .get(&key)
            .is_some_and(|s| s.remaining(now).is_zero())
        {
            self.revoke(key, now);
        }
        self.sessions.get_mut(&key).ok_or(ApiError::UNAUTHORIZED)
    }

    pub(super) fn revoke(&mut self, key: Key, now: Instant) {
        if self.sessions.remove(&key).is_some() {
            // Removing this generation is the revocation point. New sessions
            // can only receive a fresh generation from the registry counter.
            for slot in self
                .streams
                .slots
                .values_mut()
                .filter(|slot| slot.key == key)
            {
                slot.close(Close::SESSION, now, None);
            }
        }
    }

    pub(super) fn expire(&mut self, now: Instant) {
        let expired: Vec<_> = self
            .sessions
            .iter()
            .filter_map(|(key, session)| session.remaining(now).is_zero().then_some(*key))
            .collect();
        for key in expired {
            self.revoke(key, now);
        }
    }
}

impl Sessions {
    pub(in crate::dashboard) fn mint(
        &self,
        key: Option<Key>,
        csrf: Option<&str>,
        origin: &str,
        target: Target,
        clock: &dyn Clock,
    ) -> Result<(String, String), ApiError> {
        let key = key.ok_or(ApiError::UNAUTHORIZED)?;
        let id = random_token().map_err(|_| ApiError::UNAVAILABLE)?;
        let token = random_token().map_err(|_| ApiError::UNAVAILABLE)?;
        let mut data = self.active.lock().map_err(|_| ApiError::UNAVAILABLE)?;
        let now = clock.now().monotonic;
        let session = data.live(key, now)?;
        if !csrf.is_some_and(|csrf| bool::from(csrf.as_bytes().ct_eq(session.csrf.as_bytes()))) {
            return Err(ApiError::FORBIDDEN);
        }
        if data.streams.stopped {
            return Err(ApiError::UNAVAILABLE);
        }
        let wait = || ApiError::rate_limit(Duration::from_secs(1));
        if !data.streams.mint_rate.take(now) {
            return Err(wait());
        }
        let mut total = 0;
        for session in data.sessions.values_mut() {
            session.tickets.retain(|_, ticket| ticket.expires > now);
            total += session.tickets.len();
        }
        let session = data.live(key, now)?;
        if !session.mint_rate.take(now) || total >= 32 || session.tickets.len() >= 8 {
            return Err(wait());
        }
        if session.tickets.contains_key(&id) {
            return Err(ApiError::UNAVAILABLE);
        }
        session.tickets.insert(
            id.clone(),
            Ticket {
                digest: cookie_key(&token).expect("generated ticket"),
                target,
                origin: origin.to_owned(),
                generation: session.generation,
                expires: now + TICKET_TTL,
            },
        );
        Ok((id, token))
    }

    pub(in crate::dashboard) fn register_stream(
        self: &Arc<Self>,
        key: Option<Key>,
        clock: Arc<dyn Clock>,
    ) -> Result<Connection, ApiError> {
        let key = key.ok_or(ApiError::UNAUTHORIZED)?;
        let mut data = self.active.lock().map_err(|_| ApiError::UNAVAILABLE)?;
        let generation = data.live(key, clock.now().monotonic)?.generation;
        let registry = &mut data.streams;
        if registry.stopped || registry.slots.len() >= 8 {
            return Err(ApiError::UNAVAILABLE);
        }
        let id = registry.next;
        registry.next = id.checked_add(1).ok_or(ApiError::UNAVAILABLE)?;
        let (revision, _) = watch::channel(());
        registry.slots.insert(
            id,
            Slot {
                key,
                generation,
                revision,
                target: None,
                close: None,
                close_committed: false,
                closing_message: None,
                queue: VecDeque::new(),
                control: None,
                in_flight: false,
                command: None,
                terminal: None,
            },
        );
        Ok(Connection {
            id,
            sessions: self.clone(),
            clock,
        })
    }

    pub(in crate::dashboard) async fn stop_streams(&self, clock: &dyn Clock) -> anyhow::Result<()> {
        let drained = {
            let mut data = self
                .active
                .lock()
                .map_err(|_| anyhow::anyhow!("stream state unavailable"))?;
            let now = clock.now().monotonic;
            for session in data.sessions.values_mut() {
                session.tickets.clear();
            }
            data.streams.stopped = true;
            for slot in data.streams.slots.values_mut() {
                slot.close(Close::STOP, now, None);
            }
            if data.streams.slots.is_empty() && data.streams.captures.is_empty() {
                return Ok(());
            }
            data.streams.drained.clone()
        };
        tokio::time::timeout(CLOSE_TIMEOUT + Duration::from_secs(1), drained.notified())
            .await
            .map_err(|_| anyhow::anyhow!("dashboard stream drain unconfirmed"))
    }
}

pub(in crate::dashboard) struct Connection {
    id: u64,
    sessions: Arc<Sessions>,
    clock: Arc<dyn Clock>,
}

impl Drop for Connection {
    fn drop(&mut self) {
        if let Ok(mut data) = self.sessions.active.lock() {
            let registry = &mut data.streams;
            if let Some(slot) = registry.slots.remove(&self.id)
                && let Some(command) = slot.command
            {
                command.cancel();
            }
            registry.notify_drained();
        }
    }
}

/// A worker can address its socket without keeping the socket registered.
#[derive(Clone)]
pub(in crate::dashboard) struct Stream {
    id: u64,
    sessions: Arc<Sessions>,
    clock: Arc<dyn Clock>,
}

pub(in crate::dashboard) enum Write {
    Message(Message, Instant),
    Close(Close, Instant),
    Wait,
}

impl Connection {
    pub fn stream(&self) -> Stream {
        Stream {
            id: self.id,
            sessions: self.sessions.clone(),
            clock: self.clock.clone(),
        }
    }

    pub fn redeem(&self, id: &str, token: &str, origin: &str) -> Result<Target, Close> {
        let digest = cookie_key(token);
        let mut data = self.sessions.active.lock().map_err(|_| Close::SESSION)?;
        let now = self.clock.now().monotonic;
        let slot = data.streams.slots.get(&self.id).ok_or(Close::SESSION)?;
        let (key, generation) = (slot.key, slot.generation);
        if let Some((close, _)) = slot.close {
            return Err(close);
        }
        let session = data.live(key, now).map_err(|_| Close::SESSION)?;
        if session.generation != generation {
            return Err(Close::SESSION);
        }
        let ticket = session
            .tickets
            .get(id)
            .filter(|ticket| {
                ticket.generation == generation
                    && ticket.origin == origin
                    && ticket.expires > now
                    && digest.is_some_and(|digest| bool::from(digest.ct_eq(&ticket.digest)))
            })
            .ok_or(Close::TICKET)?;
        let target = ticket.target.clone();
        session.tickets.remove(id);
        let slots = &mut data.streams.slots;
        let peers = slots.values().filter(|slot| slot.close.is_none());
        let available = match &target {
            Target::Terminal(_) => !peers
                .clone()
                .any(|slot| slot.target.as_ref() == Some(&target)),
            Target::Console { vm, .. } => {
                let mut same_vm = 0;
                let mut total = 0;
                for slot in peers {
                    if let Some(Target::Console { vm: viewer, .. }) = &slot.target {
                        total += 1;
                        same_vm += u8::from(viewer == vm);
                    }
                }
                same_vm < 2 && total < 4
            }
        };
        if !available {
            return Err(Close {
                code: 1013,
                reason: "stream capacity reached",
            });
        }
        let slot = slots.get_mut(&self.id).expect("registered socket");
        if matches!(target, Target::Terminal(_)) {
            slot.terminal = Some(TerminalLimit {
                absolute: now + TERMINAL_LIFETIME,
                idle: now + TERMINAL_IDLE,
            });
        }
        slot.target = Some(target.clone());
        slot.revision.send_replace(());
        Ok(target)
    }
}

impl Stream {
    fn with_slot<T>(&self, f: impl FnOnce(&mut Slot, Instant, bool) -> T) -> Option<T> {
        let mut data = self.sessions.active.lock().ok()?;
        let now = self.clock.now().monotonic;
        let slot = data.streams.slots.get(&self.id)?;
        let (key, generation) = (slot.key, slot.generation);
        let live = data
            .live(key, now)
            .is_ok_and(|session| session.generation == generation);
        let slot = data.streams.slots.get_mut(&self.id)?;
        if !live
            && slot
                .close
                .is_none_or(|(close, _)| close.code != Close::SESSION.code)
        {
            slot.close(Close::SESSION, now, None);
        }
        slot.expire_terminal(now);
        Some(f(slot, now, live))
    }

    pub fn revision(&self) -> Option<watch::Receiver<()>> {
        self.with_slot(|slot, _, _| slot.revision.subscribe())
    }

    pub fn close(&self, close: Close, message: Option<Message>) {
        self.with_slot(|slot, now, _| slot.close(close, now, message));
    }

    pub fn closing(&self) -> bool {
        self.with_slot(|slot, _, _| slot.close.is_some())
            .unwrap_or(true)
    }

    pub fn deadline(&self) -> Option<Instant> {
        let mut data = self.sessions.active.lock().ok()?;
        let now = self.clock.now().monotonic;
        let slot = data.streams.slots.get(&self.id)?;
        if slot.close.is_some() {
            return None;
        }
        let key = slot.key;
        let session_deadline = now + data.live(key, now).ok()?.remaining(now);
        let slot = data.streams.slots.get_mut(&self.id)?;
        slot.expire_terminal(now);
        if slot.close.is_some() {
            return None;
        }
        Some(
            slot.terminal_deadline()
                .map_or(session_deadline, |terminal| terminal.min(session_deadline)),
        )
    }

    fn enqueue(
        &self,
        messages: impl IntoIterator<Item = Message, IntoIter: ExactSizeIterator>,
        done: bool,
    ) {
        let messages = messages.into_iter();
        self.with_slot(|slot, now, live| {
            if done {
                slot.command = None;
                if let Some(limit) = &mut slot.terminal {
                    limit.idle = now + TERMINAL_IDLE;
                }
            }
            if live && slot.close.is_none() {
                if slot.queue.len() + messages.len() > QUEUE_LIMIT {
                    slot.close(Close::BACKPRESSURE, now, None);
                } else {
                    slot.queue.extend(messages);
                    slot.revision.send_replace(());
                }
            }
        });
    }

    pub fn queue(&self, messages: impl IntoIterator<Item = Message, IntoIter: ExactSizeIterator>) {
        self.enqueue(messages, false);
    }

    pub fn complete(
        &self,
        messages: impl IntoIterator<Item = Message, IntoIter: ExactSizeIterator>,
    ) {
        self.enqueue(messages, true);
    }

    pub fn control(&self, message: Message) {
        self.with_slot(|slot, _, live| {
            if live && slot.close.is_none() {
                slot.control = Some(message);
                slot.revision.send_replace(());
            }
        });
    }

    pub fn console_space(&self) -> bool {
        self.with_slot(|slot, _, live| {
            live && slot.close.is_none() && slot.queue.is_empty() && !slot.in_flight
        })
        .unwrap_or(false)
    }

    pub fn queue_console(&self, pair: [Message; 2]) {
        self.with_slot(|slot, _, live| {
            if live && slot.close.is_none() && slot.queue.is_empty() && !slot.in_flight {
                slot.queue.extend(pair);
                slot.revision.send_replace(());
            }
        });
    }

    pub fn commit(&self) -> Option<Write> {
        self.with_slot(|slot, now, live| {
            slot.in_flight = false;
            if let Some((close, deadline)) = slot.close {
                slot.close_committed = true;
                if live && let Some(message) = slot.closing_message.take() {
                    return Write::Message(message, deadline);
                }
                return Write::Close(close, deadline);
            }
            match slot.queue.pop_front().or_else(|| slot.control.take()) {
                Some(message) => {
                    slot.in_flight = true;
                    Write::Message(message, now + CLOSE_TIMEOUT)
                }
                None => Write::Wait,
            }
        })
    }

    pub fn admit_terminal<T>(
        &self,
        try_admit: impl FnOnce() -> Option<T>,
    ) -> Option<(T, Option<Cancellation>)> {
        self.with_slot(|slot, _, live| {
            if !live || slot.close.is_some() || !matches!(slot.target, Some(Target::Terminal(_))) {
                return None;
            }
            let admission = try_admit()?;
            if slot.command.is_some() {
                return Some((admission, None));
            }
            let cancel = Cancellation::default();
            slot.command = Some(cancel.clone());
            slot.revision.send_replace(());
            Some((admission, Some(cancel)))
        })
        .flatten()
    }

    pub fn cancel(&self) {
        self.with_slot(|slot, _, _| {
            if let Some(command) = &slot.command {
                command.cancel();
            }
        });
    }

    pub fn claim_completion(&self, cancel: &Cancellation) -> bool {
        self.with_slot(|slot, _, live| {
            let interrupted = !live || slot.close.is_some() || cancel.is_cancelled();
            // Retain the slot until audit and all result frames are committed
            // to the queue, but exclude late cancellation from this outcome.
            interrupted
        })
        .unwrap_or(true)
    }
}

pub(in crate::dashboard) struct Capture {
    sessions: Arc<Sessions>,
    clock: Arc<dyn Clock>,
    target: Target,
    vm: [u8; 16],
}

impl Sessions {
    pub(in crate::dashboard) fn capture(
        self: &Arc<Self>,
        target: Target,
        clock: Arc<dyn Clock>,
    ) -> Option<Capture> {
        let Target::Console { vm, .. } = target else {
            return None;
        };
        let mut data = self.active.lock().ok()?;
        let registry = &mut data.streams;
        if registry.stopped || registry.captures.len() >= 4 || !registry.captures.insert(vm) {
            return None;
        }
        Some(Capture {
            sessions: self.clone(),
            target,
            clock,
            vm,
        })
    }
}

impl Capture {
    /// Open viewers of this generation, which may receive frames.
    pub fn viewers(&self) -> Vec<Stream> {
        self.select(true)
    }

    /// Every still-registered viewer of this generation, including those
    /// with a pending close. A capture-ending close goes to all of them so
    /// the shared precedence in `Slot::close` arbitrates against a close
    /// the writer has not yet taken.
    pub fn registered(&self) -> Vec<Stream> {
        self.select(false)
    }

    fn select(&self, open_only: bool) -> Vec<Stream> {
        let Ok(mut data) = self.sessions.active.lock() else {
            return Vec::new();
        };
        data.expire(self.clock.now().monotonic);
        data.streams
            .slots
            .iter()
            .filter(|(_, slot)| {
                (!open_only || slot.close.is_none()) && slot.target.as_ref() == Some(&self.target)
            })
            .map(|(&id, _)| Stream {
                id,
                sessions: self.sessions.clone(),
                clock: self.clock.clone(),
            })
            .collect()
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        if let Ok(mut data) = self.sessions.active.lock() {
            data.streams.captures.remove(&self.vm);
            data.streams.notify_drained();
        }
    }
}
