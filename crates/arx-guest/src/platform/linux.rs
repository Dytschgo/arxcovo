use std::cell::Cell;
use std::fs::{self, File, OpenOptions};
use std::io::{self, PipeReader, PipeWriter, Read, Seek, SeekFrom, Write};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt};
use std::os::unix::process::ExitStatusExt;
use std::path::{Component, Path, PathBuf};
use std::process::ExitStatus;
use std::thread;
use std::time::{Duration, Instant};

use arx_guest::{MAX_SCRIPT, ScriptExecution, ScriptExecutor, ScriptOutcome, serve};

const PORT: &str = "org.arxcovo.agent.0";
const CGROUP_MOUNT: &str = "/sys/fs/cgroup";
const SERVICE: &str = "arx-guest.service";
const SUPERVISOR: &str = "arx-supervisor";
const COMMAND_SLOT: &str = "arx-command";
const PIDS_MAX: &[u8] = b"32";
const SCRIPT_UID: libc::uid_t = 65534;
const SCRIPT_GID: libc::gid_t = 65534;
const POLL_INTERVAL: Duration = Duration::from_millis(5);
const CLEANUP_BUDGET: Duration = Duration::from_secs(1);

pub(crate) fn serve_virtio() -> Result<(), &'static str> {
    let port = open_port().map_err(|_| "virtio setup failed")?;
    let runner = ReapOwner::establish().ok().and_then(|owner| {
        CgroupRoot::qualify()
            .ok()
            .map(|root| ScriptRunner::new(root, Some(port.as_fd()), owner))
    });
    serve_connection(&port, &port, runner)
}

fn open_port() -> io::Result<File> {
    // udev supplies the symlink. Open its resolved /dev node only after checking
    // the kernel's identity, and check the opened descriptor against it again.
    let path = fs::canonicalize(Path::new("/dev/virtio-ports").join(PORT))?;
    let metadata = path.metadata()?;
    if path.parent() != Some(Path::new("/dev")) || !metadata.file_type().is_char_device() {
        return Err(invalid());
    }
    let device = metadata.rdev();
    verify_port_name(device)?;
    let port = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NOCTTY | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)?;
    let opened = port.metadata()?;
    if !opened.file_type().is_char_device() || opened.rdev() != device {
        return Err(invalid());
    }
    verify_port_name(opened.rdev())?;
    // SAFETY: the open File owns this descriptor for both fcntl calls.
    unsafe {
        let fd = port.as_raw_fd();
        let flags = libc::fcntl(fd, libc::F_GETFL);
        if flags == -1 || libc::fcntl(fd, libc::F_SETFL, flags & !libc::O_NONBLOCK) == -1 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(port)
}

fn verify_port_name(device: libc::dev_t) -> io::Result<()> {
    let sysfs =
        Path::new("/sys/dev/char").join(format!("{}:{}", libc::major(device), libc::minor(device)));
    if fs::canonicalize(sysfs.join("subsystem"))? != Path::new("/sys/class/virtio-ports")
        || bounded_read(File::open(sysfs.join("name"))?, 128)?.trim_end_matches('\n') != PORT
    {
        return Err(invalid());
    }
    Ok(())
}

fn serve_connection(
    reader: impl Read,
    writer: impl Write,
    runner: Option<ScriptRunner<'_>>,
) -> Result<(), &'static str> {
    let retired = Cell::new(false);
    let mut executor = LinuxExecutor {
        runner,
        retired: &retired,
    };
    let result = serve(
        reader,
        SessionWriter {
            writer,
            retired: &retired,
        },
        &mut executor,
    );
    if retired.get() {
        return Err("script containment failed");
    }
    result.map_err(|_| "guest protocol failed")
}

struct SessionWriter<'a, W> {
    writer: W,
    retired: &'a Cell<bool>,
}

impl<W: Write> Write for SessionWriter<'_, W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        // The portable executor API requires an outcome. A retired session
        // rejects even the frame prefix so a placeholder never reaches the peer.
        if self.retired.get() {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        self.writer.write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}

struct LinuxExecutor<'a> {
    runner: Option<ScriptRunner<'a>>,
    retired: &'a Cell<bool>,
}

impl ScriptExecutor for LinuxExecutor<'_> {
    fn script_execution(&self) -> ScriptExecution {
        if self.runner.is_some() {
            ScriptExecution::Available
        } else {
            ScriptExecution::Unavailable
        }
    }

    fn run_script(&mut self, script: &str, timeout_s: u32) -> ScriptOutcome {
        let Some(runner) = &self.runner else {
            return ScriptOutcome::Unavailable;
        };
        match runner.run(script, Duration::from_secs(timeout_s.into())) {
            Ok(outcome) => outcome,
            Err(_) => {
                self.retired.set(true);
                ScriptOutcome::LaunchFailed
            }
        }
    }
}

#[derive(Debug)]
enum Qualification {
    SupervisorIdentity,
    Delegation,
    CommandSlot,
}

struct CgroupRoot {
    path: PathBuf,
    owner: u32,
}

impl CgroupRoot {
    fn qualify() -> Result<Self, Qualification> {
        // The Ubuntu unit runs a root supervisor in a leaf via Delegate=pids
        // and DelegateSubgroup=arx-supervisor. The shell runs as nobody, with
        // no access to the supervisor's cgroup controls or serial descriptor.
        if effective_uid() != 0 {
            return Err(Qualification::SupervisorIdentity);
        }
        Self::discover(SERVICE, 0)
    }

    fn discover(service: &str, owner: u32) -> Result<Self, Qualification> {
        let supervisor = supervisor_path(service).map_err(|_| Qualification::Delegation)?;
        let parent = supervisor.parent().ok_or(Qualification::Delegation)?;
        verify_group(&supervisor, owner).map_err(|_| Qualification::Delegation)?;
        verify_group(parent, owner).map_err(|_| Qualification::Delegation)?;
        let procs =
            File::open(supervisor.join("cgroup.procs")).map_err(|_| Qualification::Delegation)?;
        let members = bounded_read(procs, 128).map_err(|_| Qualification::Delegation)?;
        if members.trim().parse::<u32>() != Ok(std::process::id()) {
            return Err(Qualification::Delegation);
        }
        let mut subtree = control(parent, "cgroup.subtree_control", owner)
            .map_err(|_| Qualification::Delegation)?;
        subtree
            .write_all(b"+pids")
            .map_err(|_| Qualification::Delegation)?;
        let root = Self {
            path: parent.to_owned(),
            owner,
        };
        // A stale slot is deliberately left in place and disables capability.
        // mkdir must establish that this slot was made by this invocation.
        root.new_command()
            .and_then(|mut group| {
                group.kill()?;
                group.wait_empty(Instant::now() + CLEANUP_BUDGET)?;
                root.remove_command(&group)
            })
            .map_err(|_| Qualification::CommandSlot)?;
        Ok(root)
    }
}

