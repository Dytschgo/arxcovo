//! Corroborated console input for qualification and foreground admission.

use super::*;
use std::{
    sync::{Arc, Mutex, mpsc},
    thread,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};

const REFUSAL: NativeRefusal = NativeRefusal::OwnerActivityUnverified;
const IDLE_MS: u64 = (IDLE_TICKS / 10_000) as u64;
const LIVE_MS: u64 = (LIVE_TICKS / 10_000) as u64;
const POLL: Duration = Duration::from_millis(500);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum View {
    Admission,
    Transition,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Source {
    Wts,
    Local,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Input {
    pub(super) source: Source,
    pub(super) cursor: Option<u64>,
    pub(super) idle: bool,
}

impl Input {
    pub(super) fn wts(sample: WtsSample) -> Self {
        Self {
            source: Source::Wts,
            cursor: Some(sample.input as u64),
            idle: sample.current - sample.input >= IDLE_TICKS,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct LocalSample {
    session: u32,
    before: u64,
    after: u64,
    tick: u32,
}

impl LocalSample {
    pub(super) fn fresh_at(self, now: u64) -> bool {
        now >> 32 == self.after >> 32
            && now
                .checked_sub(self.after)
                .is_some_and(|age| age <= LIVE_MS)
    }

    fn cursor(self, owner_session: u32) -> Result<u64, NativeRefusal> {
        let era = self.after >> 32;
        if self.session != owner_session
            || self
                .after
                .checked_sub(self.before)
                .is_none_or(|gap| gap > LIVE_MS)
            || self.before >> 32 != era
            || self.tick == 0
            || self.tick > self.after as u32
        {
            return Err(REFUSAL);
        }
        Ok((era << 32) | u64::from(self.tick))
    }
}

pub(super) fn local_sample(owner_session: u32) -> Option<LocalSample> {
    let before = boot::uptime();
    let mut session = 0;
    if unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut session) } == 0
        || session != owner_session
        || unsafe { WTSGetActiveConsoleSessionId() } != session
    {
        return None;
    }
    let mut protocol = WtsBuffer(null_mut());
    let mut size = 0;
    // Read protocol from the calling session, whose input this API returns.
    if unsafe {
        WTSQuerySessionInformationW(
            null_mut(),
            WTS_CURRENT_SESSION,
            WTSClientProtocolType,
            &mut protocol.0,
            &mut size,
        )
    } == 0
        || protocol.0.is_null()
        || size as usize != size_of::<u16>()
        || unsafe { protocol.0.cast::<u16>().read_unaligned() } != 0
    {
        return None;
    }
    let mut input = LASTINPUTINFO {
        cbSize: size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    if unsafe { GetLastInputInfo(&mut input) } == 0
        || unsafe { WTSGetActiveConsoleSessionId() } != session
    {
        return None;
    }
    Some(LocalSample {
        session,
        before,
        after: boot::uptime(),
        tick: input.dwTime,
    })
}

#[derive(Default)]
struct History {
    last: Option<(u64, u64)>,
    since: Option<u64>,
}

impl History {
    fn observe(&mut self, sample: LocalSample, owner: u32) -> Result<Input, NativeRefusal> {
        let cursor = sample.cursor(owner)?;
        if let Some((previous, at)) = self.last.replace((cursor, sample.after)) {
            if cursor < previous || cursor >> 32 != previous >> 32 || sample.before < at {
                return Err(REFUSAL);
            }
            if self.since.is_some() && sample.after - at > LIVE_MS {
                return Err(REFUSAL);
            }
            if cursor != previous {
                self.since = None;
            }
        }
        // A reported tick is an input cursor. Its age grants no idle credit.
        let since = *self.since.get_or_insert(sample.after);
        Ok(Input {
            source: Source::Local,
            cursor: Some(cursor),
            idle: sample.after - since >= IDLE_MS,
        })
    }
}

struct Binding {
    source: Source,
    logon: i64,
    history: Mutex<History>,
}

impl Binding {
    fn input(
        &self,
        evidence: &ConsoleEvidence,
        view: View,
        history: &mut History,
    ) -> Result<Input, NativeRefusal> {
        let source = console_source(evidence, view)?;
        if source != self.source {
            return Err(REFUSAL);
        }
        if source == Source::Wts {
            return Ok(Input::wts(evidence.wts[1]));
        }
        if evidence.wts[0].logon != self.logon {
            return Err(REFUSAL);
        }
        if !evidence.owner_console_active() {
            history.since = None;
            return Ok(Input {
                source,
                cursor: None,
                idle: false,
            });
        }
        history.observe(evidence.local_input.ok_or(REFUSAL)?, evidence.token_session)
    }

    fn capture(
        &self,
        token: &VmToken,
        view: View,
    ) -> Result<(ConsoleEvidence, Input), NativeRefusal> {
        // Keep queries ordered with updates, including a lock or switch.
        let mut history = self.history.lock().map_err(|_| REFUSAL)?;
        let result = console_evidence(token, view).and_then(|evidence| {
            let input = self.input(&evidence, view, &mut history)?;
            Ok((evidence, input))
        });
        if result.is_err() {
            history.since = None;
        }
        result
    }
}

#[derive(Default)]
pub(super) struct Collector(Mutex<Option<Observer>>);

impl Collector {
    fn with_observer<T>(
        &self,
        token: &Arc<VmToken>,
        f: impl FnOnce(&Observer) -> Result<T, NativeRefusal>,
    ) -> Result<T, NativeRefusal> {
        let mut observer = self.0.lock().map_err(|_| REFUSAL)?;
        if observer.is_none() {
            *observer = Some(Observer::start(token)?);
        }
        f(observer.as_ref().ok_or(REFUSAL)?)
    }

    pub(super) fn capture(
        &self,
        token: &Arc<VmToken>,
        view: View,
    ) -> Result<(ConsoleEvidence, Input), NativeRefusal> {
        self.with_observer(token, |observer| observer.capture(token, view))
    }

    pub(super) fn baseline(&self, token: &Arc<VmToken>, wait: bool) -> Result<bool, NativeRefusal> {
        self.with_observer(token, |observer| observer.baseline(token, wait))
    }
}

struct Observer {
    binding: Arc<Binding>,
    // Dropping the sender stops the worker. It holds no launch authority.
    _stop: Option<mpsc::Sender<()>>,
}

impl Observer {
    fn start(token: &Arc<VmToken>) -> Result<Self, NativeRefusal> {
        let evidence = console_evidence(token, View::Admission)?;
        let source = console_source(&evidence, View::Admission)?;
        let mut history = History::default();
        if source == Source::Local {
            history.observe(evidence.local_input.ok_or(REFUSAL)?, evidence.token_session)?;
        }
        let binding = Arc::new(Binding {
            source,
            logon: evidence.wts[0].logon,
            history: Mutex::new(history),
        });
        let stop = if source == Source::Local {
            let (sender, receiver) = mpsc::channel();
            let binding = binding.clone();
            let token = token.clone();
            thread::Builder::new()
                .name("arx-owner-input".into())
                .spawn(move || {
                    while let Err(mpsc::RecvTimeoutError::Timeout) = receiver.recv_timeout(POLL) {
                        let _ = binding.capture(&token, View::Admission);
                    }
                })
                .map_err(|_| REFUSAL)?;
            Some(sender)
        } else {
            None
        };
        Ok(Self {
            binding,
            _stop: stop,
        })
    }

    fn capture(
        &self,
        token: &VmToken,
        view: View,
    ) -> Result<(ConsoleEvidence, Input), NativeRefusal> {
        self.binding.capture(token, view)
    }

    /// A one-shot qualifier can wait at most one idle window plus capture allowance.
    fn baseline(&self, token: &VmToken, wait: bool) -> Result<bool, NativeRefusal> {
        let deadline = Instant::now() + Duration::from_millis(IDLE_MS + LIVE_MS);
        loop {
            let (_, input) = self.capture(token, View::Admission)?;
            if input.idle || !wait || input.source == Source::Wts || Instant::now() >= deadline {
                return Ok(input.idle);
            }
            thread::sleep(POLL);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(ms: u64, input: u32) -> LocalSample {
        LocalSample {
            session: 2,
            before: ms,
            after: ms,
            tick: input,
        }
    }

    fn zero_wts() -> ConsoleEvidence {
        let mut evidence = super::super::tests::console();
        for wts in &mut evidence.wts {
            wts.input = 0;
        }
        evidence
    }

    fn binding(evidence: &ConsoleEvidence) -> Binding {
        Binding {
            source: Source::Local,
            logon: evidence.wts[0].logon,
            history: Mutex::default(),
        }
    }

    #[test]
    fn zero_wts_owner_input_requires_a_full_observed_idle_window() {
        let mut evidence = zero_wts();
        let binding = binding(&evidence);
        let mut history = History::default();
        assert_eq!(
            binding.input(&evidence, View::Admission, &mut history),
            Err(REFUSAL)
        );
        let start = 800_000;
        // An input cursor older than ten minutes grants no credit at startup.
        for offset in (0..IDLE_MS).step_by(1_000).chain([IDLE_MS - 1, IDLE_MS]) {
            evidence.local_input = Some(sample(start + offset, 100));
            assert_eq!(
                binding
                    .input(&evidence, View::Admission, &mut history)
                    .unwrap()
                    .idle,
                offset == IDLE_MS,
                "observed interval {offset} ms"
            );
        }
        evidence.local_input = Some(sample(start + IDLE_MS + 1, 101));
        assert!(
            !binding
                .input(&evidence, View::Admission, &mut history)
                .unwrap()
                .idle
        );
    }

    type Mutation = (&'static str, fn(&mut ConsoleEvidence));
    #[test]
    fn zero_wts_owner_input_refuses_ambiguous_or_invalid_evidence() {
        let cases: [Mutation; 10] = [
            ("wrong process session", |e| {
                e.local_input.as_mut().unwrap().session += 1
            }),
            ("mixed inputs", |e| e.wts[1].input = e.wts[1].logon),
            ("non-owner", |e| e.lsa[0].owner = false),
            ("remote logon", |e| e.lsa[0].logon_type = 10),
            ("unstable token", |e| e.token_stable = false),
            ("account ambiguous", |e| e.accounts[1] = None),
            ("stale WTS", |e| e.now += LIVE_TICKS + 1),
            ("stale local capture", |e| {
                e.local_input.as_mut().unwrap().after += LIVE_MS + 1
            }),
            ("future input", |e| {
                e.local_input.as_mut().unwrap().tick = 1_001
            }),
            ("changed logon", |e| {
                for wts in &mut e.wts {
                    wts.logon += 1;
                }
            }),
        ];
        for (name, mutate) in cases {
            let mut evidence = zero_wts();
            let binding = binding(&evidence);
            evidence.local_input = Some(sample(1_000, 900));
            mutate(&mut evidence);
            assert_eq!(
                binding.input(&evidence, View::Admission, &mut History::default()),
                Err(REFUSAL),
                "{name}"
            );
        }
    }

    #[test]
    fn local_input_refuses_tick_regressions_wrap_and_observation_gaps() {
        let era = 1_u64 << 32;
        let cases = [
            (
                "input goes backwards",
                sample(1_000, 900),
                sample(1_001, 899),
            ),
            ("clock goes backwards", sample(1_000, 900), sample(999, 900)),
            (
                "clock era changes",
                sample(era - 1, u32::MAX - 5),
                sample(era + 1, 1),
            ),
            (
                "wrap during the query",
                sample(era - 1, u32::MAX - 5),
                LocalSample {
                    before: era - 1,
                    ..sample(era + 1, 1)
                },
            ),
            (
                "gap exceeds freshness",
                sample(1_000, 900),
                sample(1_000 + LIVE_MS + 1, 900),
            ),
        ];
        for (name, first, next) in cases {
            let mut history = History::default();
            assert!(!history.observe(first, 2).unwrap().idle);
            assert_eq!(history.observe(next, 2), Err(REFUSAL), "{name}");
        }
    }

    #[test]
    fn local_input_freshness_rejects_rollover_after_the_query() {
        let era = 1_u64 << 32;
        let before_wrap = sample(era - 1, u32::MAX - 5);
        assert!(before_wrap.fresh_at(era - 1));
        assert!(!before_wrap.fresh_at(era));
        assert!(!before_wrap.fresh_at(era - 2));

        let after_wrap = sample(era + 5, 1);
        assert!(after_wrap.fresh_at(era + 5 + LIVE_MS));
        assert!(!after_wrap.fresh_at(era + 6 + LIVE_MS));
    }

    #[test]
    fn transition_capture_clears_idle_while_the_verified_owner_is_away() {
        let mut evidence = zero_wts();
        let binding = binding(&evidence);
        let mut history = History::default();
        evidence.local_input = Some(sample(1_000, 900));
        assert!(
            !binding
                .input(&evidence, View::Admission, &mut history)
                .unwrap()
                .idle
        );
        evidence.active_console = 3;
        evidence.local_input = None;
        assert_eq!(
            binding.input(&evidence, View::Admission, &mut history),
            Err(REFUSAL)
        );
        assert_eq!(
            binding
                .input(&evidence, View::Transition, &mut history)
                .unwrap()
                .cursor,
            None
        );
        assert_eq!(history.since, None);
        evidence.active_console = evidence.token_session;
        evidence.local_input = Some(sample(1_000 + IDLE_MS, 901));
        assert!(
            !binding
                .input(&evidence, View::Transition, &mut history)
                .unwrap()
                .idle
        );
    }
}
