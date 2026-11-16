//! Descriptor-bound Linux observation; fixed payload-free outcomes only.
#[cfg(test)]
use std::ffi::CString;
use std::{
    ffi::CStr,
    io::Read,
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    time::{Duration, Instant},
};
const ANCHOR: &CStr = c"/";
const OWNER: u32 = 0;
const CANDIDATES: &[&[&CStr]] = &[
    &[c"usr", c"bin", c"sudo"],
    &[c"usr", c"local", c"bin", c"sudo"],
    &[c"bin", c"sudo"],
];
const ALIASES: &[&[u8]] = &[b"usr/bin", b"/usr/bin"];
const ARGV: &[&CStr] = &[c"sudo", c"-n", c"true"];
const ENV: &[&CStr] = &[];
const DEADLINE: Duration = Duration::from_secs(5);
const NULL: &CStr = c"/dev/null";
const CWD: &CStr = c"/";

#[cfg_attr(not(test), derive(Default))]
struct Settings {
    #[cfg(test)]
    anchor: CString,
    #[cfg(test)]
    owner: u32,
    #[cfg(test)]
    deadline: Duration,
    #[cfg(test)]
    argv: Vec<CString>,
    #[cfg(test)]
    env: Vec<CString>,
}
impl Settings {
    fn anchor(&self) -> &CStr {
        #[cfg(test)]
        return &self.anchor;
        #[cfg(not(test))]
        ANCHOR
    }
    fn owner(&self) -> u32 {
        #[cfg(test)]
        return self.owner;
        #[cfg(not(test))]
        OWNER
    }
    fn deadline(&self) -> Duration {
        #[cfg(test)]
        return self.deadline;
        #[cfg(not(test))]
        DEADLINE
    }
    fn argv(&self) -> Vec<*const libc::c_char> {
        #[cfg(test)]
        let mut values: Vec<_> = self.argv.iter().map(|s| s.as_ptr()).collect();
        #[cfg(not(test))]
        let mut values: Vec<_> = ARGV.iter().map(|s| s.as_ptr()).collect();
        values.push(std::ptr::null());
        values
    }
    fn env(&self) -> Vec<*const libc::c_char> {
        #[cfg(test)]
        let mut values: Vec<_> = self.env.iter().map(|s| s.as_ptr()).collect();
        #[cfg(not(test))]
        let mut values: Vec<_> = ENV.iter().map(|s| s.as_ptr()).collect();
        values.push(std::ptr::null());
        values
    }
}
#[cfg(test)]
impl Default for Settings {
    fn default() -> Self {
        Self {
            #[cfg(test)]
            anchor: ANCHOR.into(),
            #[cfg(test)]
            owner: OWNER,
            #[cfg(test)]
            deadline: DEADLINE,
            #[cfg(test)]
            argv: ARGV.iter().map(|s| CString::from(*s)).collect(),
            #[cfg(test)]
            env: ENV.iter().map(|s| CString::from(*s)).collect(),
        }
    }
}