trait CommandCgroups {
    fn new_command(&self) -> io::Result<CommandGroup>;
    fn remove_command(&self, group: &CommandGroup) -> io::Result<()>;
}

impl CommandCgroups for CgroupRoot {
    fn new_command(&self) -> io::Result<CommandGroup> {
        let path = self.path.join(COMMAND_SLOT);
        fs::create_dir(&path)?;
        let group = CommandGroup::open(&path, self.owner);
        if group.is_err() {
            // No child has been started and no owned process could have joined.
            let _ = fs::remove_dir(&path);
        }
        group
    }

    fn remove_command(&self, group: &CommandGroup) -> io::Result<()> {
        fs::remove_dir(&group.path)
    }
}

struct ScriptRunner<'port> {
    cgroups: Box<dyn CommandCgroups>,
    port: Option<BorrowedFd<'port>>,
    reap_owner: ReapOwner,
    max_budget: Duration,
    cleanup_budget: Duration,
    drop_privileges: bool,
    #[cfg(test)]
    stall: Option<&'port StallWatchdog>,
}

impl<'port> ScriptRunner<'port> {
    fn new(root: CgroupRoot, port: Option<BorrowedFd<'port>>, reap_owner: ReapOwner) -> Self {
        Self {
            drop_privileges: root.owner == 0,
            cgroups: Box::new(root),
            port,
            reap_owner,
            max_budget: Duration::from_secs(arx_guest::MAX_TIMEOUT_S.into()),
            cleanup_budget: CLEANUP_BUDGET,
            #[cfg(test)]
            stall: None,
        }
    }

    fn run(&self, script: &str, budget: Duration) -> io::Result<ScriptOutcome> {
        let deadline = Instant::now() + budget.min(self.max_budget);
        #[cfg(test)]
        let deadline = if self.stall.is_some() {
            Instant::now() + TEST_HANG_GUARD
        } else {
            deadline
        };
        let mut group = self.cgroups.new_command()?;
        let prepared = PreparedScript::new(script);
        let launch_expired = Instant::now() >= deadline;
        let mut child = if launch_expired {
            None
        } else {
            prepared
                .and_then(|script| {
                    script.fork(
                        &self.reap_owner,
                        ChildSetup {
                            port: self.port.map(|port| port.as_raw_fd()),
                            procs: Some(group.procs.as_raw_fd()),
                            drop_privileges: self.drop_privileges,
                            #[cfg(test)]
                            stdout: None,
                            #[cfg(test)]
                            stall: self.stall.map(|stall| stall.child),
                        },
                    )
                })
                .ok()
        };
        let mut launch_uncertain = false;
        let result = match &mut child {
            Some(child) => match self.wait_exec(child, deadline) {
                Ok(true) => observe(child, deadline),
                Ok(false) => Ok(Observation::LaunchFailed),
                Err(error) => {
                    launch_uncertain = true;
                    Err(error)
                }
            },
            None if launch_expired => Err(io::ErrorKind::TimedOut.into()),
            None => Ok(Observation::LaunchFailed),
        };
        let cleanup_deadline = Instant::now() + self.cleanup_budget;
        // An uncertain launch may precede setpgid or the cgroup join. Stop
        // the owned child as well as its group before killing the cgroup.
        let group_kill = match &child {
            Some(child) if launch_uncertain || matches!(result, Ok(Observation::Deadline)) => {
                child.kill()
            }
            _ => Ok(()),
        };
        let cgroup_kill = group.kill();
        let reap = match child.as_mut() {
            Some(child) => reap(child, cleanup_deadline),
            None => Ok(()),
        };
        let empty = group.wait_empty(cleanup_deadline);
        group_kill?;
        cgroup_kill?;
        reap?;
        empty?;
        self.cgroups.remove_command(&group)?;
        Ok(result?.outcome())
    }

    fn wait_exec(&self, child: &mut ScriptChild, deadline: Instant) -> io::Result<bool> {
        #[cfg(test)]
        if let Some(stall) = self.stall {
            return stall.expire_after_ready(deadline);
        }
        child.wait_exec(deadline)
    }
}

// The dedicated service has one waiter and installs no other SIGCHLD handler.
// With default SIGCHLD and no SA_NOCLDWAIT, even an exited child retains its
// PID until that waiter reaps it.
struct ReapOwner;

impl ReapOwner {
    fn establish() -> io::Result<Self> {
        // SAFETY: sigaction accepts a live, initialized action and copies it.
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = libc::SIG_DFL;
            if libc::sigemptyset(&mut action.sa_mask) != 0
                || libc::sigaction(libc::SIGCHLD, &action, std::ptr::null_mut()) != 0
            {
                return Err(io::Error::last_os_error());
            }
        }
        Self::verify()
    }

    fn verify() -> io::Result<Self> {
        let action = sigchld_action()?;
        if action.sa_sigaction != libc::SIG_DFL || action.sa_flags & libc::SA_NOCLDWAIT != 0 {
            return Err(invalid());
        }
        Ok(Self)
    }
}

fn sigchld_action() -> io::Result<libc::sigaction> {
    // SAFETY: sigaction initializes the output on a successful read-back.
    unsafe {
        let mut action = std::mem::MaybeUninit::uninit();
        if libc::sigaction(libc::SIGCHLD, std::ptr::null(), action.as_mut_ptr()) != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(action.assume_init())
    }
}

fn supervisor_path(service: &str) -> io::Result<PathBuf> {
    let cgroups = bounded_read(File::open("/proc/self/cgroup")?, 8192)?;
    let relative = cgroups
        .lines()
        .find_map(|line| line.strip_prefix("0::/"))
        .ok_or_else(invalid)?;
    let path = Path::new(relative);
    if !path.components().all(|c| matches!(c, Component::Normal(_)))
        || !path.ends_with(Path::new(service).join(SUPERVISOR))
    {
        return Err(invalid());
    }
    Ok(Path::new(CGROUP_MOUNT).join(path))
}

