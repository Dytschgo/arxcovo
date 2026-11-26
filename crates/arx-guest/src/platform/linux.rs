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

use arx_guest::compose::{
    self, Clock, CommandRunner, ComposeService, ComposeStore, Invocation, MAX_SECRET, RunResult,
    SecretFileWitness, SecretMatch, StoreError,
};
use arx_guest::compose_backup::{
    self, BackupHelperError, LINUX_HELPER_CONFIG_FD, LINUX_HELPER_DST_STAT_FD,
    LINUX_HELPER_RESULT_FD, LINUX_HELPER_VOL_FD, LinuxBackupHelperConfig, LinuxBackupHelperReply,
    MAX_BACKUP_BYTES, StagedBackupMeta, is_db_basename, verify_dir_file_no_follow_fstatat,
    verify_pinned_sqlite, verify_portable_sqlite_file,
};
use arx_guest::{MAX_SCRIPT, ScriptExecution, ScriptExecutor, ScriptOutcome, serve_with_compose};

mod docker_trial;

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
    if let Some(mode) = docker_trial::mode().map_err(|_| "Compose trial mode refused")? {
        if !compose_finalized().map_err(|_| "Compose guest marker refused")? {
            return Err("Compose trial marker required");
        }
        return arx_guest::docker_trial::serve(
            mode,
            &port,
            &port,
            &mut docker_trial::LinuxBootstrap::default(),
        )
        .map_err(|_| "Compose trial bootstrap failed");
    }
    let compose = match compose_finalized() {
        Ok(false) => None,
        Ok(true) => Some(qualify_compose().ok_or("Compose guest qualification failed")?),
        Err(_) => return Err("Compose guest marker refused"),
    };
    // Design §3: the script runner shares the guest network namespace and has
    // no ingress-isolation witness for UID/GID 65534, so legacy scripts stay
    // disabled on a Compose-qualified guest.
    let runner = if compose.is_some() {
        None
    } else {
        ReapOwner::establish().ok().and_then(|owner| {
            CgroupRoot::qualify()
                .ok()
                .map(|root| ScriptRunner::new(root, Some(port.as_fd()), owner))
        })
    };
    let result = serve_connection_with(&port, &port, runner, compose.as_ref());
    // Intent survives a disconnected host: let a running operation finish
    // before the service exits. A later session queries it by operation ID.
    if let Some(compose) = &compose {
        compose.wait_idle(OPERATION_DRAIN);
    }
    result
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

#[cfg(test)]
fn serve_connection(
    reader: impl Read,
    writer: impl Write,
    runner: Option<ScriptRunner<'_>>,
) -> Result<(), &'static str> {
    serve_connection_with(reader, writer, runner, None)
}