pub(super) fn probe() -> &'static str {
    run(
        &Settings::default(),
        #[cfg(test)]
        || {},
    )
}
// Fixed thread-local scalars only; never accessed in the fork child.
#[cfg(test)]
#[derive(Clone, Copy, Default)]
struct Observation {
    c1: bool,
    alias_skipped: bool,
    c3_opened: bool,
    path: bool,
    cloexec: bool,
    regular: bool,
    dev: u64,
    ino: u64,
    same_inode: bool,
    uid: u32,
    gid: u32,
    exec_eof: bool,
    nonzero: bool,
}
#[cfg(test)]
fn observation_accepts(o: Observation, uid: u32, gid: u32) -> bool {
    o.c1 && o.alias_skipped
        && !o.c3_opened
        && o.path
        && o.cloexec
        && o.regular
        && o.same_inode
        && o.uid == uid
        && uid != 0
        && o.gid == gid
        && gid != 0
        && o.exec_eof
        && o.nonzero
}
#[cfg(test)]
std::thread_local! {
    static OBSERVATION: std::cell::Cell<Observation> = const { std::cell::Cell::new(Observation {
        c1: false, alias_skipped: false, c3_opened: false, path: false,
        cloexec: false, regular: false, dev: 0, ino: 0, same_inode: false, uid: 0, gid: 0, exec_eof: false, nonzero: false,
    }) };
}
#[cfg(test)]
fn observe(update: impl FnOnce(&mut Observation)) {
    OBSERVATION.with(|slot| {
        let mut value = slot.get();
        update(&mut value);
        slot.set(value);
    });
}
// Test-only payload-free attribution, separate from identity-bearing Observation.
#[cfg(test)]
mod attribution {
    use std::time::{Duration, Instant};

    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub(super) enum Site {
        #[default]
        None,
        Anchor,
        Classify,
        Null,
        Cwd,
        Pipe,
        Fdwrap,
        Fork,
        Deadline,
        ReapedBad,
        WaitErr,
    }
    impl Site {
        pub(super) fn label(self) -> &'static str {
            match self {
                Self::None => "none",
                Self::Anchor => "anchor",
                Self::Classify => "classify",
                Self::Null => "null",
                Self::Cwd => "cwd",
                Self::Pipe => "pipe",
                Self::Fdwrap => "fdwrap",
                Self::Fork => "fork",
                Self::Deadline => "deadline",
                Self::ReapedBad => "reaped_bad",
                Self::WaitErr => "wait_err",
            }
        }
    }

    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub(super) enum Read {
        #[default]
        Na,
        Byte,
        Eof,
        Eagain,
        Other,
    }
    impl Read {
        pub(super) fn label(self) -> &'static str {
            match self {
                Self::Na => "na",
                Self::Byte => "byte",
                Self::Eof => "eof",
                Self::Eagain => "eagain",
                Self::Other => "other",
            }
        }
    }

    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub(super) enum Status {
        #[default]
        Na,
        Exited,
        SignaledKill,
        SignaledOther,
        Other,
    }
    impl Status {
        pub(super) fn label(self) -> &'static str {
            match self {
                Self::Na => "na",
                Self::Exited => "exited",
                Self::SignaledKill => "signaled_kill",
                Self::SignaledOther => "signaled_other",
                Self::Other => "other",
            }
        }
    }

    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub(super) enum Wait {
        #[default]
        Na,
        Pending,
        Reaped,
        Interrupted,
        Echild,
        Other,
    }
    impl Wait {
        pub(super) fn label(self) -> &'static str {
            match self {
                Self::Na => "na",
                Self::Pending => "pending",
                Self::Reaped => "reaped",
                Self::Interrupted => "interrupted",
                Self::Echild => "echild",
                Self::Other => "other",
            }
        }
    }

    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub(super) struct Attribution {
        pub site: Site,
        pub pre_ms: u32,
        pub wait_ms: u32,
        pub polls: u32,
        pub deadline_ms: u32,
        pub read: Read,
        pub status: Status,
        pub wait: Wait,
    }
    std::thread_local! {
        pub(super) static ATTRIBUTION: std::cell::Cell<Attribution> = Default::default();
    }
    pub(super) fn attribute(update: impl FnOnce(&mut Attribution)) {
        ATTRIBUTION.with(|slot| {
            let mut value = slot.get();
            update(&mut value);
            slot.set(value);
        });
    }
    pub(super) fn ms(duration: Duration) -> u32 {
        duration.as_millis().min(60_000) as u32
    }
    // Created before Child: final timing includes its unchanged parent cleanup.
    pub(super) struct Timing {
        start: Instant,
        forked: Option<Instant>,
    }
    impl Timing {
        pub(super) fn new() -> Self {
            Self {
                start: Instant::now(),
                forked: None,
            }
        }
        pub(super) fn parent_fork_returned(&mut self) {
            let now = Instant::now();
            attribute(|a| a.pre_ms = ms(now.duration_since(self.start)));
            self.forked = Some(now);
        }
    }
    impl Drop for Timing {
        fn drop(&mut self) {
            attribute(|a| match self.forked {
                Some(start) => a.wait_ms = ms(start.elapsed()),
                None => a.pre_ms = ms(self.start.elapsed()),
            });
        }
    }
}
#[cfg(test)]
use attribution::{ATTRIBUTION, Attribution, Site, Timing, attribute};

