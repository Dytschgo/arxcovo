//! Inactive setup driver. All observations and mutations go through the port.
use super::parse::{self, Entry, Location};
use crate::development::Cancellation;
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    time::Duration,
};

pub(crate) const PROC_MAGIC: u64 = 0x9fa0;
pub(crate) const CGROUP_MAGIC: u64 = 0x63677270;
const TEXT_MAX: usize = 4096;
const RECORD_MAX: usize = 512;
const CHILD_MAX: usize = 64;
const DEADLINE: Duration = Duration::from_secs(5);
const POLL: Duration = Duration::from_millis(5);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FsError {
    Invalid,
    Busy,
    Missing,
    Refused,
    TooBig,
    Io,
}
pub(crate) type FsResult<T> = Result<T, FsError>;
pub(crate) struct ProcFile {
    pub magic: u64,
    pub bytes: Vec<u8>,
}
pub(crate) struct Stat {
    pub uid: u32,
    pub mode: u32,
    pub magic: u64,
}
pub(crate) struct RawEntry {
    pub name: Vec<u8>,
    pub d_type: u8,
    pub fstatat_mode: Option<Result<u32, ()>>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Created {
    Created,
    Exists,
}
/// Implementations must bind handles without following links; root includes
/// trusted ancestors. Reads and listings must enforce the supplied bounds.
pub(crate) trait CgroupFs {
    type Dir;
    fn proc_cgroup(&self) -> FsResult<ProcFile>;
    fn proc_status(&self) -> FsResult<ProcFile>;
    fn identity(&self) -> FsResult<(u32, u32)>;
    fn pid(&self) -> FsResult<u32>;
    fn open_root(&self) -> FsResult<Self::Dir>;
    fn open_child(&self, dir: &Self::Dir, name: &[u8]) -> FsResult<Self::Dir>;
    fn stat(&self, dir: &Self::Dir) -> FsResult<Stat>;
    fn read(&self, dir: &Self::Dir, file: &str, max: usize) -> FsResult<Vec<u8>>;
    fn write(&self, dir: &Self::Dir, file: &str, bytes: &[u8]) -> FsResult<()>;
    fn xattr_delegate(&self, dir: &Self::Dir) -> FsResult<Option<Vec<u8>>>;
    fn mkdir_runs(&self, dir: &Self::Dir) -> FsResult<Created>;
    fn list(&self, dir: &Self::Dir, max_records: usize) -> FsResult<Vec<RawEntry>>;
    fn rmdir(&self, dir: &Self::Dir, name: &[u8]) -> FsResult<()>;
    fn open_ro(&self, dir: &Self::Dir, file: &str) -> FsResult<()>;
}
pub(crate) trait Clock {
    fn now(&self) -> Duration;
    fn sleep(&self, duration: Duration);
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SetupEvidence {
    pub leaves_removed: u8,
    pub runs_created: bool,
    pub unit_written: bool,
    pub runs_written: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SetupOutcome {
    Ready(SetupEvidence),
    Unavailable(SetupFailure),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SetupFailure {
    Hardening,
    Identity,
    Locate,
    Root,
    Unit,
    Delegation,
    UnitState,
    Supervisor,
    EnableUnit,
    Runs,
    KillInterface,
    KillTimeout,
    LeafStructure,
    LeafRemoval,
    VerifyFailed,
    EnableRuns,
    Postcondition,
    Stopped,
    Panicked,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SetupCause {
    Setup,
    Hardening,
    Janitor,
    Stopped,
}
impl SetupFailure {
    pub(crate) fn cause(self) -> SetupCause {
        match self {
            Self::Hardening => SetupCause::Hardening,
            Self::KillTimeout | Self::LeafRemoval | Self::VerifyFailed | Self::Panicked => {
                SetupCause::Janitor
            }
            Self::Stopped => SetupCause::Stopped,
            _ => SetupCause::Setup,
        }
    }
}
fn check(ok: bool, failure: SetupFailure) -> Result<(), SetupFailure> {
    if ok { Ok(()) } else { Err(failure) }
}
fn stopped(token: &Cancellation) -> Result<(), SetupFailure> {
    check(!token.is_cancelled(), SetupFailure::Stopped)
}
fn trusted<F: CgroupFs + ?Sized>(fs: &F, dir: &F::Dir) -> bool {
    fs.stat(dir).is_ok_and(|s| {
        s.uid == 0
            && s.mode & 0o022 == 0
            && s.magic == CGROUP_MAGIC
            && s.mode & 0o170000 == 0o040000
    })
}
fn text<F: CgroupFs + ?Sized>(fs: &F, dir: &F::Dir, file: &str) -> Option<String> {
    let bytes = fs.read(dir, file, TEXT_MAX).ok()?;
    if bytes.len() > TEXT_MAX {
        return None;
    }
    String::from_utf8(bytes).ok()
}
fn empty_procs<F: CgroupFs + ?Sized>(fs: &F, dir: &F::Dir) -> bool {
    text(fs, dir, "cgroup.procs")
        .and_then(|s| parse::pids(&s))
        .is_some_and(|p| p.is_empty())
}
fn empty_events<F: CgroupFs + ?Sized>(fs: &F, dir: &F::Dir) -> bool {
    text(fs, dir, "cgroup.events").and_then(|s| parse::events(&s)) == Some(false)
}
fn controllers<F: CgroupFs + ?Sized>(fs: &F, dir: &F::Dir, file: &str) -> bool {
    text(fs, dir, file).is_some_and(|s| {
        let c = parse::controllers(&s);
        c.memory && c.pids
    })
}
fn domain<F: CgroupFs + ?Sized>(fs: &F, dir: &F::Dir) -> bool {
    text(fs, dir, "cgroup.type").is_some_and(|s| s == "domain" || s == "domain\n")
}
fn children<F: CgroupFs + ?Sized>(fs: &F, dir: &F::Dir) -> Option<Vec<Vec<u8>>> {
    let records = fs.list(dir, RECORD_MAX).ok()?;
    if records.len() > RECORD_MAX {
        return None;
    }
    let mut children = Vec::new();
    for r in records {
        match parse::classify_entry(&r.name, r.d_type, r.fstatat_mode) {
            Entry::Dot | Entry::Control => {}
            Entry::Child => {
                children.push(r.name);
                if children.len() > CHILD_MAX {
                    return None;
                }
            }
            Entry::Refused => return None,
        }
    }
    Some(children)
}
fn enable<F: CgroupFs + ?Sized>(fs: &F, dir: &F::Dir) -> bool {
    fs.write(dir, "cgroup.subtree_control", b"+memory +pids")
        .is_ok()
        && controllers(fs, dir, "cgroup.subtree_control")
}
pub(crate) fn setup<F: CgroupFs + ?Sized>(
    fs: &F,
    location: &Location,
    clock: &dyn Clock,
    token: &Cancellation,
) -> SetupOutcome {
    // Never inspect or format a panic payload. No hook replacement here.
    match catch_unwind(AssertUnwindSafe(|| body(fs, location, clock, token))) {
        Ok(Ok(e)) => SetupOutcome::Ready(e),
        Ok(Err(f)) => SetupOutcome::Unavailable(f),
        Err(_) => SetupOutcome::Unavailable(SetupFailure::Panicked),
    }
}
fn body<F: CgroupFs + ?Sized>(
    fs: &F,
    location: &Location,
    clock: &dyn Clock,
    token: &Cancellation,
) -> Result<SetupEvidence, SetupFailure> {
    use SetupFailure::*;
    stopped(token)?;
    let status = fs.proc_status().map_err(|_| Hardening)?;
    check(
        status.magic == PROC_MAGIC && parse::hardening(&status.bytes) == parse::Hardening::Standard,
        Hardening,
    )?;
    stopped(token)?;
    check(fs.identity().map_err(|_| Identity)? == (0, 0), Identity)?;
    stopped(token)?;
    let proc = fs.proc_cgroup().map_err(|_| Locate)?;
    let located = parse::locate(&proc.bytes).ok_or(Locate)?;
    check(
        proc.magic == PROC_MAGIC && located.unit_components() == location.unit_components(),
        Locate,
    )?;
    stopped(token)?;
    let root = fs.open_root().map_err(|_| Root)?;
    check(trusted(fs, &root), Root)?;
    stopped(token)?;
    let mut unit = root;
    for component in location.unit_components() {
        stopped(token)?;
        unit = fs
            .open_child(&unit, component.as_bytes())
            .map_err(|_| Unit)?;
        check(trusted(fs, &unit), Unit)?;
    }
    check(
        fs.xattr_delegate(&unit).map_err(|_| Delegation)?.as_deref() == Some(b"1"),
        Delegation,
    )?;
    check(
        domain(fs, &unit) && empty_procs(fs, &unit) && controllers(fs, &unit, "cgroup.controllers"),
        UnitState,
    )?;
    let supervisor = fs
        .open_child(&unit, b"supervisor")
        .map_err(|_| Supervisor)?;
    check(trusted(fs, &supervisor), Supervisor)?;
    let own = fs.pid().map_err(|_| Supervisor)?;
    check(
        text(fs, &supervisor, "cgroup.procs").and_then(|s| parse::pids(&s)) == Some(vec![own]),
        Supervisor,
    )?;
    stopped(token)?;
    check(enable(fs, &unit), EnableUnit)?;
    stopped(token)?;
    let created = fs.mkdir_runs(&unit).map_err(|_| Runs)? == Created::Created;
    let runs = fs.open_child(&unit, b"runs").map_err(|_| Runs)?;
    check(
        trusted(fs, &runs) && domain(fs, &runs) && empty_procs(fs, &runs),
        Runs,
    )?;
    stopped(token)?;
    fs.write(&runs, "cgroup.kill", b"1")
        .map_err(|_| KillInterface)?;
    let start = clock.now();
    loop {
        stopped(token)?;
        let populated = text(fs, &runs, "cgroup.events")
            .and_then(|s| parse::events(&s))
            .ok_or(Postcondition)?;
        if !populated {
            break;
        }
        check(clock.now().saturating_sub(start) < DEADLINE, KillTimeout)?;
        clock.sleep(POLL);
    }
    stopped(token)?;
    let leaves = children(fs, &runs).ok_or(LeafStructure)?;
    for name in &leaves {
        stopped(token)?;
        check(parse::leaf_name(name), LeafStructure)?;
        let leaf = fs.open_child(&runs, name).map_err(|_| LeafStructure)?;
        check(
            trusted(fs, &leaf)
                && children(fs, &leaf).is_some_and(|c| c.is_empty())
                && empty_events(fs, &leaf),
            LeafStructure,
        )?;
    }
    for name in &leaves {
        stopped(token)?;
        fs.rmdir(&runs, name).map_err(|_| LeafRemoval)?;
    }
    stopped(token)?;
    check(
        children(fs, &runs).is_some_and(|c| c.is_empty()) && empty_events(fs, &runs),
        VerifyFailed,
    )?;
    stopped(token)?;
    check(enable(fs, &runs), EnableRuns)?;
    stopped(token)?;
    for file in ["memory.max", "memory.swap.max", "pids.max"] {
        fs.open_ro(&runs, file).map_err(|_| Postcondition)?;
    }
    check(
        empty_events(fs, &runs)
            && empty_procs(fs, &runs)
            && children(fs, &runs).is_some_and(|c| c.is_empty()),
        Postcondition,
    )?;
    stopped(token)?;
    Ok(SetupEvidence {
        leaves_removed: leaves.len() as u8,
        runs_created: created,
        unit_written: true,
        runs_written: true,
    })
}