fn serve_connection_with(
    reader: impl Read,
    writer: impl Write,
    runner: Option<ScriptRunner<'_>>,
    compose: Option<&ComposeService>,
) -> Result<(), &'static str> {
    let retired = Cell::new(false);
    let mut executor = LinuxExecutor {
        // Defense in depth: never retain a script runner alongside Compose.
        runner: if compose.is_some() { None } else { runner },
        retired: &retired,
    };
    let result = serve_with_compose(
        reader,
        SessionWriter {
            writer,
            retired: &retired,
        },
        &mut executor,
        compose,
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
                            #[cfg(test)]
                            gate: None,
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
    /// Inherited read end. Tests allocate it at fd 10 or above; the child
    /// maps it to fd 3 before exec because dash rejects higher redirections.
    #[cfg(test)]
    gate: Option<RawFd>,
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
const HELD_GATE_FD: RawFd = 3;
#[cfg(test)]
const HELD_GATE_FLOOR: RawFd = 10;

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
        let (setup, input, null, report) = pin_held_gate(setup, input, null, report);
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

/// Put the test gate on fd 3 without dropping a descriptor the child still
/// needs. Dash accepts redirections only for fds 0-9, so a fixture that
/// inherits fd 10 or above would lose the gate at the shell.
#[cfg(test)]
unsafe fn pin_held_gate(
    mut setup: ChildSetup,
    mut input: RawFd,
    mut null: RawFd,
    mut report: RawFd,
) -> (ChildSetup, RawFd, RawFd, RawFd) {
    unsafe {
        let Some(gate) = setup.gate else {
            return (setup, input, null, report);
        };
        if gate != HELD_GATE_FD {
            let flags = libc::fcntl(HELD_GATE_FD, libc::F_GETFD);
            if flags != -1 {
                let moved = libc::fcntl(HELD_GATE_FD, libc::F_DUPFD, HELD_GATE_FLOOR);
                if moved < HELD_GATE_FLOOR {
                    launch_failed(report);
                }
                if (flags & libc::FD_CLOEXEC) != 0
                    && libc::fcntl(moved, libc::F_SETFD, libc::FD_CLOEXEC) != 0
                {
                    launch_failed(report);
                }
                if input == HELD_GATE_FD {
                    input = moved;
                }
                if null == HELD_GATE_FD {
                    null = moved;
                }
                if report == HELD_GATE_FD {
                    report = moved;
                }
                if let Some(fd) = setup.stdout.as_mut()
                    && *fd == HELD_GATE_FD
                {
                    *fd = moved;
                }
                if let Some(fd) = setup.procs.as_mut()
                    && *fd == HELD_GATE_FD
                {
                    *fd = moved;
                }
                if let Some(stall) = setup.stall.as_mut() {
                    if stall.ready == HELD_GATE_FD {
                        stall.ready = moved;
                    }
                    if stall.gate == HELD_GATE_FD {
                        stall.gate = moved;
                    }
                }
                if libc::close(HELD_GATE_FD) != 0 {
                    launch_failed(report);
                }
            }
            if libc::dup2(gate, HELD_GATE_FD) < 0 || libc::close(gate) != 0 {
                launch_failed(report);
            }
        }
        (setup, input, null, report)
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
    // Tests sample this instant after the exec pipe is readable. Production
    // always uses the clock. None keeps that path.
    #[cfg(test)]
    exec_now: Option<Instant>,
    // Set only by the comparison below, after wait_readable has returned.
    #[cfg(test)]
    deadline_boundary_reached: bool,
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
            #[cfg(test)]
            exec_now: None,
            #[cfg(test)]
            deadline_boundary_reached: false,
        }
    }

    fn wait_exec(&mut self, deadline: Instant) -> io::Result<bool> {
        if let Some(error) = self.pidfd_error.take() {
            return Err(error);
        }
        wait_readable(self.exec_reader.as_raw_fd(), deadline)?;
        // A pipe that becomes readable as the deadline expires is still a
        // timeout. Exec closes the writer; that close is not a successful wait
        // once the budget is spent.
        #[cfg(test)]
        let now = self.exec_now.unwrap_or_else(Instant::now);
        #[cfg(not(test))]
        let now = Instant::now();
        if now >= deadline {
            #[cfg(test)]
            {
                self.deadline_boundary_reached = true;
            }
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

// ---------------------------------------------------------------------------
// Compose engine and storage. Everything below is **unverified on real
// hardware**: hosted CI exercises the runner's environment clearing, deadline
// kill and capture bound, and the store's fd-relative no-follow behavior in a
// temporary directory, but never Docker, the recipe's roots or a guest tmpfs.

/// Upper bound on finishing a running operation after the host disconnects.
const OPERATION_DRAIN: Duration = Duration::from_secs(30 * 60);
use std::os::unix::fs::PermissionsExt;
const ROOT_UID: u32 = 0;

fn compose_finalized() -> io::Result<bool> {
    match walk_owned_dirs("/etc/arxcovo", ROOT_UID) {
        Ok(dir) => finalized_marker(dir.as_fd(), ROOT_UID),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn finalized_marker(dir: BorrowedFd<'_>, owner: u32) -> io::Result<bool> {
    const VALUE: &str = "ubuntu-noble-amd64-compose-v1\n";
    let file = match open_at(
        dir,
        "compose-finalized",
        libc::O_RDONLY | libc::O_NONBLOCK,
        0,
    ) {
        Ok(file) => File::from(file),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    let meta = file.metadata()?;
    if !meta.is_file()
        || meta.uid() != owner
        || meta.gid() != 0 && owner == ROOT_UID
        || meta.mode() & 0o022 != 0
        || bounded_read(file, VALUE.len() as u64)? != VALUE
    {
        return Err(invalid());
    }
    Ok(true)
}

fn qualify_compose() -> Option<ComposeService> {
    if effective_uid() != ROOT_UID || verify_engine(compose::DOCKER).is_err() {
        return None;
    }
    walk_owned_dirs(compose::ENGINE_CWD, ROOT_UID).ok()?;
    let store = LinuxComposeStore::open(
        compose::STATE_ROOT,
        compose::SECRET_ROOT,
        compose::DOCKER_VOLUME_ROOT,
        ROOT_UID,
        true,
    )
    .ok()?;
    ComposeService::new(
        Box::new(DockerRunner::new()),
        Box::new(store),
        Box::new(SystemClock),
    )
    .ok()
}

fn verify_engine(path: &str) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.uid() != ROOT_UID || metadata.mode() & 0o022 != 0 {
        return Err(invalid());
    }
    Ok(())
}

struct SystemClock;

impl Clock for SystemClock {
    fn now_unix(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs())
    }
}

/// Starts the fixed engine with an empty ambient environment plus exactly
/// `ENGINE_ENV`, in a fixed empty directory, stdin and stderr on the null
/// device and its own process group, so a deadline kill reaches the Compose
/// plugin as well.
struct DockerRunner {
    program: &'static str,
    cwd: &'static str,
}

impl DockerRunner {
    fn new() -> Self {
        Self {
            program: compose::DOCKER,
            cwd: compose::ENGINE_CWD,
        }
    }
}

impl CommandRunner for DockerRunner {
    fn run(&mut self, invocation: &Invocation) -> RunResult {
        use std::os::unix::process::CommandExt;
        use std::process::{Command, Stdio};

        let mut command = Command::new(self.program);
        command
            .args(&invocation.argv)
            .env_clear()
            .envs(compose::ENGINE_ENV)
            .current_dir(self.cwd)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .stdout(if invocation.capture {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .process_group(0);
        let Ok(mut child) = command.spawn() else {
            return RunResult::LaunchFailed;
        };
        let reader = child.stdout.take().map(|mut stdout| {
            thread::spawn(move || -> Result<(Vec<u8>, bool), ()> {
                let mut bytes = Vec::new();
                // Read up to MAX_CAPTURE + 1 bytes so exact-bound output is
                // distinguished from truncated output.
                (&mut stdout)
                    .take(compose::MAX_CAPTURE as u64 + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|_| ())?;
                let truncated = bytes.len() > compose::MAX_CAPTURE;
                if truncated {
                    bytes.clear();
                    // Drain the rest so the child never blocks on a full pipe.
                    let _ = io::copy(&mut stdout, &mut io::sink());
                }
                Ok((bytes, truncated))
            })
        });
        let deadline = Instant::now() + invocation.timeout;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
                _ => break None,
            }
        };
        let Some(status) = status else {
            if let Ok(group) = libc::pid_t::try_from(child.id()) {
                // SAFETY: the unreaped child leads this process group.
                unsafe { libc::kill(-group, libc::SIGKILL) };
            }
            let _ = child.kill();
            let _ = child.wait();
            // The reader is left to finish on its own; a descendant could
            // still hold the pipe.
            return RunResult::TimedOut;
        };
        let capture_result = match reader {
            Some(reader) => reader.join().ok().and_then(Result::ok),
            None => Some((Vec::new(), false)),
        };
        match (status.code(), capture_result) {
            (Some(code), Some((stdout, false))) => RunResult::Exited { code, stdout },
            (Some(code), _) => RunResult::OutputIncomplete { code },
            (None, _) => RunResult::Signaled,
        }
    }

    fn probe_http(&mut self, port: u16, path: &str) -> bool {
        use std::net::{SocketAddr, TcpStream};

        let address = SocketAddr::from(([127, 0, 0, 1], port));
        let Ok(mut stream) = TcpStream::connect_timeout(&address, Duration::from_secs(2)) else {
            return false;
        };
        if stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .and_then(|()| stream.set_write_timeout(Some(Duration::from_secs(5))))
            .is_err()
        {
            return false;
        }
        let request =
            format!("GET {path} HTTP/1.0\r\nHost: arx-health\r\nConnection: close\r\n\r\n");
        let mut head = [0u8; 12];
        stream.write_all(request.as_bytes()).is_ok()
            && stream.read_exact(&mut head).is_ok()
            && head.starts_with(b"HTTP/1.")
            && head[8] == b' '
            && head[9] == b'2'
    }

    fn pause(&mut self, duration: Duration) {
        thread::sleep(duration);
    }
}

/// Root-owned storage reached only through directory descriptors. Every
/// component is opened with O_NOFOLLOW and checked for its owner and mode,
/// files are replaced through an O_EXCL temporary and renameat, and secret
/// files are chowned and chmodded on the descriptor before any byte lands.
struct LinuxComposeStore {
    state: OwnedFd,
    state_path: PathBuf,
    secrets: OwnedFd,
    volume_root: String,
    #[cfg(test)]
    volume_root_fd: Option<OwnedFd>,
    owner: u32,
}

impl LinuxComposeStore {
    fn open(
        state: &str,
        secrets: &str,
        volume_root: &str,
        owner: u32,
        require_tmpfs: bool,
    ) -> io::Result<Self> {
        let state_fd = walk_owned_dirs(state, owner)?;
        verify_private_dir(state_fd.as_fd(), owner)?;
        let secrets = walk_owned_dirs(secrets, owner)?;
        verify_private_dir(secrets.as_fd(), owner)?;
        if require_tmpfs && !is_tmpfs(secrets.as_fd())? {
            return Err(invalid());
        }
        Ok(Self {
            state: state_fd,
            state_path: PathBuf::from(state),
            secrets,
            volume_root: volume_root.to_owned(),
            #[cfg(test)]
            volume_root_fd: None,
            owner,
        })
    }

    fn open_volume_parent(&self, volume_name: &str) -> io::Result<OwnedFd> {
        #[cfg(test)]
        if let Some(root_fd) = &self.volume_root_fd {
            let dir = open_at(
                root_fd.as_fd(),
                volume_name,
                libc::O_RDONLY | libc::O_DIRECTORY,
                0,
            )?;
            let meta = File::from(dir.try_clone()?).metadata()?;
            if !meta.is_dir()
                || (meta.uid() != self.owner && meta.uid() != ROOT_UID)
                || meta.mode() & 0o022 != 0
            {
                return Err(invalid());
            }
            return Ok(dir);
        }
        walk_owned_dirs(&format!("{}/{volume_name}", self.volume_root), self.owner)
    }

    fn version_dir(&self) -> io::Result<OwnedFd> {
        private_subdir(
            self.state.as_fd(),
            compose::LAYOUT_VERSION,
            self.owner,
            true,
        )
    }

    fn backup_staging_dir(&self) -> io::Result<OwnedFd> {
        private_subdir(self.state.as_fd(), "backup-staging", self.owner, true)
    }
}

/// The fixed nginx paths reside in a root-owned directory. Keep its descriptor
/// across staging, validation, rename and rollback.
struct LinuxNginx(OwnedFd);

impl compose::routing::NginxController for LinuxNginx {
    fn read_active(&self) -> Result<Option<String>, compose::routing::NginxError> {
        use compose::routing::NginxError::ReadActiveFailed;
        const MAX_CONFIG: u64 = 64 * 1024;
        let fd = match open_at(
            self.0.as_fd(),
            "nginx.conf",
            libc::O_RDONLY | libc::O_NONBLOCK,
            0,
        ) {
            Ok(fd) => fd,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(ReadActiveFailed),
        };
        let file = File::from(fd);
        let meta = file.metadata().map_err(|_| ReadActiveFailed)?;
        if !meta.is_file() || meta.uid() != 0 || meta.mode() & 0o022 != 0 || meta.len() > MAX_CONFIG
        {
            return Err(ReadActiveFailed);
        }
        let mut prior = String::new();
        file.take(MAX_CONFIG + 1)
            .read_to_string(&mut prior)
            .map_err(|_| ReadActiveFailed)?;
        if prior.len() as u64 > MAX_CONFIG {
            return Err(ReadActiveFailed);
        }
        Ok(Some(prior))
    }

    fn write_staging(&mut self, config: &str) -> Result<(), compose::routing::NginxError> {
        replace_at(
            self.0.as_fd(),
            "nginx.conf.next",
            config.as_bytes(),
            0o600,
            None,
        )
        .map(|_| ())
        .map_err(|_| compose::routing::NginxError::StageWriteFailed)
    }

    fn remove_staging(&mut self) {
        // SAFETY: held directory and fixed NUL-terminated basename.
        unsafe {
            libc::unlinkat(self.0.as_raw_fd(), c"nginx.conf.next".as_ptr(), 0);
        }
    }

    fn run_fixed(&mut self, argv: &[&str]) -> bool {
        use compose::routing::{NGINX_BIN, nginx_reload_argv, nginx_validate_argv};
        if argv != nginx_validate_argv() && argv != nginx_reload_argv() {
            return false;
        }
        let mut runner = DockerRunner {
            program: NGINX_BIN,
            cwd: "/",
        };
        matches!(
            runner.run(&Invocation {
                argv: argv[1..].iter().map(|s| (*s).to_owned()).collect(),
                timeout: Duration::from_secs(3),
                capture: false,
            }),
            RunResult::Exited { code: 0, .. }
        )
    }

    fn activate_staging(&mut self) -> Result<(), compose::routing::NginxError> {
        let dir = self.0.as_raw_fd();
        // SAFETY: both fixed basenames refer to entries of the held directory.
        if unsafe {
            libc::renameat(
                dir,
                c"nginx.conf.next".as_ptr(),
                dir,
                c"nginx.conf".as_ptr(),
            )
        } != 0
        {
            return Err(compose::routing::NginxError::ActivateFailed);
        }
        Ok(())
    }

    fn restore_active(
        &mut self,
        previous: Option<&str>,
    ) -> Result<(), compose::routing::NginxError> {
        let failed = compose::routing::NginxError::RollbackFailed;
        if let Some(config) = previous {
            replace_at(self.0.as_fd(), "nginx.conf", config.as_bytes(), 0o600, None)
                .map(|_| ())
                .map_err(|_| failed)
        } else {
            // SAFETY: fixed basename in the held directory.
            if unsafe { libc::unlinkat(self.0.as_raw_fd(), c"nginx.conf".as_ptr(), 0) } == 0 {
                Ok(())
            } else {
                Err(failed)
            }
        }
    }
}

impl ComposeStore for LinuxComposeStore {
    fn install_routes(&mut self, table: &compose::routing::RoutingTable) -> Result<(), StoreError> {
        let dir = walk_owned_dirs("/etc/arxcovo", 0).map_err(|_| StoreError)?;
        compose::routing::apply_nginx_config(table, &mut LinuxNginx(dir))
            .map(|_| ())
            .map_err(|_| StoreError)
    }

    fn load_state(&mut self) -> Result<Option<Vec<u8>>, StoreError> {
        let dir = self.version_dir().map_err(|_| StoreError)?;
        let file = match open_at(dir.as_fd(), "state.json", libc::O_RDONLY, 0) {
            Ok(fd) => File::from(fd),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(StoreError),
        };
        let metadata = file.metadata().map_err(|_| StoreError)?;
        if !metadata.is_file() || metadata.uid() != self.owner || metadata.mode() & 0o077 != 0 {
            return Err(StoreError);
        }
        let mut bytes = Vec::new();
        file.take(compose::MAX_STATE_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| StoreError)?;
        if bytes.len() > compose::MAX_STATE_BYTES {
            return Err(StoreError);
        }
        Ok(Some(bytes))
    }

    fn save_state(&mut self, bytes: &[u8]) -> Result<(), StoreError> {
        let dir = self.version_dir().map_err(|_| StoreError)?;
        replace_at(dir.as_fd(), "state.json", bytes, 0o600, None)
            .map(|_| ())
            .map_err(|_| StoreError)
    }

    fn write_revision(
        &mut self,
        project: &str,
        revision: u64,
        yaml: &str,
    ) -> Result<(), StoreError> {
        let mut dir = self.version_dir().map_err(|_| StoreError)?;
        for name in ["projects", project, &format!("r{revision}")] {
            dir = private_subdir(dir.as_fd(), name, self.owner, true).map_err(|_| StoreError)?;
        }
        replace_at(dir.as_fd(), "compose.yaml", yaml.as_bytes(), 0o600, None)
            .map(|_| ())
            .map_err(|_| StoreError)
    }

    fn write_rotation(
        &mut self,
        project: &str,
        revision: u64,
        operation_id: &str,
        yaml: &str,
    ) -> Result<(), StoreError> {
        let mut dir = self.version_dir().map_err(|_| StoreError)?;
        for name in ["projects", project, &format!("r{revision}")] {
            dir = private_subdir(dir.as_fd(), name, self.owner, true).map_err(|_| StoreError)?;
        }
        replace_at(
            dir.as_fd(),
            &format!("rot-{operation_id}.yaml"),
            yaml.as_bytes(),
            0o600,
            None,
        )
        .map(|_| ())
        .map_err(|_| StoreError)
    }

    fn write_secret(
        &mut self,
        project: &str,
        instance: &str,
        owner: (u32, u32),
        bytes: &[u8],
    ) -> Result<SecretFileWitness, StoreError> {
        let dir = private_subdir(self.secrets.as_fd(), project, self.owner, true)
            .map_err(|_| StoreError)?;
        replace_at(dir.as_fd(), instance, bytes, 0o400, Some(owner)).map_err(|_| StoreError)
    }

    fn stat_secret(
        &mut self,
        project: &str,
        instance: &str,
        owner: (u32, u32),
    ) -> Option<SecretFileWitness> {
        let dir = private_subdir(self.secrets.as_fd(), project, self.owner, false).ok()?;
        let metadata = open_at(dir.as_fd(), instance, libc::O_RDONLY, 0)
            .map(File::from)
            .and_then(|file| file.metadata())
            .ok()?;
        if metadata.is_file()
            && (metadata.uid(), metadata.gid()) == owner
            && metadata.mode() & 0o7777 == 0o400
            && metadata.nlink() == 1
        {
            Some(SecretFileWitness {
                dev: metadata.dev(),
                ino: metadata.ino(),
                uid: metadata.uid(),
                gid: metadata.gid(),
            })
        } else {
            None
        }
    }

    fn compare_secret(
        &mut self,
        project: &str,
        instance: &str,
        owner: (u32, u32),
        bytes: &[u8],
    ) -> SecretMatch {
        struct Wiped(Vec<u8>);
        impl Drop for Wiped {
            fn drop(&mut self) {
                compose::wipe_bytes(&mut self.0);
            }
        }

        let dir = match private_subdir(self.secrets.as_fd(), project, self.owner, false) {
            Ok(dir) => dir,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return SecretMatch::Missing,
            Err(_) => return SecretMatch::Error,
        };
        let file = match open_at(dir.as_fd(), instance, libc::O_RDONLY, 0) {
            Ok(fd) => File::from(fd),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return SecretMatch::Missing,
            Err(_) => return SecretMatch::Error,
        };
        let Ok(metadata) = file.metadata() else {
            return SecretMatch::Error;
        };
        if !metadata.is_file()
            || (metadata.uid(), metadata.gid()) != owner
            || metadata.mode() & 0o7777 != 0o400
            || metadata.nlink() != 1
        {
            return SecretMatch::Mismatch;
        }
        let mut stored = Wiped(Vec::with_capacity(bytes.len().min(MAX_SECRET) + 1));
        if file
            .take(MAX_SECRET as u64 + 1)
            .read_to_end(&mut stored.0)
            .is_err()
        {
            return SecretMatch::Error;
        }
        if stored.0.as_slice() == bytes {
            SecretMatch::Match(SecretFileWitness {
                dev: metadata.dev(),
                ino: metadata.ino(),
                uid: metadata.uid(),
                gid: metadata.gid(),
            })
        } else {
            SecretMatch::Mismatch
        }
    }

    fn prepare_volume(&mut self, name: &str, owner: (u32, u32)) -> Result<(), StoreError> {
        let parent = walk_owned_dirs(&format!("{}/{name}", self.volume_root), self.owner)
            .map_err(|_| StoreError)?;
        let data = File::from(
            open_at(
                parent.as_fd(),
                "_data",
                libc::O_RDONLY | libc::O_DIRECTORY,
                0,
            )
            .map_err(|_| StoreError)?,
        );
        let metadata = data.metadata().map_err(|_| StoreError)?;
        if !metadata.is_dir() {
            return Err(StoreError);
        }
        if (metadata.uid(), metadata.gid()) == owner {
            return Ok(());
        }
        // Only a fresh, empty, broker-owned data root is handed over.
        let empty = fs::read_dir(format!("/proc/self/fd/{}", data.as_raw_fd()))
            .map_err(|_| StoreError)?
            .next()
            .is_none();
        if metadata.uid() != self.owner || !empty {
            return Err(StoreError);
        }
        std::os::unix::fs::fchown(&data, Some(owner.0), Some(owner.1)).map_err(|_| StoreError)?;
        data.set_permissions(fs::Permissions::from_mode(0o700))
            .map_err(|_| StoreError)
    }

    fn run_sqlite_backup(
        &mut self,
        operation_id: &str,
        volume_name: &str,
        owner: (u32, u32),
        database: &str,
        max_bytes: u64,
        deadline: Duration,
    ) -> Result<StagedBackupMeta, BackupHelperError> {
        use std::os::unix::process::CommandExt;
        use std::process::{Command, Stdio};

        verify_pinned_sqlite()?;
        if operation_id.len() != 32
            || !operation_id
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
            || !is_db_basename(database)
            || max_bytes == 0
            || max_bytes > MAX_BACKUP_BYTES
            || component(volume_name).is_err()
        {
            return Err(BackupHelperError::InvalidInput);
        }
        let deadline_at = Instant::now() + deadline;

        // 1. Walk volume root fd-relative with O_NOFOLLOW and verify ownership.
        let vol_parent = self
            .open_volume_parent(volume_name)
            .map_err(|_| BackupHelperError::PathViolation)?;
        let vol_data_fd = open_at(
            vol_parent.as_fd(),
            "_data",
            libc::O_RDONLY | libc::O_DIRECTORY,
            0,
        )
        .map_err(|e| {
            if e.kind() == io::ErrorKind::NotFound {
                BackupHelperError::NotFound
            } else {
                BackupHelperError::PathViolation
            }
        })?;
        let vol_meta = File::from(
            vol_data_fd
                .try_clone()
                .map_err(|_| BackupHelperError::StoreFailed)?,
        )
        .metadata()
        .map_err(|_| BackupHelperError::PathViolation)?;
        if !vol_meta.is_dir() || (vol_meta.uid(), vol_meta.gid()) != owner {
            return Err(BackupHelperError::PathViolation);
        }

        // 2. Non-blocking, no-follow fstatat pre-check of DB and optional
        // WAL/SHM/journal companions before spawning the helper process.
        verify_dir_file_no_follow_fstatat(vol_data_fd.as_fd(), database, owner, true)?;
        for suffix in ["-wal", "-shm", "-journal"] {
            let companion = format!("{database}{suffix}");
            verify_dir_file_no_follow_fstatat(vol_data_fd.as_fd(), &companion, owner, false)?;
        }

        // 3. Create exclusive 0600 root-owned staging leaf under backup-staging.
        let staging_dir = self
            .backup_staging_dir()
            .map_err(|_| BackupHelperError::StoreFailed)?;
        let final_name = format!("{operation_id}.sqlite3");
        let dst_fd = open_at(
            staging_dir.as_fd(),
            &final_name,
            libc::O_RDWR | libc::O_CREAT | libc::O_EXCL,
            0o600,
        )
        .map_err(|_| BackupHelperError::StoreFailed)?;
        let dst_meta = File::from(
            dst_fd
                .try_clone()
                .map_err(|_| BackupHelperError::StoreFailed)?,
        )
        .metadata()
        .map_err(|_| BackupHelperError::StoreFailed)?;
        if !dst_meta.is_file()
            || dst_meta.nlink() != 1
            || dst_meta.uid() != self.owner
            || dst_meta.mode() & 0o7777 != 0o600
        {
            unlink_staging_artifacts(staging_dir.as_fd(), &final_name);
            return Err(BackupHelperError::StoreFailed);
        }
        let dst_path = self.state_path.join("backup-staging").join(&final_name);

        let result = (|| -> Result<StagedBackupMeta, BackupHelperError> {
            let remaining_ms = deadline_at
                .saturating_duration_since(Instant::now())
                .as_millis()
                .min(u128::from(u64::MAX)) as u64;
            if remaining_ms == 0 {
                return Err(BackupHelperError::TimedOut);
            }
            let isolate_root = self.owner == ROOT_UID;
            let cfg = LinuxBackupHelperConfig {
                database: database.to_owned(),
                dst_path: dst_path
                    .to_str()
                    .ok_or(BackupHelperError::PathViolation)?
                    .to_owned(),
                vol_path: if isolate_root {
                    None
                } else {
                    Some(format!("{}/{volume_name}/_data", self.volume_root))
                },
                max_bytes,
                deadline_ms: remaining_ms,
                uid: owner.0,
                gid: owner.1,
                isolate_root,
            };
            let cfg_bytes = serde_json::to_vec(&cfg).map_err(|_| BackupHelperError::StoreFailed)?;
            let (cfg_read, mut cfg_write) =
                io::pipe().map_err(|_| BackupHelperError::StoreFailed)?;
            let (res_read, res_write) = io::pipe().map_err(|_| BackupHelperError::StoreFailed)?;
            cfg_write
                .write_all(&cfg_bytes)
                .map_err(|_| BackupHelperError::StoreFailed)?;
            drop(cfg_write);

            // Dup descriptors above 9 with CLOEXEC so pre_exec dup2 onto fds 3..=6
            // can never collide with a source fd number.
            let vol_high = dup_above_cloexec(vol_data_fd.as_fd(), 10)
                .map_err(|_| BackupHelperError::StoreFailed)?;
            let dst_high = dup_above_cloexec(dst_fd.as_fd(), 10)
                .map_err(|_| BackupHelperError::StoreFailed)?;
            let cfg_high = dup_above_cloexec(cfg_read.as_fd(), 10)
                .map_err(|_| BackupHelperError::StoreFailed)?;
            let res_high = dup_above_cloexec(res_write.as_fd(), 10)
                .map_err(|_| BackupHelperError::StoreFailed)?;
            let vol_raw = vol_high.as_raw_fd();
            let dst_raw = dst_high.as_raw_fd();
            let cfg_raw = cfg_high.as_raw_fd();
            let res_raw = res_high.as_raw_fd();

            let exe = std::env::current_exe().map_err(|_| BackupHelperError::Unavailable)?;
            let mut cmd = Command::new(exe);
            cmd.env_clear()
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            #[cfg(not(test))]
            {
                cmd.arg("backup-sqlite-helper");
            }
            #[cfg(test)]
            {
                cmd.env("ARX_TEST_BACKUP_HELPER", "1").args([
                    "--exact",
                    "--test-threads=1",
                    "platform::linux::compose_tests::backup_sqlite_helper_test_trampoline",
                ]);
            }
            // SAFETY: closure runs between fork and exec and uses only
            // async-signal-safe libc calls (setpgid, dup2).
            unsafe {
                cmd.pre_exec(move || {
                    if libc::setpgid(0, 0) != 0 {
                        return Err(io::Error::last_os_error());
                    }
                    for (src, dst) in [
                        (vol_raw, LINUX_HELPER_VOL_FD),
                        (dst_raw, LINUX_HELPER_DST_STAT_FD),
                        (cfg_raw, LINUX_HELPER_CONFIG_FD),
                        (res_raw, LINUX_HELPER_RESULT_FD),
                    ] {
                        if libc::dup2(src, dst) < 0 {
                            return Err(io::Error::last_os_error());
                        }
                    }
                    Ok(())
                });
            }

            let mut child = cmd.spawn().map_err(|_| BackupHelperError::Unavailable)?;
            // Close parent ends of child-only fds immediately after spawn.
            drop(vol_high);
            drop(dst_high);
            drop(cfg_high);
            drop(res_high);
            drop(cfg_read);
            drop(res_write);

            // Wait for helper process with hard deadline; kill process group and reap on timeout.
            let hard_deadline = deadline_at + Duration::from_secs(2);
            let status = loop {
                if let Some(status) = child
                    .try_wait()
                    .map_err(|_| BackupHelperError::StoreFailed)?
                {
                    break Some(status);
                }
                if Instant::now() >= hard_deadline {
                    if let Ok(pgid) = libc::pid_t::try_from(child.id()) {
                        // SAFETY: child leads its own process group and has not been reaped.
                        unsafe { libc::kill(-pgid, libc::SIGKILL) };
                    }
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                thread::sleep(POLL_INTERVAL);
            };
            let Some(status) = status else {
                return Err(BackupHelperError::TimedOut);
            };

            let mut reply_bytes = Vec::new();
            let _ = res_read.take(1025).read_to_end(&mut reply_bytes);
            if let Ok(reply) = serde_json::from_slice::<LinuxBackupHelperReply>(&reply_bytes) {
                if !reply.ok {
                    let code = reply.error.as_deref().unwrap_or("backup_failed");
                    return Err(BackupHelperError::from_wire_code(code));
                }
            } else if !status.success() {
                return Err(BackupHelperError::BackupFailed);
            }
            if !status.success() {
                return Err(BackupHelperError::BackupFailed);
            }

            // 4. Normalize staged database to portable DELETE journal mode
            // (header bytes 18/19 = 1/1), flush leaf + staging directory, and
            // run PRAGMA quick_check + SHA-256 verification on the root-owned copy.
            {
                let conn = rusqlite::Connection::open_with_flags(
                    &dst_path,
                    rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE
                        | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX
                        | rusqlite::OpenFlags::SQLITE_OPEN_NOFOLLOW,
                )
                .map_err(|_| BackupHelperError::Corrupt)?;
                conn.execute_batch("PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL;")
                    .map_err(|_| BackupHelperError::StoreFailed)?;
            }
            for suffix in ["-wal", "-shm", "-journal"] {
                if let Ok(cname) = component(&format!("{final_name}{suffix}")) {
                    // SAFETY: staging_dir is a live directory descriptor.
                    unsafe { libc::unlinkat(staging_dir.as_raw_fd(), cname.as_ptr(), 0) };
                }
            }
            let final_file = File::from(
                open_at(staging_dir.as_fd(), &final_name, libc::O_RDWR, 0)
                    .map_err(|_| BackupHelperError::StoreFailed)?,
            );
            let final_meta = final_file
                .metadata()
                .map_err(|_| BackupHelperError::StoreFailed)?;
            if !final_meta.is_file()
                || final_meta.nlink() != 1
                || final_meta.uid() != self.owner
                || final_meta.mode() & 0o7777 != 0o600
                || (final_meta.dev(), final_meta.ino()) != (dst_meta.dev(), dst_meta.ino())
            {
                return Err(BackupHelperError::PathViolation);
            }
            final_file
                .sync_all()
                .map_err(|_| BackupHelperError::StoreFailed)?;
            drop(final_file);
            // SAFETY: staging_dir is a live directory descriptor.
            if unsafe { libc::fsync(staging_dir.as_raw_fd()) } != 0 {
                return Err(BackupHelperError::StoreFailed);
            }
            verify_portable_sqlite_file(&dst_path, max_bytes)
        })();

        drop(dst_fd);
        drop(vol_data_fd);
        if result.is_err() {
            unlink_staging_artifacts(staging_dir.as_fd(), &final_name);
        }
        result
    }

    fn read_backup_chunk(
        &mut self,
        operation_id: &str,
        offset: u64,
        len: usize,
    ) -> Result<Vec<u8>, StoreError> {
        if operation_id.len() != 32
            || !operation_id
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
            || len == 0
            || len > compose_backup::BACKUP_CHUNK_BYTES
        {
            return Err(StoreError);
        }
        let dir = self.backup_staging_dir().map_err(|_| StoreError)?;
        let name = format!("{operation_id}.sqlite3");
        let mut file =
            File::from(open_at(dir.as_fd(), &name, libc::O_RDONLY, 0).map_err(|_| StoreError)?);
        let meta = file.metadata().map_err(|_| StoreError)?;
        if !meta.is_file()
            || meta.uid() != self.owner
            || meta.mode() & 0o7777 != 0o600
            || meta.nlink() != 1
        {
            return Err(StoreError);
        }
        file.seek(SeekFrom::Start(offset)).map_err(|_| StoreError)?;
        let mut buf = vec![0u8; len];
        file.read_exact(&mut buf).map_err(|_| StoreError)?;
        Ok(buf)
    }

    fn verify_backup_staging(
        &mut self,
        operation_id: &str,
        expected_bytes: u64,
        expected_digest: &str,
    ) -> bool {
        if operation_id.len() != 32
            || !operation_id
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        {
            return false;
        }
        let Ok(dir) = self.backup_staging_dir() else {
            return false;
        };
        let name = format!("{operation_id}.sqlite3");
        let Ok(fd) = open_at(dir.as_fd(), &name, libc::O_RDONLY, 0) else {
            return false;
        };
        let Ok(fd_meta) = File::from(fd).metadata() else {
            return false;
        };
        if !fd_meta.is_file()
            || fd_meta.uid() != self.owner
            || fd_meta.mode() & 0o7777 != 0o600
            || fd_meta.nlink() != 1
        {
            return false;
        }
        let plain_path = self.state_path.join("backup-staging").join(&name);
        let Ok(path_meta) = fs::symlink_metadata(&plain_path) else {
            return false;
        };
        if path_meta.file_type().is_symlink()
            || (path_meta.dev(), path_meta.ino()) != (fd_meta.dev(), fd_meta.ino())
        {
            return false;
        }
        let Ok(meta) = verify_portable_sqlite_file(&plain_path, MAX_BACKUP_BYTES) else {
            return false;
        };
        meta.total_bytes == expected_bytes && meta.digest == expected_digest
    }

    fn remove_backup_staging(&mut self, operation_id: &str) -> Result<bool, StoreError> {
        if operation_id.len() != 32
            || !operation_id
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        {
            return Err(StoreError);
        }
        let dir = self.backup_staging_dir().map_err(|_| StoreError)?;
        let name = format!("{operation_id}.sqlite3");
        let cname = component(&name).map_err(|_| StoreError)?;
        // SAFETY: dir is a live root-owned staging dir fd and cname is NUL-terminated.
        let rc = unsafe { libc::unlinkat(dir.as_raw_fd(), cname.as_ptr(), 0) };
        if rc == 0 {
            let _ = unsafe { libc::fsync(dir.as_raw_fd()) };
            Ok(true)
        } else {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::NotFound {
                Ok(false)
            } else {
                Err(StoreError)
            }
        }
    }
}

fn dup_above_cloexec(fd: BorrowedFd<'_>, min_fd: libc::c_int) -> io::Result<OwnedFd> {
    // SAFETY: fd is a live descriptor and F_DUPFD_CLOEXEC returns a fresh fd >= min_fd.
    let dup = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_DUPFD_CLOEXEC, min_fd) };
    if dup < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: dup is a fresh descriptor owned by this caller.
    Ok(unsafe { OwnedFd::from_raw_fd(dup) })
}

fn unlink_staging_artifacts(dir: BorrowedFd<'_>, leaf_name: &str) {
    for suffix in ["", "-wal", "-shm", "-journal"] {
        if let Ok(cname) = component(&format!("{leaf_name}{suffix}")) {
            // SAFETY: dir is a live directory descriptor and cname is NUL-terminated.
            unsafe { libc::unlinkat(dir.as_raw_fd(), cname.as_ptr(), 0) };
        }
    }
}

fn component(name: &str) -> io::Result<std::ffi::CString> {
    if name.is_empty() || name == "." || name == ".." || name.contains('/') {
        return Err(invalid());
    }
    std::ffi::CString::new(name).map_err(|_| invalid())
}

fn open_at(
    dir: BorrowedFd<'_>,
    name: &str,
    flags: libc::c_int,
    mode: libc::mode_t,
) -> io::Result<OwnedFd> {
    let name = component(name)?;
    // SAFETY: dir is a live descriptor and name a NUL-terminated component.
    let fd = unsafe {
        libc::openat(
            dir.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            libc::c_uint::from(mode),
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: openat returned a fresh descriptor owned by the caller.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

/// Opens an absolute path one component at a time from `/`. Every directory
/// on the way must be owned by `owner` or root and not group/other writable.
fn walk_owned_dirs(path: &str, owner: u32) -> io::Result<OwnedFd> {
    let rest = path.strip_prefix('/').ok_or_else(invalid)?;
    let mut dir: OwnedFd = File::open("/")?.into();
    for name in rest.split('/') {
        let next = open_at(dir.as_fd(), name, libc::O_RDONLY | libc::O_DIRECTORY, 0)?;
        let metadata = File::from(next.try_clone()?).metadata()?;
        if !metadata.is_dir()
            || (metadata.uid() != owner && metadata.uid() != ROOT_UID)
            || metadata.mode() & 0o022 != 0
        {
            return Err(invalid());
        }
        dir = next;
    }
    Ok(dir)
}

fn verify_private_dir(dir: BorrowedFd<'_>, owner: u32) -> io::Result<()> {
    let metadata = File::from(dir.try_clone_to_owned()?).metadata()?;
    if !metadata.is_dir() || metadata.uid() != owner || metadata.mode() & 0o077 != 0 {
        return Err(invalid());
    }
    Ok(())
}

/// Opens, and optionally creates, a 0700 child directory owned by `owner`.
fn private_subdir(
    parent: BorrowedFd<'_>,
    name: &str,
    owner: u32,
    create: bool,
) -> io::Result<OwnedFd> {
    if create {
        let cname = component(name)?;
        // SAFETY: parent is a live descriptor and cname a NUL-terminated component.
        if unsafe { libc::mkdirat(parent.as_raw_fd(), cname.as_ptr(), 0o700) } != 0 {
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::AlreadyExists {
                return Err(error);
            }
        }
    }
    let dir = open_at(parent, name, libc::O_RDONLY | libc::O_DIRECTORY, 0)?;
    verify_private_dir(dir.as_fd(), owner)?;
    Ok(dir)
}

/// Replaces `name` in `dir` atomically. The temporary is created O_EXCL and
/// no-follow, receives its owner and mode on the descriptor, is written and
/// synced, then renamed. Owner-bound secret instances require an unused target
/// name. State and revision files use replacing rename. Neither follows links.
fn replace_at(
    dir: BorrowedFd<'_>,
    name: &str,
    bytes: &[u8],
    mode: libc::mode_t,
    owner: Option<(u32, u32)>,
) -> io::Result<SecretFileWitness> {
    let target = component(name)?;
    let temporary_name = format!(".{name}.tmp");
    let temporary = component(&temporary_name)?;
    // SAFETY: dir is live and temporary a NUL-terminated component.
    if unsafe { libc::unlinkat(dir.as_raw_fd(), temporary.as_ptr(), 0) } != 0 {
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::NotFound {
            return Err(error);
        }
    }
    let mut file = File::from(open_at(
        dir,
        &temporary_name,
        libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
        mode,
    )?);
    let written = (|| -> io::Result<SecretFileWitness> {
        if let Some((uid, gid)) = owner {
            std::os::unix::fs::fchown(&file, Some(uid), Some(gid))?;
        }
        file.set_permissions(fs::Permissions::from_mode(mode))?;
        file.write_all(bytes)?;
        file.sync_all()?;
        let metadata = file.metadata()?;
        Ok(SecretFileWitness {
            dev: metadata.dev(),
            ino: metadata.ino(),
            uid: metadata.uid(),
            gid: metadata.gid(),
        })
    })();
    drop(file);
    // SAFETY: both names are NUL-terminated components of the live dir.
    let renamed = written.and_then(|witness| {
        if unsafe {
            libc::renameat2(
                dir.as_raw_fd(),
                temporary.as_ptr(),
                dir.as_raw_fd(),
                target.as_ptr(),
                if owner.is_some() {
                    libc::RENAME_NOREPLACE
                } else {
                    0
                },
            )
        } != 0
        {
            Err(io::Error::last_os_error())
        } else {
            Ok(witness)
        }
    });
    let witness = match renamed {
        Ok(witness) => witness,
        Err(error) => {
            // SAFETY: as above.
            unsafe { libc::unlinkat(dir.as_raw_fd(), temporary.as_ptr(), 0) };
            return Err(error);
        }
    };
    // SAFETY: dir is a live descriptor.
    if unsafe { libc::fsync(dir.as_raw_fd()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(witness)
}

fn is_tmpfs(dir: BorrowedFd<'_>) -> io::Result<bool> {
    // SAFETY: fstatfs initializes the storage on success; dir stays open.
    unsafe {
        let mut stat = std::mem::MaybeUninit::<libc::statfs>::uninit();
        if libc::fstatfs(dir.as_raw_fd(), stat.as_mut_ptr()) != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(stat.assume_init().f_type == libc::TMPFS_MAGIC)
    }
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
            let observed = observe(&mut child, Instant::now() + TEST_HANG_GUARD).unwrap();
            if matches!(observed, Observation::Deadline) {
                child.kill().unwrap();
                reap(&mut child, Instant::now() + TEST_HANG_GUARD).unwrap();
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
        assert!(!child.wait_exec(Instant::now() + TEST_HANG_GUARD).unwrap());
        reap(&mut child, Instant::now() + TEST_HANG_GUARD).unwrap();
    }

    #[test]
    fn short_deadline_kills_process_group_and_reaps_owned_child() {
        let mut gate = HeldGate::open();
        let (mut pipe, output) = io::pipe().unwrap();
        let mut child = launched(
            concat!(
                "/bin/sh -c 'read _ <&3 || { printf '\\''%s\\n'\\'' failed; exit 1; }; ",
                "printf '\\''%s\\n'\\'' held; read _ <&3' & printf '%s\\n' \"$!\"; wait"
            ),
            ChildSetup {
                stdout: Some(output.as_raw_fd()),
                gate: Some(gate.fd()),
                ..ChildSetup::default()
            },
        );
        drop(output);
        let descendant = read_pid(&mut pipe);
        // SAFETY: getpgid reads kernel state and has no pointer parameters.
        assert_eq!(unsafe { libc::getpgid(descendant) }, child.pid);
        assert_blocked_until_release(&mut pipe, &mut gate);
        // The second read is still blocked, so the descendant is still in the group.
        assert_eq!(unsafe { libc::getpgid(descendant) }, child.pid);
        // The descendant is held on the gate, so this deadline is already due
        // and observe classifies it without sleeping.
        assert!(matches!(
            observe(&mut child, Instant::now()).unwrap(),
            Observation::Deadline
        ));
        let pid = child.pid;
        child.kill().unwrap();
        reap(&mut child, Instant::now() + TEST_HANG_GUARD).unwrap();
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
            reap(&mut child, Instant::now() + TEST_HANG_GUARD).unwrap();
        }
        assert_eq!(release.expect("stalled child kept the port open"), 0);
        assert_eq!(inherited, 0, "the first child step did not close the port");
        assert!(reaped, "the watchdog did not reap the stalled child");
        assert_eq!(result, Err("script containment failed"));
    }

    #[test]
    fn production_exec_pipe_deadline_times_out() {
        // The stall watchdog replaces wait_exec. This fixture calls the
        // production pipe wait. The child holds the writer, and the injected
        // clock is still inside the budget, so only wait_readable's deadline
        // can time out.
        let (started, ready) = io::pipe().unwrap();
        let (gate, _held_gate) = io::pipe().unwrap();
        let mut held = PreparedScript::new("exit 7")
            .unwrap()
            .fork(
                &ReapOwner::verify().unwrap(),
                ChildSetup {
                    stall: Some(StallBeforeExec {
                        ready: ready.as_raw_fd(),
                        gate: gate.as_raw_fd(),
                    }),
                    ..ChildSetup::default()
                },
            )
            .unwrap();
        let expired = Instant::now();
        held.exec_now = Some(
            expired
                .checked_sub(Duration::from_millis(1))
                .expect("deadline is not the monotonic epoch"),
        );
        let error = held.wait_exec(expired).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(
            !held.deadline_boundary_reached,
            "already-due deadline must time out inside wait_readable"
        );
        held.kill().unwrap();
        reap(&mut held, Instant::now() + TEST_HANG_GUARD).unwrap();
        drop((started, ready, gate));

        // Exec closes the writer. Wait for that close under the hang guard:
        // expiry fails the test and is not the timeout under assertion.
        // The injected clock is then equal to a still-future deadline, so
        // only the production comparison can return TimedOut.
        let mut child = PreparedScript::new("exit 0")
            .unwrap()
            .fork(&ReapOwner::verify().unwrap(), ChildSetup::default())
            .unwrap();
        wait_readable(
            child.exec_reader.as_raw_fd(),
            Instant::now() + TEST_HANG_GUARD,
        )
        .expect("hang guard expired before the exec pipe was readable");
        let deadline = Instant::now() + Duration::from_secs(60);
        child.exec_now = Some(deadline);
        let error = child.wait_exec(deadline).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(
            child.deadline_boundary_reached,
            "timeout was not produced by the production deadline comparison"
        );
        reap(&mut child, Instant::now() + TEST_HANG_GUARD).unwrap();
    }

    #[test]
    fn production_cleanup_writes_kill_and_waits_for_empty_before_result() {
        let fixture = Arc::new(FakeCgroups::new());
        let release = Arc::clone(&fixture);
        let peer = thread::spawn(move || -> io::Result<()> {
            // The kill write is the latch. The hang guard only fails a peer
            // that never observes it; the success path does not sleep a budget.
            let deadline = Instant::now() + TEST_HANG_GUARD;
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
        // The held gate in this fixture runs only where a delegated cgroup v2
        // tree exists. Local QEMU provisions that tree; hosted CI does not, so
        // discovery fails and the test prints SKIP. That return is not gate
        // coverage. Hosted dash covers the same gate in the process-group
        // fixture, short_deadline_kills_process_group_and_reaps_owned_child.
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
                // One line, then return. This is not a pass of the held gate.
                eprintln!("SKIP delegated-cgroup: {reason:?}");
                return;
            }
        };
        let mut group = root.new_command().unwrap();
        let mut gate = HeldGate::open();
        let (mut pipe, output) = io::pipe().unwrap();
        let mut child = launched(
            concat!(
                "setsid /bin/sh -c 'printf \"%s\\n\" \"$$\"; ",
                "read _ <&3 || { printf '\\''%s\\n'\\'' failed; exit 1; }; ",
                "printf '\\''%s\\n'\\'' held; read _ <&3' & exit 0"
            ),
            ChildSetup {
                procs: Some(group.procs.as_raw_fd()),
                drop_privileges: owner == 0,
                stdout: Some(output.as_raw_fd()),
                gate: Some(gate.fd()),
                ..ChildSetup::default()
            },
        );
        drop(output);
        let descendant = read_pid(&mut pipe);
        // SAFETY: getsid reads kernel state and has no pointer parameters.
        assert_eq!(unsafe { libc::getsid(descendant) }, descendant);
        assert_blocked_until_release(&mut pipe, &mut gate);
        assert_eq!(unsafe { libc::getsid(descendant) }, descendant);
        assert_eq!(
            observe(&mut child, Instant::now() + TEST_HANG_GUARD)
                .unwrap()
                .outcome(),
            ScriptOutcome::Exited { exit_code: 0 }
        );
        assert!(group.populated().unwrap());
        assert!(matches!(discover(), Err(Qualification::CommandSlot)));
        group.kill().unwrap();
        group.wait_empty(Instant::now() + TEST_HANG_GUARD).unwrap();
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
                        TEST_HANG_GUARD,
                    )
                    .unwrap(),
                ScriptOutcome::Exited { exit_code: 0 }
            );
        }
        // The loop cannot finish. Reaching `held` above and spending this
        // whole budget are both required; a return before either is not coverage.
        let timeout = Duration::from_millis(50);
        let started = Instant::now();
        assert_eq!(
            runner.run("while :; do :; done", timeout).unwrap(),
            ScriptOutcome::TimedOut
        );
        assert!(
            started.elapsed() >= timeout,
            "delegated-cgroup returned before the production timeout"
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
                max_budget: TEST_HANG_GUARD,
                cleanup_budget: TEST_HANG_GUARD,
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
                // Failure bound for scripts that do exit. Aligned with the hang guard.
                timeout_s: 5,
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
        assert!(child.wait_exec(Instant::now() + TEST_HANG_GUARD).unwrap());
        child
    }

    fn read_pid(pipe: &mut PipeReader) -> libc::pid_t {
        read_line(pipe).trim_end().parse().unwrap()
    }

    fn read_line(pipe: &mut PipeReader) -> String {
        let mut line = Vec::new();
        let deadline = Instant::now() + TEST_HANG_GUARD;
        loop {
            wait_readable(pipe.as_raw_fd(), deadline)
                .expect("hang guard expired before the fixture line");
            let mut byte = [0];
            assert_eq!(
                pipe.read(&mut byte).unwrap(),
                1,
                "stdout closed before the fixture line finished"
            );
            line.push(byte[0]);
            if byte[0] == b'\n' {
                break;
            }
            assert!(line.len() <= 32, "fixture line is too long");
        }
        String::from_utf8(line).unwrap()
    }

    /// The descendant must stay blocked until this test writes a line. A shell
    /// that rejected the redirection prints `failed` and never prints `held`.
    fn assert_blocked_until_release(output: &mut PipeReader, gate: &mut HeldGate) {
        let mut poll = libc::pollfd {
            fd: output.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let ready = loop {
            // SAFETY: one initialized pollfd. A zero timeout does not sleep.
            match unsafe { libc::poll(&mut poll, 1, 0) } {
                -1 if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted => continue,
                ready => break ready,
            }
        };
        assert_eq!(
            ready, 0,
            "descendant wrote or closed stdout before the gate was released"
        );
        gate.release_line();
        assert_eq!(
            read_line(output).trim_end(),
            "held",
            "descendant must block on the gate until this test releases it"
        );
    }

    fn ready(pipe: &impl AsRawFd) {
        wait_readable(pipe.as_raw_fd(), Instant::now() + TEST_HANG_GUARD).unwrap();
    }

    /// Read end inherited above the descriptors dash can redirect. The child
    /// maps it to fd 3 before exec. The write end stays open so `read` blocks
    /// until `release_line`.
    struct HeldGate {
        read: OwnedFd,
        write: PipeWriter,
    }

    impl HeldGate {
        fn open() -> Self {
            let (reader, write) = io::pipe().unwrap();
            // SAFETY: F_DUPFD duplicates an open pipe at or above fd 10 and
            // leaves close-on-exec clear. The pre-exec setup maps that
            // descriptor to fd 3; the shell script never names this number.
            let dup = unsafe { libc::fcntl(reader.as_raw_fd(), libc::F_DUPFD, HELD_GATE_FLOOR) };
            assert!(
                dup >= HELD_GATE_FLOOR,
                "gate descriptor was not allocated above the shell limit"
            );
            drop(reader);
            Self {
                // SAFETY: F_DUPFD returned a fresh descriptor owned by this process.
                read: unsafe { OwnedFd::from_raw_fd(dup) },
                write,
            }
        }

        fn fd(&self) -> RawFd {
            self.read.as_raw_fd()
        }

        fn release_line(&mut self) {
            self.write.write_all(b"x\n").unwrap();
        }
    }
}

#[cfg(test)]
mod compose_tests {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, symlink};
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;

    fn run(program: &'static str, argv: &[&str], timeout: Duration) -> RunResult {
        DockerRunner { program, cwd: "/" }.run(&Invocation {
            argv: argv.iter().map(|arg| (*arg).to_owned()).collect(),
            timeout,
            capture: true,
        })
    }

    #[test]
    fn docker_runner_starts_with_only_the_fixed_environment() {
        let result = run("/usr/bin/env", &[], TEST_HANG_GUARD);
        let RunResult::Exited { code: 0, stdout } = result else {
            panic!("{result:?}");
        };
        let mut lines: Vec<String> = String::from_utf8(stdout)
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect();
        lines.sort();
        let mut expected: Vec<String> = compose::ENGINE_ENV
            .iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect();
        expected.sort();
        assert_eq!(lines, expected);
    }

    #[test]
    fn docker_runner_bounds_output_and_classifies_ends() {
        let exact_cmd = format!("yes | head -c {}", compose::MAX_CAPTURE);
        let exact = run("/bin/sh", &["-c", &exact_cmd], TEST_HANG_GUARD);
        let RunResult::Exited { code: 0, stdout } = exact else {
            panic!("{exact:?}");
        };
        assert_eq!(stdout.len(), compose::MAX_CAPTURE);

        // Output exceeding MAX_CAPTURE is explicitly reported as incomplete so
        // a truncated prefix can never be mistaken for a complete inventory.
        assert_eq!(
            run("/bin/sh", &["-c", "yes | head -c 40000"], TEST_HANG_GUARD),
            RunResult::OutputIncomplete { code: 0 }
        );

        let started = Instant::now();
        assert_eq!(
            run("/bin/sh", &["-c", "sleep 30"], Duration::from_millis(100)),
            RunResult::TimedOut
        );
        assert!(started.elapsed() < TEST_HANG_GUARD, "deadline kill hung");
        assert_eq!(
            run("/bin/sh", &["-c", "kill -TERM $$"], TEST_HANG_GUARD),
            RunResult::Signaled
        );
        assert_eq!(
            run("/nonexistent/arx-docker", &[], TEST_HANG_GUARD),
            RunResult::LaunchFailed
        );
    }

    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let raw = std::env::temp_dir().join(format!(
                "arx-compose-store-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::DirBuilder::new().mode(0o700).create(&raw).unwrap();
            let path = fs::canonicalize(&raw).unwrap();
            for dir in [
                path.join("state"),
                path.join("secrets"),
                path.join("volumes"),
            ] {
                fs::DirBuilder::new().mode(0o700).create(dir).unwrap();
            }
            Self(path)
        }

        fn store(&self) -> LinuxComposeStore {
            LinuxComposeStore {
                state: File::open(self.0.join("state")).unwrap().into(),
                state_path: self.0.join("state"),
                secrets: File::open(self.0.join("secrets")).unwrap().into(),
                volume_root: self.0.join("volumes").to_string_lossy().into_owned(),
                volume_root_fd: Some(File::open(self.0.join("volumes")).unwrap().into()),
                owner: effective_uid(),
            }
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn static_compose_marker_refuses_replaced_or_untrusted_files() {
        use std::os::unix::fs::symlink;
        let scratch = Scratch::new();
        let dir = File::open(&scratch.0).unwrap();
        let marker = scratch.0.join("compose-finalized");
        let check = || finalized_marker(dir.as_fd(), effective_uid());
        assert!(!check().unwrap());
        symlink("missing", &marker).unwrap();
        assert!(check().is_err());
        fs::remove_file(&marker).unwrap();
        fs::write(&marker, "ubuntu-noble-amd64-compose-v1\n").unwrap();
        fs::set_permissions(&marker, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(check().unwrap());
        fs::set_permissions(&marker, fs::Permissions::from_mode(0o666)).unwrap();
        assert!(check().is_err());
        fs::set_permissions(&marker, fs::Permissions::from_mode(0o600)).unwrap();
        fs::write(&marker, "ubuntu-noble-amd64-compose-v1\ntrailing").unwrap();
        assert!(check().is_err());
    }

    #[test]
    fn store_replaces_files_fd_relative_and_never_follows_links() {
        let scratch = Scratch::new();
        let mut store = scratch.store();
        // SAFETY: getegid has no arguments and no failure mode.
        let owner = (effective_uid(), unsafe { libc::getegid() });

        assert_eq!(store.load_state(), Ok(None));
        store.save_state(b"{\"saved\":1}").unwrap();
        assert_eq!(store.load_state(), Ok(Some(b"{\"saved\":1}".to_vec())));

        store.write_revision("calm-life", 1, "name: x\n").unwrap();
        let yaml = scratch
            .0
            .join("state/v1/projects/calm-life/r1/compose.yaml");
        assert_eq!(fs::read(&yaml).unwrap(), b"name: x\n");
        assert_eq!(fs::metadata(&yaml).unwrap().mode() & 0o777, 0o600);

        assert!(
            store
                .stat_secret("calm-life", "admin_password", owner)
                .is_none()
        );
        assert_eq!(
            store.compare_secret("calm-life", "admin_password", owner, b"first"),
            SecretMatch::Missing
        );
        let first_witness = store
            .write_secret("calm-life", "admin_password", owner, b"first")
            .unwrap();
        let secret = scratch.0.join("secrets/calm-life/admin_password");
        assert_eq!(fs::metadata(&secret).unwrap().mode() & 0o7777, 0o400);
        assert_eq!(
            store.stat_secret("calm-life", "admin_password", owner),
            Some(first_witness)
        );
        assert_eq!(
            store.compare_secret("calm-life", "admin_password", owner, b"first"),
            SecretMatch::Match(first_witness)
        );
        assert_eq!(
            store.compare_secret("calm-life", "admin_password", owner, b"other"),
            SecretMatch::Mismatch
        );
        assert!(
            store
                .stat_secret("calm-life", "admin_password", (owner.0 + 1, owner.1))
                .is_none()
        );

        // Immutable publication refuses both an existing inode and a planted link.
        assert_eq!(
            store.write_secret("calm-life", "admin_password", owner, b"second"),
            Err(StoreError)
        );
        assert_eq!(
            store.stat_secret("calm-life", "admin_password", owner),
            Some(first_witness)
        );
        let outside = scratch.0.join("outside");
        fs::write(&outside, b"untouched").unwrap();
        fs::remove_file(&secret).unwrap();
        symlink(&outside, &secret).unwrap();
        assert!(
            store
                .stat_secret("calm-life", "admin_password", owner)
                .is_none()
        );
        assert_eq!(
            store.compare_secret("calm-life", "admin_password", owner, b"untouched"),
            SecretMatch::Error
        );
        symlink(
            &outside,
            scratch.0.join("secrets/calm-life/.admin_password.tmp"),
        )
        .unwrap();
        assert_eq!(
            store.write_secret("calm-life", "admin_password", owner, b"second"),
            Err(StoreError)
        );
        assert_eq!(fs::read(&outside).unwrap(), b"untouched");
        assert!(fs::symlink_metadata(&secret).unwrap().is_symlink());

        // A linked project directory is refused.
        let elsewhere = scratch.0.join("elsewhere");
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&elsewhere)
            .unwrap();
        symlink(&elsewhere, scratch.0.join("secrets/evil")).unwrap();
        assert_eq!(
            store.write_secret("evil", "admin_password", owner, b"value"),
            Err(StoreError)
        );
        assert!(fs::read_dir(&elsewhere).unwrap().next().is_none());

        assert!(walk_owned_dirs("relative/path", owner.0).is_err());
        assert!(
            walk_owned_dirs("/proc/self", owner.0).is_err(),
            "followed a link"
        );
    }

    #[test]
    fn docker_runner_discards_stderr_and_non_capturing_stdout_canary() {
        let canary = format!("arx-proc-canary-{:016x}", 0x1234_5678_9abc_def0_u64);
        let script = format!("printf '%s\\n' '{canary}'; printf '%s\\n' '{canary}' >&2; exit 1");
        let non_capturing = DockerRunner {
            program: "/bin/sh",
            cwd: "/",
        }
        .run(&Invocation {
            argv: vec!["-c".to_owned(), script],
            timeout: TEST_HANG_GUARD,
            capture: false,
        });
        assert_eq!(
            non_capturing,
            RunResult::Exited {
                code: 1,
                stdout: Vec::new(),
            },
            "non-capturing invocation leaked subprocess output"
        );

        let stderr_script = format!("printf 'ok\\n'; printf '%s\\n' '{canary}' >&2");
        let capturing = run("/bin/sh", &["-c", &stderr_script], TEST_HANG_GUARD);
        let RunResult::Exited { code: 0, stdout } = capturing else {
            panic!("{capturing:?}");
        };
        assert_eq!(stdout, b"ok\n");
        assert!(
            !String::from_utf8_lossy(&stdout).contains(&canary),
            "stderr canary reached captured stdout"
        );
    }

    #[test]
    fn compose_qualified_guest_disables_legacy_scripts() {
        use arx_guest::{Request, Response, ScriptStatus, read_frame, write_frame};

        let scratch = Scratch::new();
        let compose = ComposeService::new(
            Box::new(DockerRunner {
                program: "/bin/false",
                cwd: "/",
            }),
            Box::new(scratch.store()),
            Box::new(SystemClock),
        )
        .unwrap();
        let mut requests = Vec::new();
        write_frame(
            &mut requests,
            &Request::Hello {
                version: 1,
                challenge: "0123456789abcdef0123456789abcdef".to_owned(),
            },
        )
        .unwrap();
        write_frame(
            &mut requests,
            &Request::RunScript {
                id: 1,
                script: "exit 7".to_owned(),
                timeout_s: 5,
            },
        )
        .unwrap();
        let mut responses = Vec::new();
        let root = CgroupRoot {
            path: PathBuf::from("/proc/self/cgroup"),
            owner: effective_uid(),
        };
        let runner = ScriptRunner::new(root, None, ReapOwner::verify().unwrap());
        assert_eq!(
            serve_connection_with(
                requests.as_slice(),
                &mut responses,
                Some(runner),
                Some(&compose),
            ),
            Ok(())
        );
        let mut reader = responses.as_slice();
        assert!(matches!(
            read_frame::<Response>(&mut reader).unwrap(),
            Some(Response::HelloAck {
                script_execution: ScriptExecution::Unavailable,
                ..
            })
        ));
        assert!(matches!(
            read_frame::<Response>(&mut reader).unwrap(),
            Some(Response::ScriptResult {
                id: 1,
                status: ScriptStatus::Unavailable,
                exit_code: None,
            })
        ));
        assert!(reader.is_empty());
    }

    #[test]
    fn backup_sqlite_helper_test_trampoline() {
        if std::env::var_os("ARX_TEST_BACKUP_HELPER").is_some() {
            let code = compose_backup::run_linux_backup_helper_main();
            std::process::exit(if code == std::process::ExitCode::SUCCESS {
                0
            } else {
                1
            });
        }
    }

    #[test]
    fn linux_compose_store_runs_sqlite_backup_and_refuses_symlinks_fifos_and_foreign_owners() {
        let scratch = Scratch::new();
        let mut store = scratch.store();
        let uid = effective_uid();
        // SAFETY: getegid is always safe and side-effect-free.
        let gid = unsafe { libc::getegid() };
        let owner = (uid, gid);

        let vol_name = "arx-calm-life-data";
        let vol_dir = scratch.0.join("volumes").join(vol_name);
        let vol_data = vol_dir.join("_data");
        fs::DirBuilder::new().mode(0o700).create(&vol_dir).unwrap();
        fs::DirBuilder::new().mode(0o700).create(&vol_data).unwrap();

        let db_path = vol_data.join("calm-life.sqlite3");
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA wal_autocheckpoint=0;
             CREATE TABLE events(id INTEGER PRIMARY KEY, label TEXT NOT NULL, val INTEGER NOT NULL);
             INSERT INTO events(label, val) VALUES ('boot', 1), ('wal_uncheckpointed', 42);",
        )
        .unwrap();
        assert!(vol_data.join("calm-life.sqlite3-wal").exists());

        // 1. End-to-end WAL backup via isolated helper subprocess.
        let op1 = "11111111111111111111111111111111";
        let meta = store
            .run_sqlite_backup(
                op1,
                vol_name,
                owner,
                "calm-life.sqlite3",
                MAX_BACKUP_BYTES,
                Duration::from_secs(15),
            )
            .unwrap();
        assert!(meta.total_bytes >= 4096);
        assert_eq!(meta.digest.len(), 64);

        let staging_dir = scratch.0.join("state/backup-staging");
        let staged_leaf = staging_dir.join(format!("{op1}.sqlite3"));
        let staged_meta = fs::symlink_metadata(&staged_leaf).unwrap();
        assert_eq!(staged_meta.mode() & 0o7777, 0o600);
        assert_eq!(staged_meta.nlink(), 1);
        let staged_bytes = fs::read(&staged_leaf).unwrap();
        assert_eq!(&staged_bytes[18..20], &[1, 1]);
        for suffix in ["-wal", "-shm", "-journal"] {
            assert!(!staging_dir.join(format!("{op1}.sqlite3{suffix}")).exists());
        }
        // Only the staged leaf exists in backup-staging (no .work-* leftovers).
        let entries: Vec<_> = fs::read_dir(&staging_dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(entries.len(), 1);

        // Verify staging check, exact chunk read, and offline restore of WAL rows.
        assert!(store.verify_backup_staging(op1, meta.total_bytes, &meta.digest));
        assert!(!store.verify_backup_staging(op1, meta.total_bytes + 1, &meta.digest));
        let first_len = (meta.total_bytes as usize).min(compose_backup::BACKUP_CHUNK_BYTES);
        let chunk0 = store.read_backup_chunk(op1, 0, first_len).unwrap();
        assert_eq!(chunk0, &staged_bytes[..first_len]);

        let restore_dir = scratch.0.join("restore");
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&restore_dir)
            .unwrap();
        let report = compose_backup::restore_sqlite_offline(
            &staged_leaf,
            &restore_dir,
            "restored.sqlite3",
            &meta.digest,
            MAX_BACKUP_BYTES,
        )
        .unwrap();
        let restored = rusqlite::Connection::open(&report.path).unwrap();
        let rows: Vec<(String, i64)> = restored
            .prepare("SELECT label, val FROM events ORDER BY id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(
            rows,
            vec![
                ("boot".to_owned(), 1),
                ("wal_uncheckpointed".to_owned(), 42)
            ]
        );
        drop(restored);

        // Release removes staged leaf cleanly.
        assert_eq!(store.remove_backup_staging(op1), Ok(true));
        assert!(!staged_leaf.exists());
        assert_eq!(store.remove_backup_staging(op1), Ok(false));

        // 2. Foreign-owned file / volume owner mismatch is refused.
        let wrong_owner = (uid.saturating_add(1), gid);
        assert_eq!(
            store.run_sqlite_backup(
                "22222222222222222222222222222222",
                vol_name,
                wrong_owner,
                "calm-life.sqlite3",
                MAX_BACKUP_BYTES,
                Duration::from_secs(10),
            ),
            Err(BackupHelperError::PathViolation)
        );
        let vol_fd = open_at(
            File::open(&vol_dir).unwrap().as_fd(),
            "_data",
            libc::O_RDONLY | libc::O_DIRECTORY,
            0,
        )
        .unwrap();
        assert_eq!(
            verify_dir_file_no_follow_fstatat(
                vol_fd.as_fd(),
                "calm-life.sqlite3",
                wrong_owner,
                true,
            ),
            Err(BackupHelperError::PathViolation)
        );
        drop(vol_fd);
        drop(conn);

        // 3. FIFO companion (<db>-journal) returns PathViolation quickly without hanging.
        let fifo_path = vol_data.join("calm-life.sqlite3-journal");
        let fifo_cstr = std::ffi::CString::new(fifo_path.to_str().unwrap()).unwrap();
        // SAFETY: fifo_cstr is NUL-terminated and 0o600 is a valid FIFO mode.
        assert_eq!(unsafe { libc::mkfifo(fifo_cstr.as_ptr(), 0o600) }, 0);
        let fifo_start = Instant::now();
        assert_eq!(
            store.run_sqlite_backup(
                "33333333333333333333333333333333",
                vol_name,
                owner,
                "calm-life.sqlite3",
                MAX_BACKUP_BYTES,
                Duration::from_secs(10),
            ),
            Err(BackupHelperError::PathViolation)
        );
        assert!(
            fifo_start.elapsed() < Duration::from_secs(2),
            "FIFO companion check blocked"
        );
        fs::remove_file(&fifo_path).unwrap();

        // 4. Symlinked db, -wal, -shm, and -journal are all refused.
        let outside = scratch.0.join("outside.sqlite3");
        fs::write(&outside, &staged_bytes).unwrap();
        symlink(&outside, vol_data.join("linked.sqlite3")).unwrap();
        assert_eq!(
            store.run_sqlite_backup(
                "44444444444444444444444444444444",
                vol_name,
                owner,
                "linked.sqlite3",
                MAX_BACKUP_BYTES,
                Duration::from_secs(10),
            ),
            Err(BackupHelperError::PathViolation)
        );
        for (idx, suffix) in ["-wal", "-shm", "-journal"].iter().enumerate() {
            let link = vol_data.join(format!("calm-life.sqlite3{suffix}"));
            let _ = fs::remove_file(&link);
            symlink(&outside, &link).unwrap();
            let op_id = format!("{:032x}", 0x50 + idx);
            assert_eq!(
                store.run_sqlite_backup(
                    &op_id,
                    vol_name,
                    owner,
                    "calm-life.sqlite3",
                    MAX_BACKUP_BYTES,
                    Duration::from_secs(10),
                ),
                Err(BackupHelperError::PathViolation),
                "symlinked {suffix} was not refused"
            );
            fs::remove_file(&link).unwrap();
        }

        // 5. Oversized DB is refused and staging leaf is removed.
        let op_big = "66666666666666666666666666666666";
        assert_eq!(
            store.run_sqlite_backup(
                op_big,
                vol_name,
                owner,
                "calm-life.sqlite3",
                4096,
                Duration::from_secs(10),
            ),
            Err(BackupHelperError::TooLarge)
        );
        assert!(!staging_dir.join(format!("{op_big}.sqlite3")).exists());
    }
}