fn verify_group(path: &Path, owner: u32) -> io::Result<()> {
    if fs::canonicalize(path)? != path {
        return Err(invalid());
    }
    let dir = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(path)?;
    let metadata = dir.metadata()?;
    // SAFETY: fstatfs initializes the pointed-to storage on success, and dir
    // holds the open descriptor until the call returns.
    let kind = unsafe {
        let mut stat = std::mem::MaybeUninit::<libc::statfs>::uninit();
        if libc::fstatfs(dir.as_raw_fd(), stat.as_mut_ptr()) != 0 {
            return Err(io::Error::last_os_error());
        }
        stat.assume_init().f_type
    };
    if kind != libc::CGROUP2_SUPER_MAGIC || metadata.uid() != owner || metadata.mode() & 0o022 != 0
    {
        return Err(invalid());
    }
    Ok(())
}

fn control(parent: &Path, name: &str, owner: u32) -> io::Result<File> {
    let file = OpenOptions::new()
        .write(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(parent.join(name))?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.uid() != owner || metadata.mode() & 0o022 != 0 {
        return Err(invalid());
    }
    Ok(file)
}

struct CommandGroup {
    path: PathBuf,
    procs: File,
    kill: File,
    events: File,
}

impl CommandGroup {
    fn open(path: &Path, owner: u32) -> io::Result<Self> {
        verify_group(path, owner)?;
        let pids = path.join("pids.max");
        control(path, "pids.max", owner)?.write_all(PIDS_MAX)?;
        if bounded_read(File::open(pids)?, 128)?.trim_end().as_bytes() != PIDS_MAX {
            return Err(invalid());
        }
        let mut group = Self::open_controls(path, owner)?;
        if group.populated()? {
            return Err(invalid());
        }
        Ok(group)
    }

    fn open_controls(path: &Path, owner: u32) -> io::Result<Self> {
        Ok(Self {
            path: path.to_owned(),
            procs: control(path, "cgroup.procs", owner)?,
            kill: control(path, "cgroup.kill", owner)?,
            events: File::open(path.join("cgroup.events"))?,
        })
    }

    fn populated(&mut self) -> io::Result<bool> {
        self.events.seek(SeekFrom::Start(0))?;
        let events = bounded_read(&self.events, 1024)?;
        let mut values = events
            .lines()
            .filter_map(|line| line.strip_prefix("populated "));
        match (values.next(), values.next()) {
            (Some("0"), None) => Ok(false),
            (Some("1"), None) => Ok(true),
            _ => Err(invalid()),
        }
    }

    fn kill(&mut self) -> io::Result<()> {
        self.kill.write_all(b"1")
    }

    fn wait_empty(&mut self, deadline: Instant) -> io::Result<()> {
        while self.populated()? {
            pause_until(deadline)?;
        }
        Ok(())
    }
}

struct PreparedScript {
    input: PipeReader,
    null: File,
    exec_reader: PipeReader,
    exec_writer: PipeWriter,
}

#[derive(Default)]
struct ChildSetup {
    port: Option<RawFd>,
    procs: Option<RawFd>,
    drop_privileges: bool,
    #[cfg(test)]
    stdout: Option<RawFd>,
    #[cfg(test)]
    stall: Option<StallBeforeExec>,
}

#[cfg(test)]
#[derive(Clone, Copy)]
struct StallBeforeExec {
    ready: RawFd,
    gate: RawFd,
}

#[cfg(test)]
const TEST_HANG_GUARD: Duration = Duration::from_secs(5);

#[cfg(test)]
struct StallWatchdog {
    child: StallBeforeExec,
    ready: PipeReader,
    receipt: Cell<Option<(libc::pid_t, u8)>>,
}

#[cfg(test)]
impl StallWatchdog {
    fn expire_after_ready(&self, deadline: Instant) -> io::Result<bool> {
        wait_readable(self.ready.as_raw_fd(), deadline)?;
        let mut receipt = [0; 5];
        (&self.ready).read_exact(&mut receipt)?;
        let [a, b, c, d, inherited] = receipt;
        self.receipt
            .set(Some((libc::pid_t::from_ne_bytes([a, b, c, d]), inherited)));
        // Inject the watchdog event only after the whole pre-exec record.
        Err(io::ErrorKind::TimedOut.into())
    }
}

impl PreparedScript {
    fn new(script: &str) -> io::Result<Self> {
        if script.len() > MAX_SCRIPT {
            return Err(invalid());
        }
        let (input, mut writer) = io::pipe()?;
        // SAFETY: writer owns the pipe descriptor. F_GETPIPE_SZ takes no argument.
        let capacity = unsafe { libc::fcntl(writer.as_raw_fd(), libc::F_GETPIPE_SZ) };
        if !usize::try_from(capacity).is_ok_and(|capacity| capacity >= script.len()) {
            return Err(invalid());
        }
        // The private pipe has room for the entire script before a reader runs.
        writer.write_all(script.as_bytes())?;
        drop(writer);
        let (exec_reader, exec_writer) = io::pipe()?;
        Ok(Self {
            input,
            null: File::options().write(true).open("/dev/null")?,
            exec_reader,
            exec_writer,
        })
    }

    fn fork(self, _owner: &ReapOwner, setup: ChildSetup) -> io::Result<ScriptChild> {
        let input = self.input.as_raw_fd();
        let null = self.null.as_raw_fd();
        let report = self.exec_writer.as_raw_fd();
        let zero: libc::c_ulong = 0;
        let flags = libc::SIGCHLD as libc::c_ulong;
        // s390 passes the child stack before the flags. All other arguments
        // are zero because this is a separate process with copied descriptors.
        let (first, second) = if cfg!(target_arch = "s390x") {
            (zero, flags)
        } else {
            (flags, zero)
        };
        // SAFETY: fork-style clone (no shared stack, memory, or FD table).
        // The child uses only async-signal-safe operations and never returns
        // through Rust or libc fork hooks. The first child syscall closes the
        // port. Before that close there is only local control flow, and the
        // supervisor's File keeps this close from being the final release.
        let pid = unsafe {
            let pid = libc::syscall(libc::SYS_clone, first, second, zero, zero, zero);
            if pid == 0 {
                if let Some(port) = setup.port
                    && libc::syscall(libc::SYS_close, libc::c_long::from(port)) != 0
                {
                    launch_failed(report);
                }
                exec_child(setup, input, null, report);
            }
            pid
        };
        if pid < 0 {
            return Err(io::Error::last_os_error());
        }
        drop(self.exec_writer);
        Ok(ScriptChild::new(pid as libc::pid_t, self.exec_reader))
    }
}

// SAFETY: fork child only. All descriptors are inherited and live, and all
// strings and buffers have fixed size. No allocation, lock or Rust unwind.
unsafe fn exec_child(setup: ChildSetup, input: RawFd, null: RawFd, report: RawFd) -> ! {
    unsafe {
        #[cfg(test)]
        if let Some(stall) = setup.stall {
            let [a, b, c, d] = libc::getpid().to_ne_bytes();
            let inherited = u8::from(
                setup
                    .port
                    .is_some_and(|port| libc::fcntl(port, libc::F_GETFD) != -1),
            );
            let ready = [a, b, c, d, inherited];
            libc::write(stall.ready, ready.as_ptr().cast(), ready.len());
            // Bound even a test whose supervisor regresses and never kills it.
            let mut gate = libc::pollfd {
                fd: stall.gate,
                events: libc::POLLIN,
                revents: 0,
            };
            if libc::poll(&mut gate, 1, 10_000) <= 0 {
                libc::_exit(124);
            }
            libc::read(stall.gate, [0u8].as_mut_ptr().cast(), 1);
        }

        #[cfg(not(test))]
        let stdout = null;
        #[cfg(test)]
        let stdout = setup.stdout.unwrap_or(null);
        let one: libc::c_ulong = 1;
        let zero: libc::c_ulong = 0;
        let gid = libc::c_ulong::from(SCRIPT_GID);
        let uid = libc::c_ulong::from(SCRIPT_UID);
        // Raw credential syscalls avoid libc's setxid locks and broadcasts.
        let failed = libc::setpgid(0, 0) != 0
            || setup
                .procs
                .is_some_and(|fd| libc::write(fd, b"0".as_ptr().cast(), 1) != 1)
            || libc::signal(libc::SIGPIPE, libc::SIG_DFL) == libc::SIG_ERR
            || libc::dup2(input, libc::STDIN_FILENO) < 0
            || libc::dup2(stdout, libc::STDOUT_FILENO) < 0
            || libc::dup2(null, libc::STDERR_FILENO) < 0
            || libc::chdir(c"/".as_ptr()) != 0
            || (setup.drop_privileges
                && (libc::syscall(libc::SYS_setgroups, zero, std::ptr::null::<libc::gid_t>())
                    != 0
                    || libc::syscall(libc::SYS_setresgid, gid, gid, gid) != 0
                    || libc::syscall(libc::SYS_setresuid, uid, uid, uid) != 0))
            || libc::prctl(libc::PR_SET_NO_NEW_PRIVS, one, zero, zero, zero) != 0;
        if !failed {
            let shell = c"/bin/sh".as_ptr();
            let args = [shell, c"-s".as_ptr(), std::ptr::null()];
            let env = [
                c"PATH=/usr/bin:/bin".as_ptr(),
                c"LANG=C".as_ptr(),
                std::ptr::null(),
            ];
            libc::execve(shell, args.as_ptr(), env.as_ptr());
        }
        launch_failed(report);
    }
}

// SAFETY: the reporting pipe is private, empty and has a live reader during
// launch (or returns EPIPE after retirement). The child must never unwind.
unsafe fn launch_failed(report: RawFd) -> ! {
    unsafe {
        libc::write(report, b"1".as_ptr().cast(), 1);
        libc::_exit(127);
    }
}

struct ScriptChild {
    pid: libc::pid_t,
    pidfd: Option<OwnedFd>,
    pidfd_error: Option<io::Error>,
    exec_reader: PipeReader,
    status: Option<ExitStatus>,
}

impl ScriptChild {
    fn new(pid: libc::pid_t, exec_reader: PipeReader) -> Self {
        let (pidfd, pidfd_error) = match open_pidfd(pid) {
            Ok(fd) => (fd, None),
            Err(error) => (None, Some(error)),
        };
        Self {
            pid,
            pidfd,
            pidfd_error,
            exec_reader,
            status: None,
        }
    }

    fn wait_exec(&mut self, deadline: Instant) -> io::Result<bool> {
        if let Some(error) = self.pidfd_error.take() {
            return Err(error);
        }
        wait_readable(self.exec_reader.as_raw_fd(), deadline)?;
        if Instant::now() >= deadline {
            return Err(io::ErrorKind::TimedOut.into());
        }
        // The child writes one byte on failure. Exec closes the writer.
        Ok(self.exec_reader.read(&mut [0])? == 0)
    }

    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        if self.status.is_none() {
            self.status = self.wait_status(0)?;
        }
        Ok(self.status)
    }

    fn wait_status(&self, flags: libc::c_int) -> io::Result<Option<ExitStatus>> {
        if let Some(pidfd) = &self.pidfd {
            match wait_child(libc::P_PIDFD, pidfd.as_raw_fd() as libc::id_t, flags) {
                // Linux 5.3 introduced pidfd_open before P_PIDFD wait.
                Err(error) if error.raw_os_error() == Some(libc::EINVAL) => {}
                result => return result,
            }
        }
        // The sole waiter owns the numeric PID until it reaps the child.
        wait_child(libc::P_PID, self.pid as libc::id_t, flags)
    }

    fn kill(&self) -> io::Result<()> {
        // WNOWAIT proves child ownership before the numeric group signal,
        // without giving up the PID. This service has no competing waiter.
        if self.status.is_some() || self.wait_status(libc::WNOWAIT)?.is_some() {
            return Ok(());
        }
        let group = numeric_kill(-self.pid);
        let direct = match &self.pidfd {
            Some(pidfd) => {
                // SAFETY: this live descriptor identifies our forked child.
                let sent = unsafe {
                    libc::syscall(
                        libc::SYS_pidfd_send_signal,
                        libc::c_long::from(pidfd.as_raw_fd()),
                        libc::c_long::from(libc::SIGKILL),
                        std::ptr::null::<libc::siginfo_t>(),
                        0 as libc::c_ulong,
                    )
                };
                kill_result(sent)
            }
            None => numeric_kill(self.pid),
        };
        group.and(direct)
    }
}

