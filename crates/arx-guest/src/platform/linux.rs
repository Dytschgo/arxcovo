use std::cell::Cell;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Arc, mpsc};
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
    let runner = CgroupRoot::qualify().ok().map(ScriptRunner::new);
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
    runner: Option<ScriptRunner>,
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
    runner: Option<ScriptRunner>,
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

type Spawn = dyn Fn(&mut Command) -> io::Result<Child> + Send + Sync;

struct ScriptRunner {
    cgroups: Box<dyn CommandCgroups>,
    spawn: Arc<Spawn>,
    max_budget: Duration,
    cleanup_budget: Duration,
    drop_privileges: bool,
}

impl ScriptRunner {
    fn new(root: CgroupRoot) -> Self {
        Self {
            drop_privileges: root.owner == 0,
            cgroups: Box::new(root),
            spawn: Arc::new(Command::spawn),
            max_budget: Duration::from_secs(arx_guest::MAX_TIMEOUT_S.into()),
            cleanup_budget: CLEANUP_BUDGET,
        }
    }

    fn run(&self, script: &str, budget: Duration) -> io::Result<ScriptOutcome> {
        let deadline = Instant::now() + budget.min(self.max_budget);
        let mut group = self.cgroups.new_command()?;
        let mut child = match script_command(script, Some(&group.procs), self.drop_privileges) {
            Ok(command) => spawn_until(command, Arc::clone(&self.spawn), deadline)?,
            Err(_) => None,
        };
        let result = match &mut child {
            Some(child) => observe(child, deadline),
            None => Ok(Observation::LaunchFailed),
        };
        let cleanup_deadline = Instant::now() + self.cleanup_budget;
        // A child whose wait reached the deadline is still unreaped. Its PID
        // pins the process group identity for killpg.
        let group_kill = match (&child, &result) {
            (Some(child), Ok(Observation::Deadline)) => kill_process_group(child),
            _ => Ok(()),
        };
        let cgroup_kill = group.kill();
        let reap = match child.as_mut() {
            Some(child) => reap(child, cleanup_deadline),
            None => Ok(()),
        };
        let cleanup = group
            .wait_empty(cleanup_deadline)
            .and_then(|()| self.cgroups.remove_command(&group));
        group_kill?;
        cgroup_kill?;
        cleanup?;
        reap?;
        Ok(result?.outcome())
    }
}