fn fd(raw: i32) -> Result<OwnedFd, &'static str> {
    // Keep every prepared descriptor outside stdio even if the caller closed it.
    if (0..3).contains(&raw) {
        let next = unsafe { libc::fcntl(raw, libc::F_DUPFD_CLOEXEC, 3) };
        unsafe {
            libc::close(raw);
        }
        return fd(next);
    }
    if raw >= 0 {
        Ok(unsafe { OwnedFd::from_raw_fd(raw) })
    } else {
        Err(match std::io::Error::last_os_error().raw_os_error() {
            Some(libc::ENOENT) => "absent",
            Some(libc::ELOOP | libc::ENOTDIR) => "untrusted",
            _ => "unknown",
        })
    }
}
fn stat(fd: &OwnedFd) -> Result<libc::stat, &'static str> {
    let mut value = std::mem::MaybeUninit::uninit();
    if unsafe { libc::fstat(fd.as_raw_fd(), value.as_mut_ptr()) } != 0 {
        return Err("unknown");
    }
    Ok(unsafe { value.assume_init() })
}
fn trusted(value: &libc::stat, kind: libc::mode_t, owner: u32) -> bool {
    value.st_mode & libc::S_IFMT == kind && value.st_uid == owner && value.st_mode & 0o022 == 0
}
fn directory(parent: i32, name: &CStr, owner: u32) -> Result<OwnedFd, &'static str> {
    let value = fd(unsafe {
        libc::openat(
            parent,
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    })?;
    if !trusted(&stat(&value)?, libc::S_IFDIR, owner) {
        return Err("untrusted");
    }
    Ok(value)
}
fn candidate(root: &OwnedFd, parts: &[&CStr], owner: u32) -> Result<OwnedFd, &'static str> {
    // Reopen relative to the validated anchor, never resolve its pathname again.
    let mut parent = directory(root.as_raw_fd(), c".", owner)?;
    for part in &parts[..parts.len() - 1] {
        parent = directory(parent.as_raw_fd(), part, owner)?;
    }
    let value = fd(unsafe {
        libc::openat(
            parent.as_raw_fd(),
            c"sudo".as_ptr(),
            libc::O_PATH | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    })?;
    if !trusted(&stat(&value)?, libc::S_IFREG, owner) {
        return Err("untrusted");
    }
    Ok(value)
}
fn alias(root: &OwnedFd, owner: u32) -> Result<bool, &'static str> {
    let mut value = std::mem::MaybeUninit::<libc::stat>::uninit();
    if unsafe {
        libc::fstatat(
            root.as_raw_fd(),
            c"bin".as_ptr(),
            value.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } != 0
    {
        return match std::io::Error::last_os_error().raw_os_error() {
            Some(libc::ENOENT) => Ok(false),
            _ => Err("unknown"),
        };
    }
    let value = unsafe { value.assume_init() };
    if value.st_mode & libc::S_IFMT != libc::S_IFLNK {
        return Ok(false);
    }
    if value.st_uid != owner {
        return Err("untrusted");
    }
    let mut bytes = [0u8; 16];
    let size = unsafe {
        libc::readlinkat(
            root.as_raw_fd(),
            c"bin".as_ptr(),
            bytes.as_mut_ptr().cast(),
            bytes.len(),
        )
    };
    if size < 0 {
        return Err("unknown");
    }
    if ALIASES.contains(&&bytes[..size as usize]) {
        Ok(true)
    } else {
        Err("untrusted")
    }
}
fn run(settings: &Settings, #[cfg(test)] hook: impl FnOnce()) -> &'static str {
    #[cfg(test)]
    ATTRIBUTION.with(|slot| {
        slot.set(Attribution {
            deadline_ms: attribution::ms(settings.deadline()),
            ..Attribution::default()
        })
    });
    #[cfg(test)]
    attribute(|a| a.site = Site::Anchor);
    let root = match directory(libc::AT_FDCWD, settings.anchor(), settings.owner()) {
        Ok(root) => root,
        Err("untrusted") => return "untrusted",
        Err(_) => return "unknown",
    };
    #[cfg(test)]
    attribute(|a| a.site = Site::None);
    // Anchor trust failure is distinct from open failure.
    classify_and_execute(
        root,
        settings,
        #[cfg(test)]
        hook,
    )
}
fn classify_and_execute(
    root: OwnedFd,
    settings: &Settings,
    #[cfg(test)] hook: impl FnOnce(),
) -> &'static str {
    #[cfg(test)]
    OBSERVATION.with(|slot| slot.set(Observation::default()));
    let mut chosen = None;
    let mut untrusted = false;
    let mut unknown = false;
    for (index, parts) in CANDIDATES.iter().enumerate() {
        let result = if index == 2 {
            match alias(&root, settings.owner()) {
                Ok(true) => {
                    #[cfg(test)]
                    observe(|o| o.alias_skipped = true);
                    continue;
                }
                Ok(false) => {
                    #[cfg(test)]
                    observe(|o| o.c3_opened = true);
                    candidate(&root, parts, settings.owner())
                }
                Err(reason) => Err(reason),
            }
        } else {
            candidate(&root, parts, settings.owner())
        };
        match result {
            Ok(value) => {
                if chosen.is_none() {
                    #[cfg(test)]
                    observe(|o| {
                        o.c1 = index == 0;
                        if let Ok(s) = stat(&value) {
                            o.dev = s.st_dev;
                            o.ino = s.st_ino;
                        }
                    });
                    chosen = Some(value);
                }
            }
            Err("untrusted") => untrusted = true,
            Err("absent") => (),
            Err(_) => unknown = true,
        }
    }
    if untrusted {
        return "untrusted";
    }
    if unknown {
        #[cfg(test)]
        attribute(|a| a.site = Site::Classify);
        return "unknown";
    }
    let Some(chosen) = chosen else {
        return "absent";
    };
    #[cfg(test)]
    hook();
    execute(chosen, settings)
}