fn open_pidfd(pid: libc::pid_t) -> io::Result<Option<OwnedFd>> {
    // SAFETY: the caller has just forked this waitable child. The returned
    // descriptor belongs to the caller and Linux opens it close-on-exec.
    let fd = unsafe {
        libc::syscall(
            libc::SYS_pidfd_open,
            libc::c_long::from(pid),
            0 as libc::c_ulong,
        )
    };
    if fd >= 0 {
        // SAFETY: a successful pidfd_open returns a fresh descriptor.
        return Ok(Some(unsafe { OwnedFd::from_raw_fd(fd as RawFd) }));
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ENOSYS) {
        return Ok(None);
    }
    Err(error)
}

fn wait_child(
    kind: libc::idtype_t,
    id: libc::id_t,
    flags: libc::c_int,
) -> io::Result<Option<ExitStatus>> {
    loop {
        // SAFETY: siginfo is valid writable storage. waitid with WNOHANG
        // leaves si_pid zero when the child has no reportable exit.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let waited =
            unsafe { libc::waitid(kind, id, &mut info, libc::WEXITED | libc::WNOHANG | flags) };
        if waited == 0 {
            // SAFETY: WEXITED reports SIGCHLD status information.
            return unsafe {
                if info.si_pid() == 0 {
                    Ok(None)
                } else {
                    let status = info.si_status();
                    let raw = match info.si_code {
                        libc::CLD_EXITED => status << 8,
                        libc::CLD_KILLED => status,
                        libc::CLD_DUMPED => status | 0x80,
                        _ => return Err(invalid()),
                    };
                    Ok(Some(ExitStatus::from_raw(raw)))
                }
            };
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

fn wait_readable(fd: RawFd, deadline: Instant) -> io::Result<()> {
    let mut poll = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(io::ErrorKind::TimedOut.into());
        }
        let ms = (remaining.as_millis() + 1).min(i32::MAX as u128) as i32;
        // SAFETY: poll is a single initialized descriptor record.
        match unsafe { libc::poll(&mut poll, 1, ms) } {
            -1 => {
                let error = io::Error::last_os_error();
                if error.kind() != io::ErrorKind::Interrupted {
                    return Err(error);
                }
            }
            0 => {}
            _ if poll.revents & (libc::POLLIN | libc::POLLHUP) != 0 => return Ok(()),
            _ => return Err(invalid()),
        }
    }
}

enum Observation {
    Exited(ExitStatus),
    Deadline,
    LaunchFailed,
}

impl Observation {
    fn outcome(self) -> ScriptOutcome {
        match self {
            Self::Exited(status) => {
                status
                    .code()
                    .map_or(ScriptOutcome::Terminated, |code| ScriptOutcome::Exited {
                        exit_code: code as u32,
                    })
            }
            Self::Deadline => ScriptOutcome::TimedOut,
            Self::LaunchFailed => ScriptOutcome::LaunchFailed,
        }
    }
}

fn observe(child: &mut ScriptChild, deadline: Instant) -> io::Result<Observation> {
    observe_until(|| child.try_wait(), || pause_until(deadline))
}

fn observe_until(
    mut try_wait: impl FnMut() -> io::Result<Option<ExitStatus>>,
    mut pause: impl FnMut() -> io::Result<()>,
) -> io::Result<Observation> {
    loop {
        if let Some(status) = try_wait()? {
            return Ok(Observation::Exited(status));
        }
        if pause().is_err() {
            // The child may have exited after the last poll. Prefer an
            // observed status before classifying the deadline as a timeout.
            return Ok(try_wait()?.map_or(Observation::Deadline, Observation::Exited));
        }
    }
}

fn reap(child: &mut ScriptChild, deadline: Instant) -> io::Result<()> {
    loop {
        if child.try_wait()?.is_some() {
            return Ok(());
        }
        pause_until(deadline)?;
    }
}

fn numeric_kill(pid: libc::pid_t) -> io::Result<()> {
    // SAFETY: the caller verified ownership and holds the unreaped child.
    kill_result(libc::c_long::from(unsafe {
        libc::kill(pid, libc::SIGKILL)
    }))
}

fn kill_result(result: libc::c_long) -> io::Result<()> {
    if result == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        return Ok(());
    }
    Err(error)
}

