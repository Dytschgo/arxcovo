#![cfg(all(test, target_os = "linux"))]

#[path = "support/c13_failure.rs"]
mod c13_failure;

use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::{fs::PermissionsExt, process::CommandExt},
    },
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    thread::sleep,
    time::{Duration, Instant},
};

const OWNER: u32 = 21901;
const DAEMON: u32 = 21902;
const AGENT: u32 = 21903;
const BUILD: u32 = 21904;
const PUBLISH: &str = "cluster::consensus::bound_start::runtime::tests::publish_only_probe::c13_final_publication_only";
const WAIT: Duration = Duration::from_secs(16);
const CAMPAIGN: Duration = Duration::from_secs(102);
const CLEANUP: Duration = Duration::from_secs(17);
static END: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
static PHASE: std::sync::Mutex<&'static str> = std::sync::Mutex::new("preflight");
static FIRST: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn phase(value: &'static str) {
    *PHASE.lock().unwrap() = value;
}

fn first_failure(category: &'static str, panic: Option<&std::panic::PanicHookInfo<'_>>) {
    if !FIRST.swap(true, std::sync::atomic::Ordering::SeqCst) {
        let phase = *PHASE.lock().unwrap_or_else(|error| error.into_inner());
        if let Some(panic) = panic {
            c13_failure::report_assertion(phase, file!(), panic);
        } else {
            println!("\nC13 first phase={phase} category={category}");
        }
    }
}

fn refuse(category: &'static str) -> ! {
    first_failure(category, None);
    panic!("C13 refused");
}

fn phase_end() -> Instant {
    (Instant::now() + WAIT).min(*END.get().expect("C13 campaign deadline"))
}

struct OwnedChild {
    process: Child,
    stop_sent: bool,
}

impl OwnedChild {
    fn new(process: Child) -> Self {
        Self {
            process,
            stop_sent: false,
        }
    }

    fn wait(&mut self) -> io::Result<ExitStatus> {
        let deadline = phase_end();
        loop {
            if let Some(status) = self.process.try_wait()? {
                return Ok(status);
            }
            if Instant::now() >= deadline {
                return Err(io::ErrorKind::TimedOut.into());
            }
            sleep(Duration::from_millis(20));
        }
    }

    fn interrupt(&mut self) -> io::Result<()> {
        if !self.stop_sent && self.process.try_wait()?.is_none() {
            self.stop_sent = true;
            // SAFETY: a still-owned, unreaped subprocess keeps its PID.
            if unsafe { libc::kill(self.process.id() as i32, libc::SIGINT) } != 0 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    }
}

#[derive(Default)]
struct Helpers {
    active: Option<OwnedChild>,
    started: usize,
    reaped: usize,
}

impl Helpers {
    fn spawn(&mut self, command: &mut Command) {
        assert!(self.active.is_none() && Instant::now() < phase_end());
        self.active = Some(OwnedChild::new(
            command.spawn().unwrap_or_else(|_| refuse("spawn")),
        ));
        self.started += 1;
    }

    fn reap_ready(&mut self) {
        if self
            .active
            .as_mut()
            .is_some_and(|helper| matches!(helper.process.try_wait(), Ok(Some(_))))
        {
            self.active.take();
            self.reaped += 1;
        }
    }

    fn finish(&mut self) -> (Child, ExitStatus) {
        let status = self
            .active
            .as_mut()
            .unwrap()
            .wait()
            .unwrap_or_else(|_| refuse("helper-reap"));
        let process = self.active.take().unwrap().process;
        self.reaped += 1;
        (process, status)
    }
}

#[derive(Default)]
struct Campaign {
    processes: Vec<(Node, OwnedChild)>,
    closes: usize,
    helpers: Helpers,
    confirmed: Vec<bool>,
}

impl Campaign {
    fn start(&mut self, node: &Node, binary: &Path, intent: &str, sockets: SocketPair) -> usize {
        assert!(Instant::now() < phase_end(), "C13 campaign budget expired");
        self.processes
            .push((node.clone(), node.start(binary, intent, sockets)));
        self.confirmed.push(false);
        self.processes.len() - 1
    }

    fn ready(&mut self, index: usize, conditions: &[u8]) {
        let (node, job) = &mut self.processes[index];
        node.ready(job, conditions, &mut self.helpers);
    }

