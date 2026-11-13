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
    let root = match directory(libc::AT_FDCWD, settings.anchor(), settings.owner()) {
        Ok(root) => root,
        Err("untrusted") => return "untrusted",
        Err(_) => return "unknown",
    };
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
    let mut chosen = None;
    let mut untrusted = false;
    let mut unknown = false;
    for (index, parts) in CANDIDATES.iter().enumerate() {
        let result = if index == 2 {
            match alias(&root, settings.owner()) {
                Ok(true) => continue,
                Ok(false) => candidate(&root, parts, settings.owner()),
                Err(reason) => Err(reason),
            }
        } else {
            candidate(&root, parts, settings.owner())
        };
        match result {
            Ok(value) => {
                if chosen.is_none() {
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
    let argv = settings.argv();
    let env = settings.env();
    let Ok(null) = fd(unsafe { libc::open(NULL.as_ptr(), libc::O_RDWR | libc::O_CLOEXEC) }) else {
        return "unknown";
    };
    let Ok(cwd) = directory(libc::AT_FDCWD, CWD, OWNER) else {
        return "unknown";
    };
    let mut pipe = [-1; 2];
    if unsafe { libc::pipe2(pipe.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK) } != 0 {
        return "unknown";
    }
    let read_result = fd(pipe[0]);
    let write_result = fd(pipe[1]);
    let (Ok(read), Ok(write)) = (read_result, write_result) else {
        return "unknown";
    };
    let pid = unsafe { libc::fork() };
    if pid < 0 {
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
            return "unknown";
        }
        let mut status = 0;
        let result = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
        if result == pid {
            child.reaped = true;
            let mut error = [0u8; 1];
            let count = unsafe { libc::read(read.as_raw_fd(), error.as_mut_ptr().cast(), 1) };
            if count != 0 || !libc::WIFEXITED(status) {
                return "unknown";
            }
            return if libc::WEXITSTATUS(status) == 0 {
                "exit_zero"
            } else {
                "exit_nonzero"
            };
        }
        if result < 0 {
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