fn spawn_until(
    mut command: Command,
    spawn: Arc<Spawn>,
    deadline: Instant,
) -> io::Result<Option<Child>> {
    if Instant::now() >= deadline {
        return Err(io::ErrorKind::TimedOut.into());
    }
    let (sender, receiver) = mpsc::sync_channel(0);
    // A stalled pre-exec handshake may still own a child with unknown cgroup
    // membership. Detach the worker: the session retires and leaves its slot
    // in place for fail-closed restart and outer VM teardown.
    thread::Builder::new().spawn(move || {
        let child = spawn(&mut command).ok();
        let late = if Instant::now() >= deadline {
            drop(sender);
            child
        } else {
            sender.send(child).err().and_then(|error| error.0)
        };
        if let Some(mut child) = late {
            let _ = kill_process_group(&child);
            let _ = reap(&mut child, Instant::now() + CLEANUP_BUDGET);
        }
    })?;
    receiver
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .map_err(|_| io::ErrorKind::TimedOut.into())
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

fn script_command(
    script: &str,
    procs: Option<&File>,
    drop_privileges: bool,
) -> io::Result<Command> {
    if script.len() > MAX_SCRIPT {
        return Err(invalid());
    }
    let (reader, mut writer) = io::pipe()?;
    // SAFETY: writer owns a live pipe descriptor. F_GETPIPE_SZ takes no argument.
    let capacity = unsafe { libc::fcntl(writer.as_raw_fd(), libc::F_GETPIPE_SZ) };
    if !usize::try_from(capacity).is_ok_and(|capacity| capacity >= script.len()) {
        return Err(invalid());
    }
    // The private empty pipe has room for the entire bounded script. Prefill
    // before spawn and close the writer, so handoff cannot wait on the shell.
    writer.write_all(script.as_bytes())?;
    drop(writer);
    let mut command = Command::new("/bin/sh");
    command
        .args(["-s"])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "C")
        .current_dir("/")
        .stdin(Stdio::from(reader))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    let procs = procs.map(File::try_clone).transpose()?;
    // SAFETY: the closure owns the duplicated descriptor and writes a fixed
    // byte. Raw credential syscalls avoid libc's setxid locks and broadcasts
    // after fork, when only the calling thread survives.
    unsafe {
        command.pre_exec(move || {
            if let Some(file) = &procs
                && libc::write(file.as_raw_fd(), b"0".as_ptr().cast(), 1) != 1
            {
                return Err(io::Error::last_os_error());
            }
            let one: libc::c_ulong = 1;
            let zero: libc::c_ulong = 0;
            let gid = libc::c_ulong::from(SCRIPT_GID);
            let uid = libc::c_ulong::from(SCRIPT_UID);
            if drop_privileges
                && (libc::syscall(libc::SYS_setgroups, zero, std::ptr::null::<libc::gid_t>()) != 0
                    || libc::syscall(libc::SYS_setresgid, gid, gid, gid) != 0
                    || libc::syscall(libc::SYS_setresuid, uid, uid, uid) != 0)
            {
                return Err(io::Error::last_os_error());
            }
            if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, one, zero, zero, zero) != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    Ok(command)
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

fn observe(child: &mut Child, deadline: Instant) -> io::Result<Observation> {
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

fn reap(child: &mut Child, deadline: Instant) -> io::Result<()> {
    loop {
        if child.try_wait()?.is_some() {
            return Ok(());
        }
        pause_until(deadline)?;
    }
}

fn kill_process_group(child: &Child) -> io::Result<()> {
    // SAFETY: PID belongs to an unreaped child spawned as its own group leader.
    if unsafe { libc::killpg(child.id() as libc::pid_t, libc::SIGKILL) } == 0 {
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
    use std::os::unix::process::ExitStatusExt;
    use std::process::ChildStdout;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU32, Ordering};

    use arx_guest::{Request, Response, ScriptStatus, read_frame, write_frame};

    use super::*;

    const CHALLENGE: &str = "0123456789abcdef0123456789abcdef";
    const FIXTURE_BUDGET: Duration = Duration::from_millis(250);

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
            let mut child = script_command(script, None, false)
                .unwrap()
                .spawn()
                .unwrap();
            let observed = observe(&mut child, Instant::now() + FIXTURE_BUDGET).unwrap();
            if matches!(observed, Observation::Deadline) {
                kill_process_group(&child).unwrap();
                reap(&mut child, Instant::now() + FIXTURE_BUDGET).unwrap();
            }
            assert_eq!(observed.outcome(), expected);
        }
        let read_only = File::open("/dev/null").unwrap();
        let error = script_command("exit 0", Some(&read_only), false)
            .unwrap()
            .spawn()
            .unwrap_err();
        assert_eq!(error.raw_os_error(), Some(libc::EBADF));
    }

    #[test]
    fn short_deadline_kills_process_group_and_reaps_owned_child() {
        let mut child = script_command("sleep 2 & printf '%s\\n' \"$!\"; wait", None, false)
            .unwrap()
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut pipe = child.stdout.take().unwrap();
        let descendant = read_pid(&mut pipe);
        // SAFETY: getpgid reads kernel state and has no pointer parameters.
        assert_eq!(unsafe { libc::getpgid(descendant) }, child.id() as i32);
        assert!(matches!(
            observe(&mut child, Instant::now() + Duration::from_millis(10)).unwrap(),
            Observation::Deadline
        ));
        let pid = child.id();
        kill_process_group(&child).unwrap();
        reap(&mut child, Instant::now() + FIXTURE_BUDGET).unwrap();
        assert_eq!(
            child.try_wait().unwrap().unwrap().signal(),
            Some(libc::SIGKILL)
        );
        // SAFETY: a null status pointer discards the result. WNOHANG bounds the call.
        assert_eq!(
            unsafe { libc::waitpid(pid as i32, std::ptr::null_mut(), libc::WNOHANG) },
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
            serve_connection(&requests[..], &mut responses, Some(ScriptRunner::new(root))),
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
    fn stalled_spawn_retires_the_session_without_joining() {
        let fixture = Arc::new(FakeCgroups::new());
        let mut runner = fixture.runner();
        runner.max_budget = Duration::from_millis(150);
        let (entered, worker) = mpsc::channel();
        let (finished, completion) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        let resume = Mutex::new(resume);
        runner.spawn = Arc::new(move |_| {
            entered.send(()).unwrap();
            let _ = resume.lock().unwrap().recv_timeout(Duration::from_secs(2));
            finished.send(()).unwrap();
            Err(io::ErrorKind::WouldBlock.into())
        });
        let (result, bytes) = run_exit_seven(runner);
        worker.recv_timeout(FIXTURE_BUDGET).unwrap();
        let still_blocked = completion.try_recv() == Err(mpsc::TryRecvError::Empty);
        let _ = release.send(());
        assert!(still_blocked, "the service waited for the stalled spawn");
        completion.recv_timeout(FIXTURE_BUDGET).unwrap();
        assert_retired(result, &bytes);
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
        let mut child = script_command(
            "setsid /bin/sh -c 'printf \"%s\\n\" \"$$\"; exec sleep 2' & exit 0",
            Some(&group.procs),
            owner == 0,
        )
        .unwrap()
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
        let mut pipe = child.stdout.take().unwrap();
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
        let runner = ScriptRunner::new(root);
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

        fn runner(self: &Arc<Self>) -> ScriptRunner {
            ScriptRunner {
                cgroups: Box::new(Arc::clone(self)),
                spawn: Arc::new(Command::spawn),
                max_budget: FIXTURE_BUDGET,
                cleanup_budget: FIXTURE_BUDGET,
                drop_privileges: false,
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

    fn run_exit_seven(runner: ScriptRunner) -> (Result<(), &'static str>, Vec<u8>) {
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

    fn expect_hello(responses: &mut &[u8], script_execution: ScriptExecution) {
        assert!(matches!(
            read_frame::<Response>(responses).unwrap(),
            Some(Response::HelloAck {
                version: 1,
                challenge,
                script_execution: advertised,
            }) if challenge == CHALLENGE && advertised == script_execution
        ));
    }

    fn read_pid(pipe: &mut ChildStdout) -> libc::pid_t {
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