fn pause_until(deadline: Instant) -> io::Result<()> {
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        return Err(io::ErrorKind::TimedOut.into());
    }
    thread::sleep(left.min(POLL_INTERVAL));
    Ok(())
}

fn effective_uid() -> u32 {
    // SAFETY: geteuid takes no arguments and has no failure mode.
    unsafe { libc::geteuid() }
}

fn bounded_read(reader: impl Read, limit: u64) -> io::Result<String> {
    let mut text = String::new();
    reader.take(limit + 1).read_to_string(&mut text)?;
    if text.len() as u64 > limit {
        return Err(invalid());
    }
    Ok(text)
}

fn invalid() -> io::Error {
    io::ErrorKind::InvalidData.into()
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::{DirBuilderExt, FileExt};
    use std::os::unix::process::CommandExt;
    use std::process::Command;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};

    use arx_guest::{Request, Response, ScriptStatus, read_frame, write_frame};

    use super::*;

    const CHALLENGE: &str = "0123456789abcdef0123456789abcdef";
    const FIXTURE_BUDGET: Duration = Duration::from_millis(250);

    #[test]
    fn inherited_sigchld_ignore_is_reset_and_child_stays_waitable() {
        const PROBE: &str = "ARX_TEST_INHERITED_SIGCHLD";
        if std::env::var_os(PROBE).is_some() {
            assert_eq!(sigchld_action().unwrap().sa_sigaction, libc::SIG_IGN);
            ignore_sigchld().unwrap();
            assert_ne!(sigchld_action().unwrap().sa_flags & libc::SA_NOCLDWAIT, 0);
            assert!(ReapOwner::verify().is_err());

            let owner = ReapOwner::establish().unwrap();
            let action = sigchld_action().unwrap();
            assert_eq!(action.sa_sigaction, libc::SIG_DFL);
            assert_eq!(action.sa_flags & libc::SA_NOCLDWAIT, 0);
            let deadline = Instant::now() + TEST_HANG_GUARD;
            let mut child = PreparedScript::new("exit 7")
                .unwrap()
                .fork(&owner, ChildSetup::default())
                .unwrap();
            assert!(child.wait_exec(deadline).unwrap());
            let observed = observe_until(
                || child.wait_status(libc::WNOWAIT),
                || pause_until(deadline),
            )
            .unwrap();
            assert_eq!(observed.outcome(), ScriptOutcome::Exited { exit_code: 7 });
            if let Some(pidfd) = &child.pidfd {
                match wait_child(
                    libc::P_PIDFD,
                    pidfd.as_raw_fd() as libc::id_t,
                    libc::WNOWAIT,
                ) {
                    Ok(status) => {
                        assert_eq!(status.and_then(|status| status.code()), Some(7));
                        eprintln!("pidfd wait fixture exercised");
                    }
                    Err(error) => assert_eq!(error.raw_os_error(), Some(libc::EINVAL)),
                }
            }
            // Exercise the numeric fallback against the same waitable child.
            child.pidfd = None;
            assert_eq!(
                child.try_wait().unwrap().and_then(|status| status.code()),
                Some(7)
            );
            assert_eq!(
                child.wait_status(libc::WNOWAIT).unwrap_err().raw_os_error(),
                Some(libc::ECHILD)
            );
            return;
        }

        let mut probe = Command::new(std::env::current_exe().unwrap());
        probe.env(PROBE, "1").args([
            "--exact",
            "platform::linux::tests::inherited_sigchld_ignore_is_reset_and_child_stays_waitable",
            "--nocapture",
        ]);
        // SAFETY: the callback only installs a fixed sigaction with libc.
        unsafe { probe.pre_exec(ignore_sigchld) };
        let deadline = Instant::now() + TEST_HANG_GUARD;
        let mut probe = probe.spawn().unwrap();
        loop {
            if let Some(status) = probe.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            if pause_until(deadline).is_err() {
                let _ = probe.kill();
                probe.wait().unwrap();
                panic!("SIGCHLD probe did not exit");
            }
        }
    }

    fn ignore_sigchld() -> io::Result<()> {
        // SAFETY: glibc sigaction reads the initialized action from this stack.
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = libc::SIG_IGN;
            action.sa_flags = libc::SA_NOCLDWAIT;
            if libc::sigemptyset(&mut action.sa_mask) != 0
                || libc::sigaction(libc::SIGCHLD, &action, std::ptr::null_mut()) != 0
            {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    }

    #[test]
    fn shell_status_and_output_redaction() {
        for (script, expected) in [
            (
                "printf arx-script-sentinel; printf arx-script-sentinel >&2; \
                 test /proc/$$/fd/1 -ef /dev/null && test /proc/$$/fd/2 -ef /dev/null",
                ScriptOutcome::Exited { exit_code: 0 },
            ),
            ("exit 7", ScriptOutcome::Exited { exit_code: 7 }),
            ("kill -TERM $$", ScriptOutcome::Terminated),
        ] {
            let mut child = launched(script, ChildSetup::default());
            let observed = observe(&mut child, Instant::now() + FIXTURE_BUDGET).unwrap();
            if matches!(observed, Observation::Deadline) {
                child.kill().unwrap();
                reap(&mut child, Instant::now() + FIXTURE_BUDGET).unwrap();
            }
            assert_eq!(observed.outcome(), expected);
        }
        let read_only = File::open("/dev/null").unwrap();
        let mut child = PreparedScript::new("exit 0")
            .unwrap()
            .fork(
                &ReapOwner::verify().unwrap(),
                ChildSetup {
                    procs: Some(read_only.as_raw_fd()),
                    ..ChildSetup::default()
                },
            )
            .unwrap();
        assert!(!child.wait_exec(Instant::now() + FIXTURE_BUDGET).unwrap());
        reap(&mut child, Instant::now() + FIXTURE_BUDGET).unwrap();
    }

    #[test]
    fn short_deadline_kills_process_group_and_reaps_owned_child() {
        let (mut pipe, output) = io::pipe().unwrap();
        let mut child = launched(
            "sleep 2 & printf '%s\\n' \"$!\"; wait",
            ChildSetup {
                stdout: Some(output.as_raw_fd()),
                ..ChildSetup::default()
            },
        );
        drop(output);
        let descendant = read_pid(&mut pipe);
        // SAFETY: getpgid reads kernel state and has no pointer parameters.
        assert_eq!(unsafe { libc::getpgid(descendant) }, child.pid);
        assert!(matches!(
            observe(&mut child, Instant::now() + Duration::from_millis(10)).unwrap(),
            Observation::Deadline
        ));
        let pid = child.pid;
        child.kill().unwrap();
        reap(&mut child, Instant::now() + FIXTURE_BUDGET).unwrap();
        assert_eq!(
            child.try_wait().unwrap().unwrap().signal(),
            Some(libc::SIGKILL)
        );
        // SAFETY: a null status pointer discards the result. WNOHANG bounds the call.
        assert_eq!(
            unsafe { libc::waitpid(pid, std::ptr::null_mut(), libc::WNOHANG) },
            -1
        );
        assert_eq!(
            io::Error::last_os_error().raw_os_error(),
            Some(libc::ECHILD)
        );
        ready(&pipe);
        assert_eq!(
            pipe.read(&mut [0]).unwrap(),
            0,
            "descendant still owns output pipe"
        );
    }

    #[test]
    fn unqualified_executor_never_reports_a_timeout() {
        let requests = script_request("while :; do :; done");
        let mut responses = Vec::new();
        assert_eq!(
            serve_connection(&requests[..], &mut responses, None),
            Ok(())
        );
        let mut responses = &responses[..];
        expect_hello(&mut responses, ScriptExecution::Unavailable);
        assert!(matches!(
            read_frame::<Response>(&mut responses).unwrap(),
            Some(Response::ScriptResult {
                id: 1,
                status: ScriptStatus::Unavailable,
                exit_code: None,
            })
        ));
        assert!(responses.is_empty());
    }

    #[test]
    fn containment_failure_suppresses_result_and_redacts_diagnostic() {
        let requests = script_request("printf arx-private-script");
        let root = CgroupRoot {
            path: PathBuf::from("/proc/self/cgroup"),
            owner: effective_uid(),
        };
        let mut responses = Vec::new();
        assert_eq!(
            serve_connection(
                &requests[..],
                &mut responses,
                Some(ScriptRunner::new(root, None, ReapOwner::verify().unwrap()))
            ),
            Err("script containment failed")
        );
        let mut responses = &responses[..];
        expect_hello(&mut responses, ScriptExecution::Available);
        assert!(
            responses.is_empty(),
            "untrustworthy result reached the wire"
        );
    }

    #[test]
    fn stalled_pre_exec_child_releases_port_and_is_reaped() {
        let fixture = Arc::new(FakeCgroups::new());
        // The child stalls before joining. The command cgroup stays empty.
        fs::write(fixture.0.join("cgroup.events"), b"populated 0\n").unwrap();
        let (mut peer, port) = io::pipe().unwrap();
        let (started, ready) = io::pipe().unwrap();
        let (gate, _held_gate) = io::pipe().unwrap();
        let stall = StallWatchdog {
            child: StallBeforeExec {
                ready: ready.as_raw_fd(),
                gate: gate.as_raw_fd(),
            },
            ready: started,
            receipt: Cell::new(None),
        };
        let mut runner = fixture.runner();
        runner.cleanup_budget = TEST_HANG_GUARD;
        runner.port = Some(port.as_fd());
        runner.stall = Some(&stall);
        let result = serve_connection(script_request("exit 7").as_slice(), &port, Some(runner));
        drop(port);
        drop(ready);

        let (pid, inherited) = stall
            .receipt
            .get()
            .expect("child did not acknowledge the stall");
        assert!(pid > 0);
        wait_readable(peer.as_raw_fd(), Instant::now() + TEST_HANG_GUARD).unwrap();
        expect_hello(&mut peer, ScriptExecution::Available);
        let release = wait_readable(peer.as_raw_fd(), Instant::now() + TEST_HANG_GUARD)
            .and_then(|()| peer.read(&mut [0]));

        // SAFETY: pid came directly from the forked fixture child. WNOHANG
        // checks whether the runner already reaped it without blocking.
        let waited = unsafe { libc::waitpid(pid, std::ptr::null_mut(), libc::WNOHANG) };
        let reaped =
            waited == -1 && io::Error::last_os_error().raw_os_error() == Some(libc::ECHILD);
        if waited == 0 {
            // Keep the mutation witness from leaving its blocked child behind.
            let mut child = ScriptChild::new(pid, stall.ready);
            child.kill().unwrap();
            reap(&mut child, Instant::now() + FIXTURE_BUDGET).unwrap();
        }
        assert_eq!(release.expect("stalled child kept the port open"), 0);
        assert_eq!(inherited, 0, "the first child step did not close the port");
        assert!(reaped, "the watchdog did not reap the stalled child");
        assert_eq!(result, Err("script containment failed"));
    }

    #[test]
    fn production_cleanup_writes_kill_and_waits_for_empty_before_result() {
        let fixture = Arc::new(FakeCgroups::new());
        let release = Arc::clone(&fixture);
        let peer = thread::spawn(move || -> io::Result<()> {
            let deadline = Instant::now() + FIXTURE_BUDGET;
            while fs::read(release.0.join("cgroup.kill"))? != b"1" {
                pause_until(deadline)?;
            }
            // Keep the real events descriptor valid while changing its value.
            control(&release.0, "cgroup.events", effective_uid())?
                .write_all_at(b"0", b"populated ".len() as u64)
        });
        let (result, bytes) = run_exit_seven(fixture.runner());
        peer.join()
            .unwrap()
            .expect("production did not write cgroup.kill");
        assert_eq!(result, Ok(()));
        let mut bytes = bytes.as_slice();
        expect_hello(&mut bytes, ScriptExecution::Available);
        expect_exit_seven(&mut bytes);
    }

    #[test]
    fn production_cleanup_retires_without_result_while_group_stays_populated() {
        let fixture = Arc::new(FakeCgroups::new());
        let mut runner = fixture.runner();
        runner.cleanup_budget = Duration::from_millis(20);
        let (result, bytes) = run_exit_seven(runner);
        assert_eq!(fs::read(fixture.0.join("cgroup.procs")).unwrap(), b"0");
        assert_eq!(fs::read(fixture.0.join("cgroup.kill")).unwrap(), b"1");
        assert_retired(result, &bytes);
    }

    #[test]
    fn exit_observed_at_deadline_wins_over_timeout() {
        let clock = Cell::new(Duration::ZERO);
        let exit_time = Duration::from_millis(9);
        let deadline = Duration::from_millis(10);
        let result = observe_until(
            || Ok((clock.get() >= exit_time).then(|| ExitStatus::from_raw(7 << 8))),
            || {
                clock.set(deadline);
                Err(io::ErrorKind::TimedOut.into())
            },
        );
        assert_eq!(
            result.unwrap().outcome(),
            ScriptOutcome::Exited { exit_code: 7 }
        );
    }

    #[test]
    fn delegated_cgroup_kills_detached_descendant_and_confirms_empty() {
        // The local emulator can provision this disposable unit-shaped tree
        // and run this test on it as an ordinary user. Hosted CI may lack it.
        let owner = effective_uid();
        let service = if owner == 0 {
            SERVICE
        } else {
            "arx-guest-test.service"
        };
        let discover = || {
            if owner == 0 {
                CgroupRoot::qualify()
            } else {
                CgroupRoot::discover(service, owner)
            }
        };
        let root = match discover() {
            Ok(root) => root,
            Err(reason) => {
                eprintln!("cgroup fixture skipped: {reason:?}");
                return;
            }
        };
        let mut group = root.new_command().unwrap();
        let (mut pipe, output) = io::pipe().unwrap();
        let mut child = launched(
            "setsid /bin/sh -c 'printf \"%s\\n\" \"$$\"; exec sleep 2' & exit 0",
            ChildSetup {
                procs: Some(group.procs.as_raw_fd()),
                drop_privileges: owner == 0,
                stdout: Some(output.as_raw_fd()),
                ..ChildSetup::default()
            },
        );
        drop(output);
        let descendant = read_pid(&mut pipe);
        // SAFETY: getsid reads kernel state and has no pointer parameters.
        assert_eq!(unsafe { libc::getsid(descendant) }, descendant);
        assert_eq!(
            observe(&mut child, Instant::now() + FIXTURE_BUDGET)
                .unwrap()
                .outcome(),
            ScriptOutcome::Exited { exit_code: 0 }
        );
        assert!(group.populated().unwrap());
        assert!(matches!(discover(), Err(Qualification::CommandSlot)));
        group.kill().unwrap();
        group.wait_empty(Instant::now() + FIXTURE_BUDGET).unwrap();
        root.remove_command(&group).unwrap();
        ready(&pipe);
        assert_eq!(pipe.read(&mut [0]).unwrap(), 0);
        assert!(!group.path.exists());
        let runner = ScriptRunner::new(root, None, ReapOwner::verify().unwrap());
        if owner == 0 {
            assert_eq!(
                runner
                    .run(
                        "test \"$(id -u):$(id -g):$(id -G)\" = 65534:65534:65534",
                        FIXTURE_BUDGET,
                    )
                    .unwrap(),
                ScriptOutcome::Exited { exit_code: 0 }
            );
        }
        assert_eq!(
            runner
                .run("while :; do :; done", Duration::from_millis(50))
                .unwrap(),
            ScriptOutcome::TimedOut
        );
        assert!(!group.path.exists());

        let (result, responses) = run_exit_seven(runner);
        assert_eq!(result, Ok(()));
        let mut responses = &responses[..];
        expect_hello(&mut responses, ScriptExecution::Available);
        expect_exit_seven(&mut responses);
        eprintln!("cgroup fixture exercised");
    }

    struct FakeCgroups(PathBuf);

    impl FakeCgroups {
        fn new() -> Self {
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let name = format!(
                "arx-guest-cgroup-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            );
            let path = std::env::temp_dir().join(name);
            fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
            let fixture = Self(path);
            for (file, contents) in [
                ("cgroup.procs", &b""[..]),
                ("cgroup.kill", &b""[..]),
                ("cgroup.events", &b"populated 1\n"[..]),
            ] {
                OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(fixture.0.join(file))
                    .unwrap()
                    .write_all(contents)
                    .unwrap();
            }
            fixture
        }

        fn runner<'port>(self: &Arc<Self>) -> ScriptRunner<'port> {
            ScriptRunner {
                cgroups: Box::new(Arc::clone(self)),
                port: None,
                reap_owner: ReapOwner::verify().unwrap(),
                max_budget: FIXTURE_BUDGET,
                cleanup_budget: FIXTURE_BUDGET,
                drop_privileges: false,
                stall: None,
            }
        }
    }

    impl CommandCgroups for Arc<FakeCgroups> {
        fn new_command(&self) -> io::Result<CommandGroup> {
            CommandGroup::open_controls(&self.0, effective_uid())
        }

        fn remove_command(&self, _: &CommandGroup) -> io::Result<()> {
            // Regular fixture files are removed by Drop. The production
            // read and write operations still use the same open controls.
            Ok(())
        }
    }

    impl Drop for FakeCgroups {
        fn drop(&mut self) {
            for file in ["cgroup.procs", "cgroup.kill", "cgroup.events"] {
                let _ = fs::remove_file(self.0.join(file));
            }
            let _ = fs::remove_dir(&self.0);
        }
    }

    fn run_exit_seven(runner: ScriptRunner<'_>) -> (Result<(), &'static str>, Vec<u8>) {
        let mut bytes = Vec::new();
        let result = serve_connection(
            script_request("exit 7").as_slice(),
            &mut bytes,
            Some(runner),
        );
        (result, bytes)
    }

    fn assert_retired(result: Result<(), &'static str>, mut bytes: &[u8]) {
        expect_hello(&mut bytes, ScriptExecution::Available);
        assert!(bytes.is_empty(), "untrustworthy result reached the wire");
        assert_eq!(result, Err("script containment failed"));
    }

    fn expect_exit_seven(responses: &mut &[u8]) {
        assert!(matches!(
            read_frame::<Response>(responses).unwrap(),
            Some(Response::ScriptResult {
                id: 1,
                status: ScriptStatus::Exited,
                exit_code: Some(7),
            })
        ));
        assert!(responses.is_empty());
    }

    fn script_request(script: &str) -> Vec<u8> {
        let mut requests = Vec::new();
        write_frame(
            &mut requests,
            &Request::Hello {
                version: 1,
                challenge: CHALLENGE.into(),
            },
        )
        .unwrap();
        write_frame(
            &mut requests,
            &Request::RunScript {
                id: 1,
                script: script.into(),
                timeout_s: 1,
            },
        )
        .unwrap();
        requests
    }

    fn expect_hello(responses: &mut impl Read, script_execution: ScriptExecution) {
        assert!(matches!(
            read_frame::<Response>(responses).unwrap(),
            Some(Response::HelloAck {
                version: 1,
                challenge,
                script_execution: advertised,
            }) if challenge == CHALLENGE && advertised == script_execution
        ));
    }

    fn launched(script: &str, setup: ChildSetup) -> ScriptChild {
        let mut child = PreparedScript::new(script)
            .unwrap()
            .fork(&ReapOwner::verify().unwrap(), setup)
            .unwrap();
        assert!(child.wait_exec(Instant::now() + FIXTURE_BUDGET).unwrap());
        child
    }

    fn read_pid(pipe: &mut PipeReader) -> libc::pid_t {
        ready(pipe);
        let mut line = [0; 32];
        let len = pipe.read(&mut line).unwrap();
        std::str::from_utf8(&line[..len])
            .unwrap()
            .trim_end()
            .parse()
            .unwrap()
    }

    fn ready(pipe: &impl AsRawFd) {
        let mut fd = libc::pollfd {
            fd: pipe.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: fd refers to a single initialized pollfd and poll is bounded.
        assert_eq!(
            unsafe { libc::poll(&mut fd, 1, FIXTURE_BUDGET.as_millis() as i32) },
            1
        );
        assert_ne!(fd.revents & (libc::POLLIN | libc::POLLHUP), 0);
    }
}
