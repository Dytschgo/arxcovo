//! Library-only SCM lifecycle. Poll deadlines cannot cancel synchronous RPCs.
#![allow(dead_code)] // PR-D wiring is deliberately absent.
#[cfg(windows)]
use super::owner_scm_windows::NativeScm;
use super::{
    layout::{Error, Layout, Platform},
    owner_plan::{Registration, registration},
};
use std::time::{Duration, Instant};
pub(super) const TYPE: u32 = 0x10;
pub(super) const START_AUTO: u32 = 2;
pub(super) const START_DEMAND: u32 = 3;
pub(super) const STOPPED: u32 = 1;
pub(super) const START_PENDING: u32 = 2;
pub(super) const STOP_PENDING: u32 = 3;
pub(super) const RUNNING: u32 = 4;
pub(super) const CONTINUE_PENDING: u32 = 5;
pub(super) const PAUSE_PENDING: u32 = 6;
pub(super) const PAUSED: u32 = 7;
pub(super) const ALREADY_RUNNING: u32 = 1056;
pub(super) const ABSENT: u32 = 1060;
pub(super) const CANNOT_ACCEPT: u32 = 1061;
pub(super) const NOT_ACTIVE: u32 = 1062;
pub(super) const SPECIFIC_ERROR: u32 = 1066;
pub(super) const MARKED: u32 = 1072;
pub(super) const EXISTS: u32 = 1073;
pub(super) const NEVER_STARTED: u32 = 1077;
pub(super) const DUPLICATE: u32 = 1078;
const STOP_DEADLINE: Duration = Duration::from_secs(30);
const START_DEADLINE: Duration = Duration::from_secs(35);
const POLL: Duration = Duration::from_millis(250);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    NotInstalled,
    Stopped,
    Starting,
    Running,
    Stopping,
    Failed,
}
impl Status {
    pub fn token(self) -> &'static str {
        match self {
            Self::NotInstalled => "not-installed",
            Self::Stopped => "stopped",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Stopping => "stopping",
            Self::Failed => "failed",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Boot {
    Demand,
    Auto,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Observation {
    Absent,
    Ours {
        boot: Boot,
        installed: bool,
        active: bool,
    },
    Foreign,
}
#[must_use]
pub struct Deregistered(());
pub(super) struct Win32(pub u32);
pub(super) enum Fault {
    Win32(u32),
    Malformed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Access {
    Status,
    Start,
    Stop,
    Install,
    Uninstall,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Level {
    Delayed,
    Sid,
    Privileges,
    Preshutdown,
    Failure,
    NonCrash,
}
const LEVELS: [Level; 6] = [
    Level::Delayed,
    Level::Sid,
    Level::Privileges,
    Level::Preshutdown,
    Level::Failure,
    Level::NonCrash,
];
#[derive(Clone, PartialEq, Eq)]
pub(super) enum Config2 {
    Delayed(u32),
    Sid(u32),
    Privileges(Vec<Vec<u16>>),
    Preshutdown(u32),
    Failure {
        reset: u32,
        reboot: Vec<u16>,
        command: Vec<u16>,
        actions: Vec<(u32, u32)>,
    },
    NonCrash(u32),
}
impl Config2 {
    pub(super) fn level(&self) -> Level {
        match self {
            Self::Delayed(_) => Level::Delayed,
            Self::Sid(_) => Level::Sid,
            Self::Privileges(_) => Level::Privileges,
            Self::Preshutdown(_) => Level::Preshutdown,
            Self::Failure { .. } => Level::Failure,
            Self::NonCrash(_) => Level::NonCrash,
        }
    }
}
#[derive(Clone)]
pub(super) struct BaseConfig {
    pub service_type: u32,
    pub start_type: u32,
    pub image: Vec<u16>,
    pub account: Vec<u16>,
}
#[derive(Clone, Copy)]
pub(super) struct Snapshot {
    pub state: u32,
    pub win32_exit: u32,
    pub specific_exit: u32,
}
pub(super) trait ScmCalls {
    fn open(&mut self, a: Access) -> Result<bool, Win32>;
    fn create(&mut self, cmdline: &[u16]) -> Result<(), Win32>;
    fn config(&mut self) -> Result<BaseConfig, Fault>;
    fn config2(&mut self, level: Level) -> Result<Config2, Fault>;
    fn state(&mut self) -> Result<Snapshot, Fault>;
    fn set_start(&mut self, b: Boot) -> Result<(), Win32>;
    fn set_config2(&mut self, v: &Config2) -> Result<(), Win32>;
    fn start(&mut self) -> Result<(), Win32>;
    fn control_stop(&mut self) -> Result<(), Win32>;
    fn delete(&mut self) -> Result<(), Win32>;
}
pub(super) trait Clock {
    fn now(&self) -> Duration;
    fn sleep(&mut self, d: Duration);
}
pub(super) struct MonotonicClock(Instant);
impl Clock for MonotonicClock {
    fn now(&self) -> Duration {
        self.0.elapsed()
    }
    fn sleep(&mut self, d: Duration) {
        std::thread::sleep(d);
    }
}
// Visibility follows the private native seam; PR-D can export a concrete facade.
#[cfg(windows)]
pub(super) struct WindowsScm<C: ScmCalls = NativeScm, K: Clock = MonotonicClock> {
    calls: C,
    clock: K,
    cmdline: Vec<u16>,
    expected: Vec<Config2>,
}
#[cfg(not(windows))]
pub(super) struct WindowsScm<C: ScmCalls, K: Clock> {
    calls: C,
    clock: K,
    cmdline: Vec<u16>,
    expected: Vec<Config2>,
}
fn command_line(r: &Registration) -> Result<Vec<u16>, Error> {
    #[cfg(windows)]
    let image: Vec<u16> = {
        use std::os::windows::ffi::OsStrExt;
        r.image.as_os_str().encode_wide().collect()
    };
    #[cfg(not(windows))]
    let image: Vec<u16> = r
        .image
        .to_str()
        .ok_or(Error::Layout)?
        .encode_utf16()
        .collect();
    // Windows absolute drive paths, including in portable fake tests.
    if image.len() < 3
        || !matches!(image[0], 65..=90 | 97..=122)
        || image[1] != 58
        || !matches!(image[2], 47 | 92)
        || image.iter().any(|v| matches!(v, 0 | 34))
    {
        return Err(Error::Layout);
    }
    let mut out = vec![34];
    out.extend(image);
    out.push(34);
    for arg in r.args {
        out.push(32);
        out.extend(arg.encode_utf16());
    }
    Ok(out)
}
fn expected(r: &Registration) -> Vec<Config2> {
    vec![
        Config2::Delayed(u32::from(r.delayed_auto_start)),
        Config2::Sid(u32::from(r.unrestricted_service_sid)),
        Config2::Privileges(
            r.privileges
                .iter()
                .map(|p| p.encode_utf16().collect())
                .collect(),
        ),
        Config2::Preshutdown(r.preshutdown_ms),
        Config2::Failure {
            reset: r.reset_seconds,
            reboot: vec![],
            command: vec![],
            actions: vec![
                (1, r.restart_delays_ms[0]),
                (1, r.restart_delays_ms[1]),
                (0, 0),
            ],
        },
        Config2::NonCrash(u32::from(r.restart_non_crash)),
    ]
}
fn manager<T, E>(r: Result<T, E>) -> Result<T, Error> {
    r.map_err(|_| Error::Manager)
}
fn allow_marked(r: Result<(), Win32>) -> Result<(), Error> {
    match r {
        Ok(()) | Err(Win32(MARKED)) => Ok(()),
        _ => Err(Error::Manager),
    }
}
fn mapped(s: Snapshot) -> Result<Status, Error> {
    Ok(match s.state {
        STOPPED if matches!(s.win32_exit, 0 | NEVER_STARTED) && s.specific_exit == 0 => {
            Status::Stopped
        }
        STOPPED | PAUSED | PAUSE_PENDING | CONTINUE_PENDING => Status::Failed,
        START_PENDING => Status::Starting,
        RUNNING => Status::Running,
        STOP_PENDING => Status::Stopping,
        _ => return Err(Error::Manager),
    })
}
#[cfg(windows)]
impl WindowsScm {
    pub(super) fn system(l: &Layout) -> Result<Self, Error> {
        Self::new(l, NativeScm::new(), MonotonicClock(Instant::now()))
    }
}
impl<C: ScmCalls, K: Clock> WindowsScm<C, K> {
    fn new(l: &Layout, calls: C, clock: K) -> Result<Self, Error> {
        if l.platform() != Platform::Windows {
            return Err(Error::Layout);
        }
        let r = registration(l);
        Ok(Self {
            calls,
            clock,
            cmdline: command_line(&r)?,
            expected: expected(&r),
        })
    }
    #[cfg(test)]
    fn for_test(l: &Layout, calls: C, clock: K) -> Self {
        Self::new(l, calls, clock).unwrap()
    }
    fn base(&mut self) -> Result<Boot, Error> {
        let b = manager(self.calls.config())?;
        if b.service_type != TYPE
            || b.image != self.cmdline
            || b.account != "LocalSystem".encode_utf16().collect::<Vec<_>>()
        {
            return Err(Error::Foreign);
        }
        match b.start_type {
            START_AUTO => Ok(Boot::Auto),
            START_DEMAND => Ok(Boot::Demand),
            _ => Err(Error::Foreign),
        }
    }
    fn installed(&mut self) -> Result<bool, Error> {
        let mut ok = true;
        for (i, l) in LEVELS.iter().enumerate() {
            let mut actual = manager(self.calls.config2(*l))?;
            let mut expected = self.expected[i].clone();
            if let (Config2::Privileges(a), Config2::Privileges(b)) = (&mut actual, &mut expected) {
                a.sort();
                b.sort();
            }
            ok &= actual == expected;
        }
        Ok(ok)
    }
    fn open(&mut self, a: Access) -> Result<bool, Error> {
        manager(self.calls.open(a))
    }
    pub fn observe(&mut self) -> Result<Observation, Error> {
        if !self.open(Access::Status)? {
            return Ok(Observation::Absent);
        }
        let boot = match self.base() {
            Err(Error::Foreign) => return Ok(Observation::Foreign),
            other => other?,
        };
        let installed = self.installed()?;
        let active = manager(self.calls.state())?.state != STOPPED;
        Ok(Observation::Ours {
            boot,
            installed,
            active,
        })
    }
    pub fn register(&mut self) -> Result<(), Error> {
        let mut present = self.open(Access::Install)?;
        if !present {
            match self.calls.create(&self.cmdline) {
                Ok(()) => {}
                Err(Win32(EXISTS)) => {
                    if !self.open(Access::Install)? {
                        return Err(Error::Manager);
                    }
                    present = true;
                }
                Err(Win32(DUPLICATE)) => return Err(Error::Foreign),
                _ => return Err(Error::Manager),
            }
        }
        if present {
            self.base()?;
            if manager(self.calls.state())?.state != STOPPED {
                return Err(Error::Active);
            }
            manager(self.calls.set_start(Boot::Demand))?;
        }
        for v in &self.expected {
            manager(self.calls.set_config2(v))?;
        }
        let boot = self.base()?;
        let installed = self.installed()?;
        if boot != Boot::Demand || !installed {
            return Err(Error::Manager);
        }
        if manager(self.calls.state())?.state != STOPPED {
            return Err(Error::Manager);
        }
        Ok(())
    }
    pub fn start(&mut self) -> Result<(), Error> {
        if !self.open(Access::Start)? {
            return Err(Error::NotInstalled);
        }
        self.base()?;
        if !self.installed()? {
            return Err(Error::NotInstalled);
        }
        let s = manager(self.calls.state())?.state;
        if !matches!(s, STOPPED | START_PENDING | RUNNING) {
            return Err(Error::Manager);
        }
        let deadline = self.clock.now() + START_DEADLINE;
        manager(self.calls.set_start(Boot::Auto))?;
        self.check(deadline)?;
        manager(self.calls.set_config2(&Config2::Delayed(1)))?;
        self.check(deadline)?;
        if s == STOPPED {
            match self.calls.start() {
                Ok(()) | Err(Win32(ALREADY_RUNNING)) => {}
                _ => return Err(Error::Manager),
            }
            self.check(deadline)?;
        }
        loop {
            let state = manager(self.calls.state())?.state;
            self.check(deadline)?;
            match state {
                RUNNING => return Ok(()),
                STOPPED => return Err(Error::Manager),
                _ => self.clock.sleep(POLL),
            }
        }
    }
    fn check(&self, deadline: Duration) -> Result<(), Error> {
        if self.clock.now() >= deadline {
            Err(Error::Manager)
        } else {
            Ok(())
        }
    }
    fn stopped(&mut self) -> Result<(), Error> {
        let deadline = self.clock.now() + STOP_DEADLINE;
        let mut sent = false;
        loop {
            let s = manager(self.calls.state())?.state;
            self.check(deadline)?;
            match s {
                STOPPED => return Ok(()),
                RUNNING | PAUSED | PAUSE_PENDING | CONTINUE_PENDING if !sent => {
                    match self.calls.control_stop() {
                        Ok(()) => sent = true,
                        Err(Win32(CANNOT_ACCEPT | NOT_ACTIVE)) => {}
                        _ => return Err(Error::Manager),
                    }
                    self.check(deadline)?;
                }
                RUNNING | PAUSED | PAUSE_PENDING | CONTINUE_PENDING | START_PENDING
                | STOP_PENDING => {}
                _ => return Err(Error::Manager),
            }
            self.clock.sleep(POLL);
        }
    }
    pub fn stop(&mut self) -> Result<(), Error> {
        if !self.open(Access::Stop)? {
            return Err(Error::NotInstalled);
        }
        self.base()?;
        self.stopped()?;
        manager(self.calls.set_start(Boot::Demand))
    }
    pub fn status(&mut self) -> Result<Status, Error> {
        if !self.open(Access::Status)? {
            return Ok(Status::NotInstalled);
        }
        self.base()?;
        if !self.installed()? {
            return Err(Error::NotInstalled);
        }
        mapped(manager(self.calls.state())?)
    }
    pub fn deregister(&mut self) -> Result<Deregistered, Error> {
        if !self.open(Access::Uninstall)? {
            return Ok(Deregistered(()));
        }
        self.base()?;
        self.stopped()?;
        allow_marked(self.calls.set_start(Boot::Demand))?;
        allow_marked(self.calls.delete())?;
        match self.calls.state() {
            Ok(Snapshot { state: STOPPED, .. }) | Err(Fault::Win32(ABSENT)) => Ok(Deregistered(())),
            _ => Err(Error::Manager),
        }
    }
}
#[cfg(test)]
#[path = "owner_scm_tests.rs"]
mod tests;