    fn close(&mut self, index: usize) {
        let (node, job) = &mut self.processes[index];
        job.interrupt().expect("C13 graceful SIGINT");
        assert!(job.stop_sent && job.wait().expect("C13 daemon reap").success());
        assert!(node.closed(), "C13 listener or lock close unconfirmed");
        self.confirmed[index] = true;
        self.closes += 1;
    }
}

impl Campaign {
    fn cleanup(&mut self, end: Instant) -> String {
        // Signal every remaining worker first. Observe them independently in one
        // common reserve. A timed-out explicit close never sends a second signal.
        let mut observed = self
            .confirmed
            .iter()
            .map(|clean| clean.then_some(true))
            .collect::<Vec<_>>();
        for (_, job) in &mut self.processes {
            let _ = job.interrupt();
        }
        if let Some(helper) = &mut self.helpers.active {
            let _ = helper.interrupt();
        }
        loop {
            for (disposition, (node, job)) in observed.iter_mut().zip(&mut self.processes) {
                if disposition.is_none()
                    && let Ok(Some(status)) = job.process.try_wait()
                {
                    *disposition = Some(status.success() && job.stop_sent && node.closed());
                }
            }
            self.helpers.reap_ready();
            if (observed.iter().all(Option::is_some) && self.helpers.active.is_none())
                || Instant::now() >= end
            {
                break;
            }
            sleep(Duration::from_millis(20));
        }
        let helper_unknown = usize::from(self.helpers.active.is_some());
        let joined = observed.iter().flatten().count();
        let clean = observed.iter().filter(|x| **x == Some(true)).count();
        format!(
            "C13 cleanup spawned={} signaled={} reaped={joined} clean={clean} helpers={} helpers_reaped={} helper_unknown={helper_unknown}",
            observed.len(),
            self.processes
                .iter()
                .filter(|(_, job)| job.stop_sent)
                .count(),
            self.helpers.started,
            self.helpers.reaped,
        )
    }
}

impl Drop for Campaign {
    fn drop(&mut self) {
        if let Some(deadline) = END.get() {
            let end = (Instant::now() + CLEANUP).min(*deadline + CLEANUP);
            println!("{}", self.cleanup(end));
        }
    }
}

#[test]
fn c13_cleanup_reports_helper_reaped_during_cleanup() {
    let child = Command::new("/bin/true").spawn().unwrap();
    let mut campaign = Campaign::default();
    campaign.helpers.active = Some(OwnedChild::new(child));
    campaign.helpers.started = 1;
    let evidence = campaign.cleanup(Instant::now() + Duration::from_secs(2));
    assert_eq!(
        evidence,
        "C13 cleanup spawned=0 signaled=0 reaped=0 clean=0 helpers=1 helpers_reaped=1 helper_unknown=0"
    );
}

struct Secret(Vec<u8>);
impl Drop for Secret {
    fn drop(&mut self) {
        for byte in &mut self.0 {
            // SAFETY: the byte is exclusively borrowed for this write.
            unsafe { std::ptr::write_volatile(byte, 0) };
        }
    }
}

fn transition(uid: u32) -> io::Result<()> {
    // SAFETY: each call takes fixed scalar values or a null pointer with length zero.
    // The owner role gets the one named group as its primary group.
    if unsafe {
        libc::setgroups(0, std::ptr::null()) == 0
            && libc::setresgid(uid, uid, uid) == 0
            && libc::setresuid(uid, uid, uid) == 0
    } {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

type SocketPair = [tokio::net::TcpSocket; 2];

fn reserve(address: std::net::SocketAddr) -> io::Result<tokio::net::TcpSocket> {
    let socket = tokio::net::TcpSocket::new_v4()?;
    // The successor is bound but does not listen while its predecessor runs.
    // The kernel requires the same binding UID for this reuse-port group.
    socket.set_reuseaddr(true)?;
    socket.set_reuseport(true)?;
    socket.bind(address)?;
    Ok(socket)
}

fn handoff_descriptor(socket: tokio::net::TcpSocket, listening: bool) -> io::Result<OwnedFd> {
    // SAFETY: both calls operate on the owned TCP descriptor. Duplicate above
    // the handoff slots so either dup2 order is safe at exec.
    let fd = unsafe {
        if listening && libc::listen(socket.as_raw_fd(), 128) != 0 {
            return Err(io::Error::last_os_error());
        }
        libc::fcntl(socket.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 6)
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful F_DUPFD_CLOEXEC returned a distinct owned descriptor.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

#[test]
fn c13_foreground_subprocess_rejects_invalid_inherited_socket_handoffs() {
    let loopback = "127.0.0.1:0".parse().unwrap();
    let bound = handoff_descriptor(reserve(loopback).unwrap(), false).unwrap();
    let second_bound = handoff_descriptor(reserve(loopback).unwrap(), false).unwrap();
    let listening = handoff_descriptor(reserve(loopback).unwrap(), true).unwrap();
    let second_listener = handoff_descriptor(reserve(loopback).unwrap(), true).unwrap();
    let datagram = std::net::UdpSocket::bind(loopback).unwrap();
    // SAFETY: duplicate this live local fd above the handoff slots.
    let datagram_fd = unsafe { libc::fcntl(datagram.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 6) };
    assert!(datagram_fd >= 6);
    // SAFETY: successful fcntl returned a distinct owned descriptor.
    let datagram = unsafe { OwnedFd::from_raw_fd(datagram_fd) };

    for (mode, intent, peer, enrollment, extra, status) in [
        (
            "--cluster-config",
            "join",
            &datagram,
            &second_bound,
            false,
            3,
        ),
        (
            "--cluster-config",
            "join",
            &listening,
            &second_bound,
            false,
            3,
        ),
        (
            "--cluster-config",
            "join",
            &bound,
            &second_listener,
            false,
            3,
        ),
        ("--cluster-config", "join", &bound, &second_bound, true, 3),
        (
            "--cluster-config",
            "recover-founder",
            &bound,
            &second_listener,
            false,
            3,
        ),
        (
            "cluster-service-worker",
            "join",
            &bound,
            &second_bound,
            false,
            2,
        ),
        // A valid pair for Join passes FD qualification and reaches config
        // refusal (Store). In both cases no installation or owner can start.
        ("--cluster-config", "join", &bound, &second_bound, false, 5),
    ] {
        let root = tempfile::tempdir().unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_arxd"));
        command
            .arg(mode)
            .arg(root.path().join("absent-installation.json"))
            .args(["--cluster-intent", intent, "--inherit-listeners"])
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let peer = peer.as_raw_fd();
        let enrollment = enrollment.as_raw_fd();
        // SAFETY: the parent holds the descriptors and pre_exec only invokes
        // async-signal-safe FD operations.
        unsafe {
            command.pre_exec(move || {
                if libc::syscall(
                    libc::SYS_close_range,
                    5u32,
                    u32::MAX,
                    libc::CLOSE_RANGE_CLOEXEC,
                ) < 0
                    || libc::dup2(peer, 3) < 0
                    || libc::dup2(enrollment, 4) < 0
                    || (extra && libc::dup2(peer, 5) < 0)
                {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        assert_eq!(
            command.status().unwrap().code(),
            Some(status),
            "{mode} {intent}"
        );
    }
    for socket in [bound, second_bound] {
        let reservation = std::net::TcpListener::from(socket);
        assert!(!is_listening(reservation.local_addr().unwrap()).unwrap());
    }
}

fn is_listening(address: std::net::SocketAddr) -> io::Result<bool> {
    let std::net::IpAddr::V4(ip) = address.ip() else {
        return Err(io::ErrorKind::InvalidInput.into());
    };
    let target = format!(
        "{:08X}:{:04X}",
        u32::from_ne_bytes(ip.octets()),
        address.port()
    );
    let wildcard = format!("00000000:{:04X}", address.port());
    const MAX: u64 = 1024 * 1024;
    let mut table = String::new();
    File::open("/proc/net/tcp")?
        .take(MAX + 1)
        .read_to_string(&mut table)?;
    if table.len() as u64 > MAX {
        return Err(io::ErrorKind::InvalidData.into());
    }
    Ok(table.lines().skip(1).any(|line| {
        let mut columns = line.split_ascii_whitespace().skip(1);
        matches!(columns.next(), Some(address) if address == target || address == wildcard)
            && columns.nth(1) == Some("0A")
    }))
}

#[test]
fn c13_held_successor_restarts_after_owned_listener_and_connection_close() {
    let first = reserve("127.0.0.1:0".parse().unwrap()).unwrap();
    let address = first.local_addr().unwrap();
    let next = reserve(address).unwrap();
    let fd = handoff_descriptor(first, true).unwrap();
    let listener = std::net::TcpListener::from(fd);
    let mut client = std::net::TcpStream::connect(address).unwrap();
    client.set_read_timeout(Some(WAIT)).unwrap();
    let (server, _) = listener.accept().unwrap();
    drop(server);
    assert_eq!(client.read(&mut [0]).unwrap(), 0);
    drop(client);
    assert!(is_listening(address).unwrap());
    drop(listener);
    assert!(!is_listening(address).unwrap());
    let _owned = handoff_descriptor(next, true).unwrap();
    assert!(is_listening(address).unwrap());
}

#[derive(Clone)]
struct Node {
    fixture: &'static str,
    root: PathBuf,
    peer: std::net::SocketAddr,
    enrollment: std::net::SocketAddr,
}

impl Node {
    fn socket(&self) -> PathBuf {
        self.root.join("control/owner.sock")
    }

    fn config(&self) -> PathBuf {
        self.root.join("install.json")
    }

    fn record(&self) -> Vec<u8> {
        fs::read(self.root.join("credentials/identity.json")).expect("published identity")
    }

    fn configure(&self, issuer: Option<std::net::SocketAddr>) {
        let mut config = OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(self.config())
            .unwrap();
        serde_json::to_writer(
            &mut config,
            &serde_json::json!({
                "workspace": self.root.join("workspace"),
                "state": self.root.join("state"),
                "credentials": self.root.join("credentials"),
                "wrap": self.root.join("wrap"),
                "custody": self.root.join("private/custody"),
                "history": self.root.join("history"),
                "owner_socket": self.socket(),
                "peer": self.peer.to_string(),
                "enrollment": self.enrollment.to_string(),
                "issuer_peer": issuer.map(|ip| ip.to_string()),
                "founder_cluster": issuer.is_none().then_some([13u8; 16]),
                "owner_uid": OWNER, "daemon_uid": DAEMON, "daemon_gid": DAEMON,
                "agent_uid": AGENT, "build_uid": BUILD, "owner_gid": OWNER,
            }),
        )
        .unwrap();
        config.sync_all().unwrap();
    }

    fn start(&self, binary: &Path, intent: &str, sockets: SocketPair) -> OwnedChild {
        let [peer, enrollment] = sockets.map(|socket| {
            handoff_descriptor(socket, intent != "join").unwrap_or_else(|_| refuse("listener"))
        });
        let mut command = Command::new(binary);
        command
            .args(["--cluster-config"])
            .arg(self.config())
            .args(["--cluster-intent", intent, "--inherit-listeners"])
            .env_clear()
            .env("ARX_C13_DIAGNOSTICS", self.fixture)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        // SAFETY: transition uses only async-signal-safe system calls.
        unsafe {
            command.pre_exec(move || {
                if libc::dup2(peer.as_raw_fd(), 3) < 0
                    || libc::dup2(enrollment.as_raw_fd(), 4) < 0
                    || libc::syscall(
                        libc::SYS_close_range,
                        5u32,
                        u32::MAX,
                        libc::CLOSE_RANGE_CLOEXEC,
                    ) < 0
                {
                    return Err(io::Error::last_os_error());
                }
                transition(DAEMON)
            });
        }
        OwnedChild::new(command.spawn().expect("C13 daemon spawn"))
    }

    fn observe(&self, job: &OwnedChild, helpers: &mut Helpers) -> Option<[u8; 29]> {
        const QUERY: &str = r#"
import socket, struct, sys
try:
    with socket.socket(socket.AF_UNIX) as s:
        s.settimeout(.5)
        s.connect(sys.argv[1])
        pid, uid, _ = struct.unpack('3i', s.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12))
        if pid != int(sys.argv[2]) or uid != 21902:
            sys.exit(2)
        s.sendall(b'ARXO\x04\x00\x00')
        s.shutdown(socket.SHUT_WR)
        out = s.makefile('rb').read(37)
        if len(out) != 36 or out[:7] != b'ARXO\x0f\x00\x1d':
            sys.exit(2)
        sys.stdout.buffer.write(out[7:])
except OSError:
    sys.exit(2)
"#;
        let mut command = Command::new("/usr/bin/python3");
        command
            .args(["-I", "-B", "-c", QUERY])
            .arg(self.socket())
            .arg(job.process.id().to_string())
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        // SAFETY: only the fixed UID/GID transition runs before exec.
        unsafe {
            command.pre_exec(|| transition(OWNER));
        }
        helpers.spawn(&mut command);
        let (mut process, status) = helpers.finish();
        if !status.success() {
            return None;
        }
        let mut out = Vec::with_capacity(30);
        process.stdout.take()?.take(30).read_to_end(&mut out).ok()?;
        out.try_into().ok()
    }

    fn status(&self, job: &OwnedChild, accepted: &[u8], helpers: &mut Helpers) -> bool {
        self.observe(job, helpers).is_some_and(|[phase, ..]| {
            if phase == 6 {
                refuse("daemon-refused");
            }
            accepted.contains(&phase)
        })
    }

    fn ready(&self, job: &mut OwnedChild, accepted: &[u8], helpers: &mut Helpers) {
        let end = phase_end();
        while !self.status(job, accepted, helpers) {
            assert!(
                job.process.try_wait().expect("C13 child status").is_none(),
                "C13 process failed before requested readiness"
            );
            assert!(Instant::now() < end, "C13 authenticated readiness elapsed");
            sleep(Duration::from_millis(40));
        }
    }

    fn closed(&self) -> bool {
        !self.socket().exists()
            && [
                "state/cluster-owner.lock",
                "state/cluster-state.lock",
                "state/cluster-log.lock",
                "private/custody/custody-lock",
            ]
            .into_iter()
            .all(|file| {
                OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(self.root.join(file))
                    .is_ok_and(|lock| lock.try_lock().is_ok())
            })
            && [self.peer, self.enrollment]
                .into_iter()
                .all(|address| is_listening(address).is_ok_and(|active| !active))
    }
}

fn pty() -> (File, File) {
    let mut master = -1;
    let mut slave = -1;
    // SAFETY: both descriptors are valid out pointers and null optional values are supported.
    assert_eq!(
        unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
            )
        },
        0
    );
    // SAFETY: successful openpty returned two newly owned descriptors.
    let (master, slave) = unsafe { (File::from_raw_fd(master), File::from_raw_fd(slave)) };
    // SAFETY: fcntl sets descriptor flags on the live PTY master.
    assert_eq!(
        unsafe { libc::fcntl(master.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) },
        0
    );
    (master, slave)
}

fn nonblocking(fd: std::os::fd::RawFd) -> io::Result<()> {
    // SAFETY: fcntl queries and updates scalar flags on the borrowed file.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn wait_io(fd: std::os::fd::RawFd, events: libc::c_short, deadline: Instant) -> io::Result<()> {
    let mut poll = libc::pollfd {
        fd,
        events,
        revents: 0,
    };
    loop {
        let now = Instant::now();
        if now >= deadline {
            return Err(io::ErrorKind::TimedOut.into());
        }
        let millis = (deadline - now).as_millis().clamp(1, 16_000) as i32;
        // SAFETY: poll points at one initialized pollfd for the bounded duration.
        if unsafe { libc::poll(&mut poll, 1, millis) } > 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted && Instant::now() < deadline {
            return Err(error);
        }
    }
}

fn write_bounded(
    writer: &mut (impl AsRawFd + Write),
    mut bytes: &[u8],
    deadline: Instant,
) -> io::Result<()> {
    while !bytes.is_empty() {
        if Instant::now() >= deadline {
            return Err(io::ErrorKind::TimedOut.into());
        }
        match writer.write(bytes) {
            Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
            Ok(n) => bytes = &bytes[n..],
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                wait_io(writer.as_raw_fd(), libc::POLLOUT, deadline)?;
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn owner_command(
    binary: &Path,
    node: &Node,
    uid: u32,
    input: Option<&Secret>,
    helpers: &mut Helpers,
) -> (ExitStatus, Secret) {
    let (mut master, slave) = pty();
    nonblocking(master.as_raw_fd()).unwrap_or_else(|_| refuse("terminal-io"));
    let fd = slave.as_raw_fd();
    let mut command = Command::new(binary);
    command
        .args([
            "service",
            "cluster-control",
            if input.is_some() { "join" } else { "mint" },
            "--socket",
        ])
        .arg(node.socket())
        .args(["--daemon-uid", "21902"])
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // SAFETY: FD is held by the parent through exec. The child uses only fixed syscalls.
    unsafe {
        command.pre_exec(move || {
            if libc::setsid() == -1 || libc::ioctl(fd, libc::TIOCSCTTY, 0) == -1 {
                return Err(io::Error::last_os_error());
            }
            libc::close(fd);
            transition(uid)
        });
    }
    helpers.spawn(&mut command);
    let deadline = phase_end();
    if let Some(paste) = input {
        loop {
            let mut term = std::mem::MaybeUninit::<libc::termios>::uninit();
            // SAFETY: the master is owned and tcgetattr writes term on success.
            let set = unsafe {
                libc::tcgetattr(master.as_raw_fd(), term.as_mut_ptr()) == 0
                    && term.assume_init().c_lflag & libc::ECHO == 0
            };
            if set {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "C13 terminal never protected input"
            );
            sleep(Duration::from_millis(10));
        }
        write_bounded(&mut master, &paste.0, deadline)
            .and_then(|()| write_bounded(&mut master, b"\r", deadline))
            .unwrap_or_else(|_| refuse("terminal-io"));
    }
    let mut output = if input.is_some() {
        Secret(Vec::new())
    } else {
        mint_output(&mut master, slave, helpers, deadline).unwrap_or_else(|_| refuse("terminal-io"))
    };
    let (_, status) = helpers.finish();
    if uid == OWNER && !status.success() {
        let action = if input.is_some() { "join" } else { "mint" };
        let exit = status
            .code()
            .map_or_else(|| "signal".to_owned(), |code| code.to_string());
        println!(
            "\nC13 owner node={} action={action} exit={exit}",
            node.fixture
        );
    }
    if input.is_none() && status.success() {
        assert!(validate_mint_output(&mut output));
    }
    (status, output)
}

fn validate_mint_output(output: &mut Secret) -> bool {
    let Some(frame) = output.0.strip_suffix(b"\n") else {
        return false;
    };
    let frame = frame.strip_suffix(b"\r").unwrap_or(frame);
    let valid = frame.strip_prefix(b"arxje1-").is_some_and(|payload| {
        !payload.is_empty()
            && payload.len() % 2 == 0
            && payload
                .iter()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
    });
    output.0.truncate(frame.len());
    valid && output.0.len() <= 8_800
}

fn mint_output(
    master: &mut File,
    slave: File,
    helpers: &mut Helpers,
    deadline: Instant,
) -> io::Result<Secret> {
    // Keep the slave open while the helper execs and opens its controlling tty.
    let _slave = slave;
    let mut output = Secret(Vec::new());
    let mut chunk = [0; 1024];
    loop {
        if Instant::now() >= deadline {
            return Err(io::ErrorKind::TimedOut.into());
        }
        let exited = helpers
            .active
            .as_mut()
            .unwrap()
            .process
            .try_wait()?
            .is_some();
        match master.read(&mut chunk) {
            Ok(n) if n > 0 && output.0.len() + n <= 8_802 => {
                output.0.extend_from_slice(&chunk[..n]);
                continue;
            }
            Ok(0) => {}
            Ok(_) => return Err(io::ErrorKind::InvalidData.into()),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error)
                if error.kind() == io::ErrorKind::WouldBlock
                    || error.raw_os_error() == Some(libc::EIO) => {}
            Err(error) => return Err(error),
        }
        if exited {
            return Ok(output);
        }
        sleep(Duration::from_millis(5));
    }
}

#[test]
fn c13_mint_capture_waits_for_delayed_controlling_tty_open() {
    let (mut master, slave) = pty();
    nonblocking(master.as_raw_fd()).unwrap();
    let fd = slave.as_raw_fd();
    let mut command = Command::new("/bin/sh");
    command.args([
        "-c",
        "test ! -e /proc/self/fd/$1 || exit 42; sleep 0.05; printf 'arxje1-abcd\\n' > /dev/tty",
        "probe",
        &master.as_raw_fd().to_string(),
    ]);
    // SAFETY: the slave is held through exec; the child uses fixed syscalls.
    unsafe {
        command.pre_exec(move || {
            if libc::setsid() == -1 || libc::ioctl(fd, libc::TIOCSCTTY, 0) == -1 {
                return Err(io::Error::last_os_error());
            }
            libc::close(fd);
            Ok(())
        });
    }
    let mut helpers = Helpers {
        active: Some(OwnedChild::new(command.spawn().unwrap())),
        started: 1,
        ..Helpers::default()
    };
    let mut output = mint_output(&mut master, slave, &mut helpers, Instant::now() + WAIT).unwrap();
    assert!(
        helpers
            .active
            .as_mut()
            .unwrap()
            .process
            .try_wait()
            .unwrap()
            .unwrap()
            .success()
    );
    assert!(validate_mint_output(&mut output));
    assert!(output.0 == b"arxje1-abcd");
    assert!(!validate_mint_output(&mut Secret(b"arxje1-abcd".to_vec())));
}

fn publish_final(probe: &Path, node: &Node, paste: &Secret, helpers: &mut Helpers) {
    let (read, mut write) = io::pipe().expect("one-use publication pipe");
    let mut command = Command::new(probe);
    command
        .args(["--exact", PUBLISH, "--ignored", "--nocapture"])
        .env_clear()
        .env("ARX_C13_PUBLISH", "1")
        .env("ARX_C13_CONFIG", node.config())
        .env("ARX_C13_DIAGNOSTICS", node.fixture)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    // SAFETY: read is the owned private pipe. The child closes unrelated handles.
    unsafe {
        command.pre_exec(move || {
            if libc::dup2(read.as_raw_fd(), 3) == -1
                || libc::fcntl(3, libc::F_SETFD, 0) == -1
                || libc::syscall(
                    libc::SYS_close_range,
                    4u32,
                    u32::MAX,
                    libc::CLOSE_RANGE_CLOEXEC,
                ) == -1
            {
                return Err(io::Error::last_os_error());
            }
            transition(DAEMON)
        });
    }
    helpers.spawn(&mut command);
    drop(command);
    nonblocking(write.as_raw_fd()).unwrap_or_else(|_| refuse("pipe-io"));
    write_bounded(&mut write, &paste.0, phase_end()).unwrap_or_else(|_| refuse("pipe-io"));
    drop(write);
    assert!(
        helpers.finish().1.success(),
        "C13 publication helper failed"
    );
    assert!(!node.socket().exists() && !node.root.join("state/cluster-state.sqlite3").exists());
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Position {
    term: u64,
    leader: u64,
    index: u64,
}

impl Position {
    fn decode(bytes: &[u8]) -> Self {
        let word = |i| u64::from_be_bytes(bytes[i..i + 8].try_into().unwrap());
        Self {
            term: word(0),
            leader: word(8),
            index: word(16),
        }
    }

    fn covers(self, previous: Self) -> bool {
        self.index > previous.index || self == previous
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Membership {
    position: Position,
    voters: Vec<Vec<u64>>,
    nodes: Vec<(u64, String)>,
}

fn membership(position: Position, bytes: &[u8]) -> Membership {
    fn take<'a>(bytes: &mut &'a [u8], count: usize) -> &'a [u8] {
        let (head, tail) = bytes.split_at(count);
        *bytes = tail;
        head
    }
    fn word(bytes: &mut &[u8]) -> u64 {
        u64::from_be_bytes(take(bytes, 8).try_into().unwrap())
    }
    let mut bytes = bytes;
    assert_eq!(take(&mut bytes, 1), [1]);
    let voters = (0..take(&mut bytes, 1)[0])
        .map(|_| {
            (0..take(&mut bytes, 1)[0])
                .map(|_| word(&mut bytes))
                .collect()
        })
        .collect();
    let nodes = (0..take(&mut bytes, 1)[0])
        .map(|_| {
            let id = word(&mut bytes);
            let size = usize::from(take(&mut bytes, 1)[0]);
            let address = std::str::from_utf8(take(&mut bytes, size)).unwrap();
            (id, address.to_owned())
        })
        .collect();
    assert!(bytes.is_empty());
    Membership {
        position,
        voters,
        nodes,
    }
}

struct Inventory {
    counts: [usize; 7],
    membership: Vec<Membership>,
    redeems: Vec<(Position, [u8; 32])>,
}

fn checked_blob<'a>(bytes: &'a [u8], header: &[u8], max: usize) -> &'a [u8] {
    use sha2::{Digest, Sha256};
    assert!((header.len() + 32..=max).contains(&bytes.len()) && bytes.starts_with(header));
    let (body, hash) = bytes.split_at(bytes.len() - 32);
    assert!(Sha256::digest(body).as_slice() == hash);
    body
}

fn open_database(path: PathBuf) -> rusqlite::Connection {
    let db =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .expect("C13 observed store");
    db.busy_timeout(Duration::from_millis(200)).unwrap();
    db
}

fn inventory(node: &Node) -> Inventory {
    let db = open_database(node.root.join("state/cluster-log.sqlite3"));
    let mut query = db
        .prepare("SELECT idx, body FROM log ORDER BY idx LIMIT 65")
        .unwrap();
    let mut rows = query.query([]).unwrap();
    let mut seen = Inventory {
        counts: [0; 7],
        membership: vec![],
        redeems: vec![],
    };
    while let Some(row) = rows.next().unwrap() {
        let idx: u64 = row.get(0).unwrap();
        let blob: Vec<u8> = row.get(1).unwrap();
        let body = checked_blob(&blob, b"ARXLE2\0\0", 4163);
        let pos = Position::decode(&body[8..32]);
        assert_eq!(pos.index, idx);
        let tag = usize::from(body[32]);
        seen.counts[tag] += 1;
        if tag == 3 {
            assert_eq!(
                usize::from(u16::from_be_bytes(body[33..35].try_into().unwrap())),
                body.len() - 35
            );
            seen.membership.push(membership(pos, &body[35..]));
        } else if tag == 6 {
            seen.redeems.push((pos, body[113..145].try_into().unwrap()));
        }
    }
    assert!(seen.counts.iter().sum::<usize>() < 65);
    seen
}

fn applied(node: &Node) -> (Position, Membership) {
    let db = open_database(node.root.join("state/cluster-state.sqlite3"));
    let blob: Vec<u8> = db
        .query_row("SELECT snapshot FROM sm WHERE singleton=1", [], |row| {
            row.get(0)
        })
        .unwrap();
    let body = checked_blob(&blob, b"ARXSM1\0\0\0\x03\x01", 16384);
    let applied = Position::decode(&body[11..35]);
    assert_eq!(body[35], 1);
    let pos = Position::decode(&body[36..60]);
    let size = usize::from(u16::from_be_bytes(body[60..62].try_into().unwrap()));
    assert!(applied.covers(pos));
    (applied, membership(pos, &body[62..62 + size]))
}

fn assert_membership(node: &Node, expected: &[Membership]) {
    assert_eq!(inventory(node).membership, expected);
    assert_eq!(&applied(node).1, expected.last().unwrap());
}

fn publication(node: &Node, id: u64, issuer: u64) -> (Position, [u8; 32]) {
    let record: serde_json::Value = serde_json::from_slice(&node.record()).unwrap();
    assert_eq!(record["node"], id);
    let cluster = [13u8; 16];
    assert_eq!(record["cluster"], serde_json::json!(cluster));
    let origin = &record["origin"];
    assert_eq!(origin["role"], "joiner");
    assert_eq!(origin["leader"], issuer);
    let key = serde_json::from_value(record["key"].clone()).unwrap();
    (
        Position {
            term: origin["term"].as_u64().unwrap(),
            leader: issuer,
            index: origin["index"].as_u64().unwrap(),
        },
        key,
    )
}

fn role_probe(uid: u32, operation: &str, path: &Path, allowed: bool, helpers: &mut Helpers) {
    const PROBE: &str = r#"
import socket, sys
try:
    if sys.argv[1] == 'owner':
        with socket.socket(socket.AF_UNIX) as s:
            s.settimeout(.5)
            s.connect(sys.argv[2])
    else:
        with open(sys.argv[2], 'rb') as f:
            if not f.read(1):
                sys.exit(3)
except PermissionError:
    sys.exit(2)
except OSError:
    sys.exit(3)
"#;
    let mut command = Command::new("/usr/bin/python3");
    command
        .args(["-I", "-B", "-c", PROBE, operation])
        .arg(path)
        .env_clear();
    // SAFETY: only the fixed fixture identity transition runs before exec.
    unsafe {
        command.pre_exec(move || transition(uid));
    }
    helpers.spawn(&mut command);
    assert_eq!(helpers.finish().1.code(), Some(if allowed { 0 } else { 2 }));
}

fn without_ca(node: &Node) {
    assert!(!node.root.join("private/voter-custody").exists());
    assert!(!node.root.join("private/custody/ca-key").exists());
    assert!(!node.root.join("history/voter").exists());
}

#[test]
#[ignore = "once-only hosted C13 launcher with disposable runner identities"]
fn c13_activation_restarts_original_nodes_and_issues_after_founder_close() {
    std::panic::set_hook(Box::new(|panic| first_failure("assertion", Some(panic))));
    END.set(Instant::now() + CAMPAIGN).unwrap();
    let mut campaign = Campaign::default();
    assert_eq!(std::env::var("ARX_C13_HOSTED").as_deref(), Ok("1"));
    // SAFETY: this samples the process UID for the launcher precondition.
    assert_eq!(unsafe { libc::geteuid() }, 0);
    let base = PathBuf::from(std::env::var_os("ARX_C13_BASE").expect("staged launcher root"));
    let binary = base.join("arxd");
    let probe = base.join("arxd-publication");
    let ip: std::net::IpAddr = std::env::var("ARX_C13_IP").unwrap().parse().unwrap();
    let reserve_pair = |addresses: [std::net::SocketAddr; 2]| {
        addresses.map(|address| reserve(address).unwrap_or_else(|_| refuse("listener")))
    };
    let initial = ["a", "b", "c", "d"].map(|_| reserve_pair([(ip, 0).into(); 2]));
    let addresses = initial.each_ref().map(|sockets| {
        sockets
            .each_ref()
            .map(|socket| socket.local_addr().unwrap())
    });
    assert_eq!(
        addresses
            .iter()
            .flatten()
            .collect::<std::collections::HashSet<_>>()
            .len(),
        8
    );
    let [a_again, b_again, c_again] = [0, 1, 2].map(|index| reserve_pair(addresses[index]));
    let [a_ports, b_ports, c_ports, _d_ports] = initial;
    let nodes = ["a", "b", "c", "d"]
        .into_iter()
        .zip(addresses)
        .map(|(name, [peer, enrollment])| Node {
            fixture: name,
            root: base.join(name),
            peer,
            enrollment,
        })
        .collect::<Vec<_>>();
    let [a, b, c, d] = nodes.as_slice() else {
        unreachable!()
    };
    for (node, issuer) in [
        (a, None),
        (b, Some(a.peer)),
        (c, Some(a.peer)),
        (d, Some(a.peer)),
    ] {
        node.configure(issuer);
        assert_eq!(
            fs::metadata(node.config()).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    phase("founder");
    let founder = campaign.start(a, &binary, "create-founder", a_ports);
    campaign.ready(founder, &[4]);
    assert_eq!(inventory(a).counts[1], 0);
    role_probe(OWNER, "owner", &a.socket(), true, &mut campaign.helpers);
    for uid in [AGENT, BUILD] {
        role_probe(uid, "owner", &a.socket(), false, &mut campaign.helpers);
        let (denied, _) = owner_command(&binary, a, uid, None, &mut campaign.helpers);
        assert_eq!(denied.code(), Some(6), "distinct role reached owner Mint");
    }
    assert_eq!(inventory(a).counts[1], 0);
    phase("join");
    let mut processes = Vec::with_capacity(2);
    let mut admissions = Vec::with_capacity(2);
    for (node, ports) in [(b, b_ports), (c, c_ports)] {
        let joiner = campaign.start(node, &binary, "join", ports);
        campaign.ready(joiner, &[1]);
        assert!(!is_listening(node.peer).expect("C13 pending peer listen state"));
        assert!(!is_listening(node.enrollment).expect("C13 pending enrollment listen state"));
        assert!(!node.root.join("state/cluster-state.sqlite3").exists());
        without_ca(node);
        let (mint, paste) = owner_command(&binary, a, OWNER, None, &mut campaign.helpers);
        assert!(mint.success(), "C13 real owner Mint refused");
        let (join, _) = owner_command(&binary, node, OWNER, Some(&paste), &mut campaign.helpers);
        assert!(join.success(), "C13 real join or self-admission refused");
        campaign.ready(joiner, &[3]);
        let evidence = node
            .observe(&campaign.processes[joiner].1, &mut campaign.helpers)
            .unwrap();
        assert_eq!(
            evidence[3..5],
            [1, 1],
            "exact admission and published no-CA proof"
        );
        admissions.push((evidence[3], Position::decode(&evidence[5..])));
        processes.push(joiner);
    }
    phase("promotion-transfer");
    campaign.ready(founder, &[5]);
    let [first, second]: [usize; 2] = processes.try_into().ok().unwrap();
    let original = [a.record(), b.record(), c.record()];
    let original_root = fs::read(a.root.join("credentials/ca.der")).unwrap();
    let [_, anchors_before, redeems_before, ..] = a
        .observe(&campaign.processes[founder].1, &mut campaign.helpers)
        .unwrap();
    assert_eq!([anchors_before, redeems_before], [2, 2]);
    let transfers = [b, c]
        .into_iter()
        .map(|node| {
            for artifact in [
                "history/voter",
                "history/strict",
                "private/voter-custody/ca-key",
            ] {
                let path = node.root.join(artifact);
                role_probe(BUILD, "read", &path, false, &mut campaign.helpers);
                role_probe(DAEMON, "read", &path, true, &mut campaign.helpers);
            }
            assert!(fs::read(node.root.join("credentials/ca.der")).unwrap() == original_root);
            1
        })
        .sum::<usize>();
    let before = inventory(a);
    assert_eq!(before.counts[1..], [2, 0, 5, 1, 0, 2]);
    assert_eq!(before.redeems, [publication(b, 2, 1), publication(c, 3, 1)]);
    let configurations = [
        vec![vec![1]],
        vec![vec![1]],
        vec![vec![1]],
        vec![vec![1], vec![1, 2, 3]],
        vec![vec![1, 2, 3]],
    ];
    for (n, (actual, voters)) in before.membership.iter().zip(configurations).enumerate() {
        assert_eq!(actual.voters, voters);
        assert_eq!(
            actual.nodes,
            [a, b, c]
                .into_iter()
                .take((n + 1).min(3))
                .enumerate()
                .map(|(i, node)| ((i + 1) as u64, node.peer.to_string()))
                .collect::<Vec<_>>()
        );
        if n > 0 {
            assert!(actual.position.index > before.membership[n - 1].position.index);
        }
    }
    for (observed, append) in admissions.iter().zip(&before.membership[1..3]) {
        assert_eq!(observed.1, append.position);
    }
    for node in [a, b, c] {
        assert_membership(node, &before.membership);
    }
    phase("recovery");
    campaign.close(founder);
    let founder = campaign.start(a, &binary, "recover-founder", a_again);
    campaign.ready(founder, &[3, 4, 5]);
    assert_membership(a, &before.membership);
    campaign.close(first);
    let first = campaign.start(b, &binary, "recover-joiner", b_again);
    campaign.ready(first, &[3, 4, 5]);
    assert_membership(b, &before.membership);
    campaign.close(second);
    let second = campaign.start(c, &binary, "recover-joiner", c_again);
    campaign.ready(second, &[3, 4, 5]);
    assert_membership(c, &before.membership);
    assert!(original == [a.record(), b.record(), c.record()]);
    campaign.close(founder);
    phase("survivor");
    let end = phase_end();
    let survivor = loop {
        if b.status(&campaign.processes[first].1, &[4, 5], &mut campaign.helpers) {
            break b;
        }
        if c.status(
            &campaign.processes[second].1,
            &[4, 5],
            &mut campaign.helpers,
        ) {
            break c;
        }
        assert!([first, second].into_iter().all(|index| {
            campaign.processes[index]
                .1
                .process
                .try_wait()
                .unwrap()
                .is_none()
        }));
        assert!(
            Instant::now() < end,
            "C13 surviving leader did not establish"
        );
        sleep(Duration::from_millis(40));
    };
    let (mint, paste) = owner_command(&binary, survivor, OWNER, None, &mut campaign.helpers);
    assert!(mint.success(), "C13 survivor Mint refused");
    phase("publication");
    publish_final(&probe, d, &paste, &mut campaign.helpers);
    assert!(fs::read(d.root.join("credentials/ca.der")).unwrap() == original_root);
    let issuer_id = if survivor.root == b.root { 2 } else { 3 };
    let final_position = publication(d, 4, issuer_id);
    let survivor_job = &campaign.processes[if issuer_id == 2 { first } else { second }].1;
    let [_, anchors_after, redeems_after, ..] = survivor
        .observe(survivor_job, &mut campaign.helpers)
        .unwrap();
    assert_eq!([anchors_after, redeems_after], [1, 1]);
    phase("close");
    campaign.close(first);
    campaign.close(second);
    let after = inventory(survivor);
    assert_eq!(
        after.membership, before.membership,
        "membership writes after restart"
    );
    assert_eq!(after.counts[1..], [3, 0, 5, 1, 0, 3]);
    assert_eq!(after.redeems[..2], before.redeems);
    assert_eq!(after.redeems[2], final_position);
    assert!(applied(survivor).0.covers(final_position.0));
    let additions = after
        .membership
        .windows(2)
        .filter(|w| w[1].nodes.len() > w[0].nodes.len())
        .count();
    let joints = after
        .membership
        .iter()
        .filter(|m| m.voters.len() == 2)
        .count();
    let flattens = after
        .membership
        .windows(2)
        .filter(|w| w[0].voters.len() == 2 && w[1].voters.len() == 1)
        .count();
    let [_, mints, _, _, found, removes, redeems] = after.counts;
    assert_eq!(
        (campaign.processes.len(), campaign.closes, transfers),
        (6, 6, 2)
    );
    println!(
        "C13 starts={} closes={} Found={found} Mint={mints} AnchorQuery={} Redeem={} admissions={} AddNodes={additions} joint={joints} flatten={flattens} transfer={transfers} enrollment_proposals={}",
        campaign.processes.len(),
        campaign.closes,
        anchors_before + anchors_after,
        redeems_before + redeems_after,
        admissions
            .iter()
            .map(|(count, _)| usize::from(*count))
            .sum::<usize>(),
        mints + found + removes + redeems,
    );
}