// Own-child guard never signals a reaped/reusable PID.
struct Child {
    pid: libc::pid_t,
    reaped: bool,
}
impl Drop for Child {
    fn drop(&mut self) {
        if !self.reaped {
            unsafe {
                libc::kill(self.pid, libc::SIGKILL);
            }
            loop {
                if unsafe { libc::waitpid(self.pid, std::ptr::null_mut(), 0) } >= 0 {
                    break;
                }
                if std::io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
                    break;
                }
            }
        }
    }
}
fn execute(executable: OwnedFd, settings: &Settings) -> &'static str {
    #[cfg(test)]
    let mut timing = Timing::new();
    let argv = settings.argv();
    let env = settings.env();
    let Ok(null) = fd(unsafe { libc::open(NULL.as_ptr(), libc::O_RDWR | libc::O_CLOEXEC) }) else {
        #[cfg(test)]
        attribute(|a| a.site = Site::Null);
        return "unknown";
    };
    let Ok(cwd) = directory(libc::AT_FDCWD, CWD, OWNER) else {
        #[cfg(test)]
        attribute(|a| a.site = Site::Cwd);
        return "unknown";
    };
    let mut pipe = [-1; 2];
    if unsafe { libc::pipe2(pipe.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK) } != 0 {
        #[cfg(test)]
        attribute(|a| a.site = Site::Pipe);
        return "unknown";
    }
    let read_result = fd(pipe[0]);
    let write_result = fd(pipe[1]);
    let (Ok(read), Ok(write)) = (read_result, write_result) else {
        #[cfg(test)]
        attribute(|a| a.site = Site::Fdwrap);
        return "unknown";
    };
    #[cfg(test)]
    observe(|o| {
        let flags = unsafe { libc::fcntl(executable.as_raw_fd(), libc::F_GETFL) };
        o.path = flags >= 0 && flags & libc::O_PATH != 0;
        let flags = unsafe { libc::fcntl(executable.as_raw_fd(), libc::F_GETFD) };
        o.cloexec = flags >= 0 && flags & libc::FD_CLOEXEC != 0;
        o.regular = stat(&executable).is_ok_and(|s| trusted(&s, libc::S_IFREG, settings.owner()));
        o.same_inode = stat(&executable).is_ok_and(|s| s.st_dev == o.dev && s.st_ino == o.ino);
        o.uid = unsafe { libc::getuid() };
        o.gid = unsafe { libc::getgid() };
    });
    let pid = unsafe { libc::fork() };
    #[cfg(test)]
    if pid != 0 {
        timing.parent_fork_returned();
    }
    if pid < 0 {
        #[cfg(test)]
        attribute(|a| a.site = Site::Fork);
        return "unknown";
    }
    if pid == 0 {
        // No allocation, destructors, formatting or Rust panic in the child.
        unsafe {
            let mut good = libc::fchdir(cwd.as_raw_fd()) == 0;
            for target in 0..3 {
                if libc::dup2(null.as_raw_fd(), target) < 0 {
                    good = false;
                }
            }
            if good {
                libc::execveat(
                    executable.as_raw_fd(),
                    c"".as_ptr(),
                    argv.as_ptr().cast(),
                    env.as_ptr().cast(),
                    libc::AT_EMPTY_PATH,
                );
            }
            libc::write(write.as_raw_fd(), b"E".as_ptr().cast(), 1);
            libc::_exit(127);
        }
    }
    let mut child = Child { pid, reaped: false };
    drop(write);
    let deadline = Instant::now() + settings.deadline();
    loop {
        if Instant::now() >= deadline {
            #[cfg(test)]
            attribute(|a| a.site = Site::Deadline);
            return "unknown";
        }
        let mut status = 0;
        #[cfg(test)]
        attribute(|a| a.polls = a.polls.saturating_add(1));
        let result = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
        #[cfg(test)]
        {
            let errno = std::io::Error::last_os_error().raw_os_error();
            attribute(|a| {
                a.wait = if result == pid {
                    attribution::Wait::Reaped
                } else if result == 0 {
                    attribution::Wait::Pending
                } else {
                    match errno {
                        Some(libc::EINTR) => attribution::Wait::Interrupted,
                        Some(libc::ECHILD) => attribution::Wait::Echild,
                        _ => attribution::Wait::Other,
                    }
                }
            });
        }
        if result == pid {
            child.reaped = true;
            #[cfg(test)]
            attribute(|a| {
                a.status = if libc::WIFEXITED(status) {
                    attribution::Status::Exited
                } else if libc::WIFSIGNALED(status) {
                    if libc::WTERMSIG(status) == libc::SIGKILL {
                        attribution::Status::SignaledKill
                    } else {
                        attribution::Status::SignaledOther
                    }
                } else {
                    attribution::Status::Other
                }
            });
            let mut error = [0u8; 1];
            let count = unsafe { libc::read(read.as_raw_fd(), error.as_mut_ptr().cast(), 1) };
            #[cfg(test)]
            {
                let errno = std::io::Error::last_os_error().raw_os_error();
                attribute(|a| {
                    a.read = match count {
                        0 => attribution::Read::Eof,
                        1 => attribution::Read::Byte,
                        _ if errno == Some(libc::EAGAIN) => attribution::Read::Eagain,
                        _ => attribution::Read::Other,
                    }
                });
            }
            if count != 0 || !libc::WIFEXITED(status) {
                #[cfg(test)]
                attribute(|a| a.site = Site::ReapedBad);
                return "unknown";
            }
            #[cfg(test)]
            observe(|o| {
                o.exec_eof = true;
                o.nonzero = libc::WEXITSTATUS(status) != 0;
            });
            return if libc::WEXITSTATUS(status) == 0 {
                "exit_zero"
            } else {
                "exit_nonzero"
            };
        }
        if result < 0 {
            #[cfg(test)]
            attribute(|a| {
                if a.wait != attribution::Wait::Interrupted {
                    a.site = Site::WaitErr;
                }
            });
            if std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            if std::io::Error::last_os_error().raw_os_error() == Some(libc::ECHILD) {
                child.reaped = true;
            }
            return "unknown";
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

pub(super) fn nnp() -> Option<bool> {
    let value = fd(unsafe {
        libc::open(
            c"/proc/self/status".as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    })
    .ok()?;
    let mut fs = std::mem::MaybeUninit::<libc::statfs>::uninit();
    if unsafe { libc::fstatfs(value.as_raw_fd(), fs.as_mut_ptr()) } != 0
        || unsafe { fs.assume_init() }.f_type != 0x9fa0
    {
        return None;
    }
    let mut bytes = Vec::new();
    std::fs::File::from(value)
        .take(8193)
        .read_to_end(&mut bytes)
        .ok()?;
    parse_nnp(&bytes)
}
fn parse_nnp(bytes: &[u8]) -> Option<bool> {
    if bytes.len() > 8192 {
        return None;
    }
    let text = std::str::from_utf8(bytes).ok()?;
    if text.contains(['\r', '\0']) {
        return None;
    }
    let mut found = None;
    for line in text.lines() {
        if line.starts_with("NoNewPrivs:") {
            if found.is_some() {
                return None;
            }
            found = Some(match line {
                "NoNewPrivs:\t1" => true,
                "NoNewPrivs:\t0" => false,
                _ => return None,
            });
        }
    }
    found
}

#[cfg(test)]
#[path = "unix_shim_rights_linux_tests.rs"]
mod tests;
