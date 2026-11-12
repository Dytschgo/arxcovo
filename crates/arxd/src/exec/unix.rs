use super::output::Capture;
use super::*;
use std::{
    ffi::CString,
    io::Read,
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::{ffi::OsStrExt, net::UnixStream, process::CommandExt},
    },
    process::{Command, Stdio},
    time::Instant,
};
#[cfg(target_os = "linux")]
use std::{fs::File, io::Write};

fn errno() -> std::io::Error {
    std::io::Error::last_os_error()
}
fn components(p: &Path) -> Vec<CString> {
    p.components()
        .filter_map(|c| match c {
            Component::Normal(s) => {
                let Ok(name) = CString::new(s.as_bytes()) else {
                    unreachable!("validated path")
                };
                Some(name)
            }
            _ => None,
        })
        .collect()
}

/// Descriptor-relative, no-follow directory walk. Called for workspace ONLY
/// after dropping identity. No canonicalize / check-then-open pathname race.
/// SAFETY: names are preallocated C strings; only async-signal-safe syscalls
/// and stack values are used, so this can also run in pre_exec.
pub(super) fn walk(names: &[CString]) -> std::io::Result<OwnedFd> {
    let mut fd = unsafe {
        libc::open(
            c"/".as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(errno());
    }
    for name in names {
        let next = unsafe {
            libc::openat(
                fd,
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        let error = errno();
        unsafe {
            libc::close(fd);
        }
        if next < 0 {
            return Err(error);
        }
        fd = next;
    }
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

fn account(c: &OwnerConfig) -> Result<Vec<OwnedFd>, RunFailure> {
    // NSS lookup/group expansion is deliberately outside pre_exec. Bound both
    // buffers and refuse ERANGE instead of retrying with unbounded allocations.
    let name = CString::new(c.account.as_str()).map_err(|_| {
        RunFailure::new(
            RunError::NotStarted(NotStarted::AccountPolicy),
            ExecError::Invalid("account"),
        )
    })?;
    let mut buffer = vec![0u8; 65536];
    let mut pw = std::mem::MaybeUninit::<libc::passwd>::uninit();
    let mut result = std::ptr::null_mut();
    let rc = unsafe {
        libc::getpwnam_r(
            name.as_ptr(),
            pw.as_mut_ptr(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut result,
        )
    };
    if rc != 0 || result.is_null() {
        return Err(RunFailure::new(
            RunError::NotStarted(NotStarted::AccountPolicy),
            ExecError::Unavailable("pinned account missing or lookup failed"),
        ));
    }
    let pw = unsafe { pw.assume_init() };
    if pw.pw_uid != c.uid || pw.pw_gid != c.gid {
        return Err(RunFailure::new(
            RunError::NotStarted(NotStarted::AccountChanged),
            ExecError::Unavailable("pinned identity changed"),
        ));
    }
    let mut groups = [0; 64];
    let mut count = groups.len() as libc::c_int;
    let rc =
        unsafe { libc::getgrouplist(name.as_ptr(), c.gid as _, groups.as_mut_ptr(), &mut count) };
    if rc < 0
        || count < 1
        || count as usize > groups.len()
        || groups[..count as usize]
            .iter()
            .any(|g| i64::from(*g) != i64::from(c.gid))
    {
        return Err(RunFailure::new(
            RunError::NotStarted(NotStarted::AccountPolicy),
            ExecError::Unavailable("supplementary group membership forbidden"),
        ));
    }
    let mut group = std::mem::MaybeUninit::<libc::group>::uninit();
    let mut found = std::ptr::null_mut();
    let rc = unsafe {
        libc::getgrnam_r(
            name.as_ptr(),
            group.as_mut_ptr(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut found,
        )
    };
    if rc != 0 || found.is_null() || unsafe { group.assume_init() }.gr_gid != c.gid {
        return Err(RunFailure::new(
            RunError::NotStarted(NotStarted::AccountPolicy),
            ExecError::Unavailable("dedicated account-named primary group required"),
        ));
    }
    // Private roots must be actual root-owned no-follow directories, inaccessible
    // to group/other. Owner configuration must enumerate all private roots.
    let mut private_roots = Vec::new();
    for p in &c.protected_directories {
        let fd = trusted_root(p)
            .map_err(|e| RunFailure::os(RunError::NotStarted(NotStarted::PrivateRoots), e))?;
        let mut st = std::mem::MaybeUninit::<libc::stat>::uninit();
        if unsafe { libc::fstat(fd.as_raw_fd(), st.as_mut_ptr()) } != 0 {
            return Err(RunFailure::os(
                RunError::NotStarted(NotStarted::PrivateRoots),
                errno(),
            ));
        }
        let st = unsafe { st.assume_init() };
        if st.st_uid != 0 || st.st_mode & 0o077 != 0 {
            return Err(RunFailure::new(
                RunError::NotStarted(NotStarted::PrivateRoots),
                ExecError::Unavailable("private directory is not root-only"),
            ));
        }
        private_roots.push(fd);
    }
    Ok(private_roots)
}

pub(super) fn run(
    c: &OwnerConfig,
    s: &RunSpec,
    cancel: &Cancellation,
    shutdown: &Cancellation,
    caller_shutdown: Option<&Cancellation>,
) -> Result<RunResult, RunFailure> {
    if unsafe { libc::getuid() } != 0 || unsafe { libc::geteuid() } != 0 {
        return Err(RunFailure::new(
            RunError::NotStarted(NotStarted::ServiceIdentity),
            ExecError::Unavailable("root service identity required; no owner fallback"),
        ));
    }
    let private_roots = account(c)?;
    if let Some(failure) = recheck(
        cancel.is_cancelled(),
        shutdown_requested(
            shutdown.is_cancelled(),
            caller_shutdown.is_some_and(Cancellation::is_cancelled),
        ),
    ) {
        return Err(failure);
    }
    #[cfg(target_os = "macos")]
    let cwd = {
        let mut names = components(&c.workspace);
        names.extend(components(&s.cwd));
        names
    };
    #[cfg(target_os = "linux")]
    let Ok(workspace_path) = CString::new(c.workspace.as_os_str().as_bytes()) else {
        unreachable!("validated path")
    };
    #[cfg(target_os = "linux")]
    let Ok(relative_cwd) = CString::new(if s.cwd.as_os_str().is_empty() {
        b".".as_slice()
    } else {
        s.cwd.as_os_str().as_bytes()
    }) else {
        unreachable!("validated cwd")
    };
    #[cfg(target_os = "macos")]
    let fd_limit = {
        let mut limit = std::mem::MaybeUninit::<libc::rlimit>::uninit();
        if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, limit.as_mut_ptr()) } != 0 {
            return Err(RunFailure::os(
                RunError::NotStarted(NotStarted::Containment),
                errno(),
            ));
        }
        let max = unsafe { limit.assume_init() }.rlim_max;
        if max > 1_048_576 {
            return Err(RunFailure::new(
                RunError::NotStarted(NotStarted::Containment),
                ExecError::Unavailable("descriptor limit exceeds safe inheritance closure"),
            ));
        }
        max as i32
    };
    #[cfg(target_os = "linux")]
    let group = super::cgroup::Group::create(c)
        .map_err(|e| RunFailure::new(RunError::NotStarted(NotStarted::Containment), e))?;
    #[cfg(target_os = "linux")]
    let procs_fd = group.procs.as_raw_fd();
    let (mut observer, reporter) = UnixStream::pair()
        .map_err(|e| RunFailure::os(RunError::NotStarted(NotStarted::Resource), e))?;
    let report_fd = reporter.as_raw_fd();
    let (uid, gid) = (c.uid, c.gid);
    let mut command = Command::new(&s.executable);
    command
        .args(&s.arguments)
        .env_clear()
        .envs(&c.environment)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // SAFETY: preallocated captures only; no NSS, locks, logging, allocation or
    // environment access after fork. Each syscall failure aborts exec.
    unsafe {
        command.pre_exec(move || {
            if libc::setpgid(0, 0) != 0 {
                return Err(errno());
            }
            #[cfg(target_os = "linux")]
            {
                if libc::syscall(
                    libc::SYS_close_range,
                    3u32,
                    u32::MAX,
                    libc::CLOSE_RANGE_CLOEXEC,
                ) != 0
                {
                    return Err(errno());
                }
                // Kernel interprets 0 as the calling task. Join before it can fork.
                if libc::write(procs_fd, b"0".as_ptr().cast(), 1) != 1 {
                    return Err(errno());
                }
                if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 {
                    return Err(errno());
                }
                if libc::prctl(libc::PR_SET_KEEPCAPS, 0, 0, 0, 0) != 0
                    || libc::prctl(
                        libc::PR_CAP_AMBIENT,
                        libc::PR_CAP_AMBIENT_CLEAR_ALL,
                        0,
                        0,
                        0,
                    ) != 0
                {
                    return Err(errno());
                }
            }
            #[cfg(target_os = "macos")]
            for fd in 3..fd_limit {
                if libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) < 0
                    && errno().raw_os_error() != Some(libc::EBADF)
                {
                    return Err(errno());
                }
            }
            // Use setgroups rather than initgroups (NSS/allocator unsafe after fork).
            // Account membership was resolved and rejected unless exactly pinned gid.
            if libc::setgroups(0, std::ptr::null()) != 0 {
                return Err(errno());
            }
            #[cfg(target_os = "linux")]
            if libc::setresgid(gid, gid, gid) != 0 || libc::setresuid(uid, uid, uid) != 0 {
                return Err(errno());
            }
            #[cfg(target_os = "linux")]
            clear_capabilities()?;
            #[cfg(target_os = "macos")]
            if libc::setgid(gid) != 0 || libc::setuid(uid) != 0 {
                return Err(errno());
            }
            if libc::getuid() != uid
                || libc::geteuid() != uid
                || libc::getgid() != gid
                || libc::getegid() != gid
            {
                return Err(std::io::Error::from_raw_os_error(libc::EPERM));
            }
            let mut observed_groups = [0 as libc::gid_t; 64];
            let count = libc::getgroups(64, observed_groups.as_mut_ptr());
            // Some Unix kernels include the effective primary group in getgroups.
            if !(0..=64).contains(&count)
                || observed_groups[..count as usize].iter().any(|g| *g != gid)
            {
                return Err(std::io::Error::from_raw_os_error(libc::EPERM));
            }
            // Actual child-identity search-permission probe, including OS ACLs.
            // Holding a root-opened directory FD does not bypass fchdir's check.
            for root in &private_roots {
                if libc::fchdir(root.as_raw_fd()) == 0
                    || !matches!(errno().raw_os_error(), Some(libc::EACCES | libc::EPERM))
                {
                    return Err(std::io::Error::from_raw_os_error(libc::EPERM));
                }
            }
            let no_core = libc::rlimit {
                rlim_cur: 0,
                rlim_max: 0,
            };
            if libc::setrlimit(libc::RLIMIT_CORE, &no_core) != 0 {
                return Err(errno());
            }
            libc::umask(0o077);
            #[cfg(target_os = "macos")]
            let directory = walk(&cwd)?;
            #[cfg(target_os = "linux")]
            let directory = workspace_cwd(&workspace_path, &relative_cwd)?;
            if libc::fchdir(directory.as_raw_fd()) != 0 {
                return Err(errno());
            }
            // Observed, not configured, IDs. CLOEXEC closes reporter on successful exec.
            let observed = [libc::getuid(), libc::getgid()];
            if libc::write(report_fd, observed.as_ptr().cast(), 8) != 8 {
                return Err(errno());
            }
            Ok(())
        });
    }
    let started = Instant::now();
    // ADR 0019 C3: retain the early check and close the final race window.
    if let Some(failure) = recheck(
        cancel.is_cancelled(),
        shutdown_requested(
            shutdown.is_cancelled(),
            caller_shutdown.is_some_and(Cancellation::is_cancelled),
        ),
    ) {
        return Err(failure);
    }
    let child = command
        .spawn()
        .map_err(|e| RunFailure::os(RunError::SpawnFailed, e))?;
    drop(command);
    drop(reporter);
    #[cfg(target_os = "linux")]
    let boundary = TreeBoundary::LinuxCgroupV2;
    #[cfg(target_os = "macos")]
    let boundary = TreeBoundary::MacProcessGroupBestEffort;
    let mut guard = ChildGuard {
        child,
        cleanup: None,
        #[cfg(target_os = "linux")]
        group,
    };
    let mut observation = [0u8; 8];
    observer
        .read_exact(&mut observation)
        .map_err(|e| RunFailure::os(RunError::AfterSpawn, e))?;
    let observed_uid = u32::from_ne_bytes([
        observation[0],
        observation[1],
        observation[2],
        observation[3],
    ]);
    let observed_gid = u32::from_ne_bytes([
        observation[4],
        observation[5],
        observation[6],
        observation[7],
    ]);
    if observed_uid != uid || observed_gid != gid {
        return Err(RunFailure::new(
            RunError::AfterSpawn,
            ExecError::Unavailable("child identity attestation mismatch"),
        ));
    }
    let out = guard.child.stdout.take().ok_or_else(|| {
        RunFailure::new(RunError::AfterSpawn, ExecError::Unavailable("stdout pipe"))
    })?;
    let err = guard.child.stderr.take().ok_or_else(|| {
        RunFailure::new(RunError::AfterSpawn, ExecError::Unavailable("stderr pipe"))
    })?;
    nonblocking(out.as_raw_fd()).map_err(|e| RunFailure::os(RunError::AfterSpawn, e))?;
    nonblocking(err.as_raw_fd()).map_err(|e| RunFailure::os(RunError::AfterSpawn, e))?;
    let (mut out, mut err) = (out, err);
    let (mut stdout, mut stderr) = (Capture::new(s.output_bytes), Capture::new(s.output_bytes));
    let outcome = loop {
        drain(&mut out, &mut stdout).map_err(|e| RunFailure::os(RunError::AfterSpawn, e))?;
        drain(&mut err, &mut stderr).map_err(|e| RunFailure::os(RunError::AfterSpawn, e))?;
        if let Some(reason) = stop_reason(
            started.elapsed(),
            s.timeout,
            cancel.is_cancelled(),
            shutdown_requested(
                shutdown.is_cancelled(),
                caller_shutdown.is_some_and(Cancellation::is_cancelled),
            ),
        ) {
            break reason;
        }
        if exited_without_reaping(guard.child.id())
            .map_err(|e| RunFailure::os(RunError::AfterSpawn, e))?
        {
            break Outcome::Exited;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let cleaned = guard.kill_tree();
    // Bounded reaping: a task stuck in uninterruptible kernel sleep must not hang
    // daemon shutdown. Never report tree cleanup confirmed in that case.
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut status = None;
    while status.is_none() && Instant::now() < deadline {
        status = guard
            .child
            .try_wait()
            .map_err(|e| RunFailure::os(RunError::AfterSpawn, e))?;
        if status.is_none() {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    drain(&mut out, &mut stdout).map_err(|e| RunFailure::os(RunError::AfterSpawn, e))?;
    drain(&mut err, &mut stderr).map_err(|e| RunFailure::os(RunError::AfterSpawn, e))?;
    Ok(RunResult {
        outcome,
        exit_code: status.and_then(|x| x.code()),
        identity: ObservedIdentity {
            uid: observed_uid,
            gid: observed_gid,
            daemon_was_root: true,
        },
        tree_boundary: boundary,
        tree_cleanup_confirmed: cleaned && status.is_some(),
        stdout: stdout.finish(),
        stderr: stderr.finish(),
    })
}

fn nonblocking(fd: libc::c_int) -> std::io::Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(errno());
    }
    Ok(())
}
fn drain(r: &mut impl Read, capture: &mut Capture) -> std::io::Result<()> {
    // A flooding stream cannot starve stderr, cancellation or the deadline.
    let mut buf = [0; 8192];
    for _ in 0..16 {
        match r.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => capture.push(&buf[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(())
}
struct ChildGuard {
    child: std::process::Child,
    cleanup: Option<bool>,
    #[cfg(target_os = "linux")]
    group: super::cgroup::Group,
}
impl ChildGuard {
    fn kill_tree(&mut self) -> bool {
        if let Some(result) = self.cleanup {
            return result;
        }
        #[cfg(target_os = "linux")]
        let result = self.group.kill_and_confirm();
        #[cfg(target_os = "macos")]
        let result = {
            unsafe {
                libc::kill(-(self.child.id() as i32), libc::SIGKILL);
            }
            let _ = self.child.kill();
            false
        };
        self.cleanup = Some(result);
        result
    }
}
impl Drop for ChildGuard {
    fn drop(&mut self) {
        self.kill_tree();
        let _ = self.child.try_wait();
    }
}

// Keep privileged syscall operations here; no root workspace File::open calls.
#[cfg(target_os = "linux")]
pub(super) fn control(fd: i32, name: &std::ffi::CStr, write: bool) -> std::io::Result<File> {
    let fd = unsafe {
        libc::openat(
            fd,
            name.as_ptr(),
            (if write {
                libc::O_WRONLY
            } else {
                libc::O_RDONLY
            }) | libc::O_NOFOLLOW
                | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        Err(errno())
    } else {
        Ok(unsafe { File::from_raw_fd(fd) })
    }
}
#[cfg(target_os = "linux")]
pub(super) fn put(fd: i32, name: &std::ffi::CStr, value: &[u8]) -> std::io::Result<()> {
    control(fd, name, true)?.write_all(value)
}
pub(super) fn trusted_root(path: &Path) -> std::io::Result<OwnedFd> {
    let names = components(path);
    // Every ancestor must be protected too, preventing directory replacement.
    for end in 0..=names.len() {
        let fd = walk(&names[..end])?;
        let mut st = std::mem::MaybeUninit::<libc::stat>::uninit();
        if unsafe { libc::fstat(fd.as_raw_fd(), st.as_mut_ptr()) } != 0 {
            return Err(errno());
        }
        let st = unsafe { st.assume_init() };
        if st.st_uid != 0 || st.st_mode & 0o022 != 0 {
            return Err(std::io::Error::from_raw_os_error(libc::EPERM));
        }
    }
    walk(&names)
}

fn exited_without_reaping(pid: u32) -> std::io::Result<bool> {
    let mut info = std::mem::MaybeUninit::<libc::siginfo_t>::zeroed();
    if unsafe {
        libc::waitid(
            libc::P_PID,
            pid,
            info.as_mut_ptr(),
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    } != 0
    {
        return Err(errno());
    }
    Ok(unsafe { info.assume_init() }.si_signo != 0)
}

#[cfg(target_os = "linux")]
fn workspace_cwd(root: &std::ffi::CStr, relative: &std::ffi::CStr) -> std::io::Result<OwnedFd> {
    #[repr(C)]
    struct OpenHow {
        flags: u64,
        mode: u64,
        resolve: u64,
    }
    let open = |fd: i32, path: &std::ffi::CStr, resolve: u64| {
        let how = OpenHow {
            flags: (libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC) as u64,
            mode: 0,
            resolve,
        };
        let raw = unsafe {
            libc::syscall(
                libc::SYS_openat2,
                fd,
                path.as_ptr(),
                &how,
                std::mem::size_of::<OpenHow>(),
            )
        };
        if raw < 0 {
            Err(errno())
        } else {
            Ok(unsafe { OwnedFd::from_raw_fd(raw as i32) })
        }
    };
    // No symlinks/magiclinks while opening owner root; then atomic beneath
    // resolution for cwd. Kernel EAGAIN/ENOSYS is a refusal, never a fallback.
    let root = open(libc::AT_FDCWD, root, 0x04)?;
    open(root.as_raw_fd(), relative, 0x04 | 0x08)
}

#[cfg(target_os = "linux")]
fn clear_capabilities() -> std::io::Result<()> {
    #[repr(C)]
    struct Header {
        version: u32,
        pid: i32,
    }
    let header = Header {
        version: 0x20080522,
        pid: 0,
    };
    // Two v3 __user_cap_data_struct entries: effective, permitted, inheritable.
    let mut data = [0u32; 6];
    if unsafe { libc::syscall(libc::SYS_capset, &header, data.as_ptr()) } != 0
        || unsafe { libc::syscall(libc::SYS_capget, &header, data.as_mut_ptr()) } != 0
    {
        return Err(errno());
    }
    if data != [0; 6] {
        return Err(std::io::Error::from_raw_os_error(libc::EPERM));
    }
    Ok(())
}
