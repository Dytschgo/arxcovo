//! Native operations have only one caller: the ignored A4 fixture below.
use super::*;
use hierarchy::{
    fake::{a4, a5},
    linux::{Dir, LinuxFs},
    setup::{CgroupFs, Clock, PROC_MAGIC},
};
use std::{
    ffi::{CStr, CString},
    io::Read,
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    time::Instant,
};

// Pending driver delivery must stage this exact reviewed cross-image. No lookup,
// env override, build fallback or executable re-entry is permitted.
const HELPER: &CStr = c"a4-helper";
fn open(parent: i32, name: &CStr, directory: bool) -> Option<OwnedFd> {
    let flags = libc::O_RDONLY
        | libc::O_NOFOLLOW
        | libc::O_CLOEXEC
        | libc::O_NONBLOCK
        | if directory { libc::O_DIRECTORY } else { 0 };
    let fd = unsafe { libc::openat(parent, name.as_ptr(), flags) };
    (fd >= 0).then(|| unsafe { OwnedFd::from_raw_fd(fd) })
}
fn trusted(fd: &OwnedFd, kind: u32) -> bool {
    let mut s = std::mem::MaybeUninit::<libc::stat>::uninit();
    if unsafe { libc::fstat(fd.as_raw_fd(), s.as_mut_ptr()) } != 0 {
        return false;
    }
    let s = unsafe { s.assume_init() };
    s.st_uid == 0 && s.st_mode & 0o022 == 0 && s.st_mode & libc::S_IFMT == kind
}
fn helper() -> Option<OwnedFd> {
    let mut dir = open(libc::AT_FDCWD, c"/", true)?;
    for part in [c"usr", c"libexec", c"arxcovo"] {
        if !trusted(&dir, libc::S_IFDIR) {
            return None;
        }
        dir = open(dir.as_raw_fd(), part, true)?;
    }
    if !trusted(&dir, libc::S_IFDIR) {
        return None;
    }
    let fd = open(dir.as_raw_fd(), HELPER, false)?;
    if !trusted(&fd, libc::S_IFREG) {
        return None;
    }
    let mut file = fs::File::from(fd);
    let info = file.metadata().ok()?;
    use std::os::unix::fs::MetadataExt;
    if info.len() != a4::IMAGE_SIZE as u64 || info.mode() & 0o111 == 0 || info.nlink() != 1 {
        return None;
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take((a4::IMAGE_SIZE + 1) as u64)
        .read_to_end(&mut bytes)
        .ok()?;
    if !a4::image(&bytes) {
        return None;
    }
    Some(file.into()) // execveat ignores the checked descriptor's read offset
}
fn now() -> Option<u64> {
    let mut ts = std::mem::MaybeUninit::<libc::timespec>::uninit();
    if unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, ts.as_mut_ptr()) } != 0 {
        return None;
    }
    let ts = unsafe { ts.assume_init() };
    if ts.tv_sec < 0 || !(0..1_000_000_000).contains(&ts.tv_nsec) {
        return None;
    }
    (ts.tv_sec as u64)
        .checked_mul(1_000_000_000)?
        .checked_add(ts.tv_nsec as u64)
}
fn pipe() -> Option<(OwnedFd, OwnedFd)> {
    let mut p = [-1; 2];
    if unsafe { libc::pipe2(p.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK) } != 0 {
        return None;
    }
    Some(unsafe { (OwnedFd::from_raw_fd(p[0]), OwnedFd::from_raw_fd(p[1])) })
}
struct Child<'a> {
    pid: i32,
    reaped: bool,
    deadline: u64,
    last: u64,
    release: OwnedFd,
    fs: &'a LinuxFs,
    leaf: &'a Dir,
}
impl Child<'_> {
    fn cleanup_failed_acceptance(&mut self) -> bool {
        if self.reaped || unsafe { libc::kill(self.pid, libc::SIGKILL) } != 0 {
            return false;
        }
        use a4::ChildOps;
        self.killed() && self.reaped
    }

    fn remaining_ns(&mut self) -> Option<u64> {
        let n = now()?;
        let remaining = a4::remaining(self.deadline, self.last, n)?;
        self.last = n;
        Some(remaining)
    }
    fn wait(&mut self) -> Option<i32> {
        loop {
            self.remaining_ns()?;
            let mut status = 0;
            let rc = unsafe { libc::waitpid(self.pid, &mut status, libc::WNOHANG) };
            if rc == self.pid {
                self.reaped = true;
                self.remaining_ns()?;
                return Some(status);
            }
            if rc < 0 {
                let e = std::io::Error::last_os_error().raw_os_error();
                if e == Some(libc::ECHILD) {
                    self.reaped = true;
                }
                if e != Some(libc::EINTR) {
                    return None;
                }
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    fn ready(&mut self, read: &OwnedFd) -> bool {
        loop {
            let Some(ns) = self.remaining_ns() else {
                return false;
            };
            let mut p = libc::pollfd {
                fd: read.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            let rc = unsafe {
                libc::poll(
                    &mut p,
                    1,
                    ns.div_ceil(1_000_000).min(i32::MAX as u64) as i32,
                )
            };
            if rc < 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            if rc != 1
                || p.revents & libc::POLLIN == 0
                || p.revents & !(libc::POLLIN | libc::POLLHUP) != 0
            {
                return false;
            }
            let mut byte = 0u8;
            let rc = unsafe { libc::read(read.as_raw_fd(), (&mut byte as *mut u8).cast(), 1) };
            if rc < 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            return rc == 1 && byte == b'R' && self.remaining_ns().is_some();
        }
    }
}
impl a4::ChildOps for Child<'_> {
    fn live(&mut self) -> bool {
        let mut status = 0;
        let rc = unsafe { libc::waitpid(self.pid, &mut status, libc::WNOHANG) };
        if rc == self.pid {
            self.reaped = true;
            return false;
        }
        if rc != 0 {
            if rc < 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ECHILD) {
                self.reaped = true; // never signal an identity we no longer own
            }
            return false;
        }
        // Numeric, no-follow procfs descriptor: bounded task inventory and
        // actual kernel membership, never a task-count fixture assignment.
        let Some(proc) = open(libc::AT_FDCWD, c"/proc", true) else {
            return false;
        };
        let mut magic = std::mem::MaybeUninit::<libc::statfs>::uninit();
        if unsafe { libc::fstatfs(proc.as_raw_fd(), magic.as_mut_ptr()) } != 0
            || unsafe { magic.assume_init() }.f_type as u64 != PROC_MAGIC
        {
            return false;
        }
        let pid = CString::new(self.pid.to_string()).unwrap();
        let Some(own) = open(proc.as_raw_fd(), &pid, true) else {
            return false;
        };
        let Some(tasks) = open(own.as_raw_fd(), c"task", true) else {
            return false;
        };
        // getdents64 is bounded to one small buffer; require just dots + PID.
        let mut buf = [0u8; 4096];
        let n = unsafe {
            libc::syscall(
                libc::SYS_getdents64,
                tasks.as_raw_fd(),
                buf.as_mut_ptr(),
                buf.len(),
            )
        };
        if n <= 0 {
            return false;
        }
        if !a4::single_task(&buf[..n as usize], pid.as_bytes())
            || unsafe {
                libc::syscall(
                    libc::SYS_getdents64,
                    tasks.as_raw_fd(),
                    buf.as_mut_ptr(),
                    buf.len(),
                )
            } != 0
        {
            return false;
        }
        self.fs
            .read(self.leaf, "cgroup.procs", 4096)
            .ok()
            .and_then(|b| String::from_utf8(b).ok())
            .and_then(|s| hierarchy::parse::pids(&s))
            == Some(vec![self.pid as u32])
    }
    fn remaining(&mut self) -> Option<Duration> {
        self.remaining_ns().map(Duration::from_nanos)
    }
    fn release(&mut self) -> bool {
        loop {
            if self.remaining_ns().is_none() {
                return false;
            }
            let rc = unsafe { libc::write(self.release.as_raw_fd(), b"G".as_ptr().cast(), 1) };
            if rc < 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            return rc == 1;
        }
    }
    fn killed(&mut self) -> bool {
        self.wait()
            .is_some_and(|s| libc::WIFSIGNALED(s) && libc::WTERMSIG(s) == libc::SIGKILL)
    }
}
impl Drop for Child<'_> {
    fn drop(&mut self) {
        // Failure-only owned-child cleanup. A successful killed() already reaped.
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

#[test]
#[ignore = "A4 fixed staged helper + Standard unit; fresh disposable Linux only; unselected"]
fn disposable_linux_a4() {
    fixture(Case::Positive);
}

#[test]
#[ignore = "A4 fixed omitted S7; fresh disposable Linux only; NOT granted"]
fn disposable_linux_a4_omitted_kill() {
    fixture(Case::Omitted);
}

enum Case {
    Positive,
    Omitted,
}

fn rejection_marker() -> bool {
    let bytes = b"ARX_A4_OMITTED_S7_REJECTED\n";
    let mut offset = 0;
    for _ in 0..64 {
        let rc = unsafe {
            libc::write(
                libc::STDOUT_FILENO,
                bytes[offset..].as_ptr().cast(),
                bytes.len() - offset,
            )
        };
        if rc > 0 {
            offset += rc as usize;
            if offset == bytes.len() {
                return true;
            }
        } else if rc == 0 || std::io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
            return false;
        }
    }
    false
}

// Both literal entries use this one fixed spawn/seed/placement body.
fn fixture(case: Case) {
    for (key, value) in [
        ("CI", "true"),
        ("GITHUB_ACTIONS", "true"),
        ("RUNNER_ENVIRONMENT", "github-hosted"),
        ("ARX_DISPOSABLE_EXEC_TEST", "1"),
    ] {
        assert_eq!(std::env::var(key).as_deref(), Ok(value));
    }
    assert_eq!(unsafe { libc::getuid() }, 0);
    assert_eq!(unsafe { libc::geteuid() }, 0);
    assert!(!Path::new("identity-ack").exists());
    // Query only: never change libtest's process-global signal disposition.
    let mut action = std::mem::MaybeUninit::<libc::sigaction>::uninit();
    assert_eq!(
        unsafe { libc::sigaction(libc::SIGPIPE, std::ptr::null(), action.as_mut_ptr()) },
        0
    );
    assert_eq!(unsafe { action.assume_init() }.sa_sigaction, libc::SIG_IGN);
    let executable = helper().expect("fixed staged A4 helper missing or pin mismatch");
    let fs = LinuxFs::system();
    let proc = fs.proc_cgroup().unwrap();
    assert_eq!(proc.magic, PROC_MAGIC);
    let location = hierarchy::locate(&proc.bytes).unwrap();
    let (runs, leaf) = fs.seed_a4(&location).unwrap();
    let (read, ready) = pipe().unwrap();
    let (release, write) = pipe().unwrap();
    let null = unsafe {
        libc::open(
            c"/dev/null".as_ptr(),
            libc::O_RDWR | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    assert!(null > 2);
    let null = unsafe { OwnedFd::from_raw_fd(null) };
    let mut st = std::mem::MaybeUninit::<libc::stat>::uninit();
    assert_eq!(unsafe { libc::fstat(null.as_raw_fd(), st.as_mut_ptr()) }, 0);
    let st = unsafe { st.assume_init() };
    assert_eq!(st.st_rdev, libc::makedev(1, 3));
    assert_eq!(st.st_mode & libc::S_IFMT, libc::S_IFCHR);
    assert_eq!(st.st_uid, 0);
    let start = now().unwrap();
    let deadline = start.checked_add(15_000_000_000).unwrap();
    let args = [
        CString::new("a4-helper").unwrap(),
        CString::new(ready.as_raw_fd().to_string()).unwrap(),
        CString::new(release.as_raw_fd().to_string()).unwrap(),
        CString::new(deadline.to_string()).unwrap(),
    ];
    let argv = [
        args[0].as_ptr(),
        args[1].as_ptr(),
        args[2].as_ptr(),
        args[3].as_ptr(),
        std::ptr::null(),
    ];
    let env = [std::ptr::null::<libc::c_char>()];
    // Parent-only bounded FD inventory. No child-side traversal or runtime audit.
    // Temporarily inheritable endpoints are restored immediately after fork.
    let entries: Vec<_> = fs::read_dir("/proc/self/fd").unwrap().take(513).collect();
    assert!(entries.len() <= 512);
    for entry in entries {
        let fd: i32 = entry
            .unwrap()
            .file_name()
            .to_str()
            .unwrap()
            .parse()
            .unwrap();
        if fd > 2 {
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
            assert!(flags < 0 || flags & libc::FD_CLOEXEC != 0);
        }
    }
    for fd in [&ready, &release] {
        assert!(fd.as_raw_fd() > 2);
        assert_eq!(unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFD, 0) }, 0);
    }
    let pid = unsafe { libc::fork() };
    if pid == 0 {
        // E4 spawn window: prepared pointers only; dup2/execveat/write/_exit.
        // There is no in-test waiting child or Rust cleanup on this branch.
        unsafe {
            let mut good = true;
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
            libc::write(ready.as_raw_fd(), b"E".as_ptr().cast(), 1);
            libc::_exit(127);
        }
    }
    // Construct the guard before any parent assertion that could unwind.
    let mut child = (pid > 0).then(|| Child {
        pid,
        reaped: false,
        deadline,
        last: start,
        release: write,
        fs: &fs,
        leaf: &leaf,
    });
    for fd in [&ready, &release] {
        assert_eq!(
            unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) },
            0
        );
    }
    drop((ready, release));
    let child = child.as_mut().expect("A4 fork failed");
    assert!(child.ready(&read));
    fs.place_a4(&runs, &leaf, pid as u32).unwrap();
    struct Monotonic(Instant);
    impl Clock for Monotonic {
        fn now(&self) -> Duration {
            self.0.elapsed()
        }
        fn sleep(&self, d: Duration) {
            std::thread::sleep(d);
        }
    }
    match case {
        Case::Positive => a4::acceptance(a4::finish(
            &fs,
            &runs,
            &leaf,
            &Monotonic(Instant::now()),
            child,
        )),
        Case::Omitted => {
            let omitted = a4::OmitS7::new(&fs).unwrap();
            let (r, l) = omitted.held(&runs, &leaf);
            let observation =
                a4::finish_observed(&omitted, &r, &l, &Monotonic(Instant::now()), child);
            let panicked = a4::assertion_rejected(observation.accepted);
            assert!(!child.reaped);
            assert!(a4::rejection(
                &omitted,
                &r,
                &l,
                &observation,
                panicked,
                child
            ));
            assert!(!child.reaped);
            fs.retained_a4(&runs, &leaf).unwrap();
            assert!(rejection_marker());
            // All attribution precedes the separately owned failure-only signal.
            assert!(child.cleanup_failed_acceptance());
            fs.cleanup_a4(&runs, &leaf).unwrap();
        }
    }
    a5::empty(&fs, &runs).unwrap();
    let socket = std::env::var_os("NOTIFY_SOCKET").unwrap();
    crate::service::unix::Notify::send_to(&socket, b"READY=1").unwrap();
    hold_identity_ack();
}
