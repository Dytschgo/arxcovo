//! Local-only Standard executor transport. The daemon owns admission and audit.
use super::{
    config::{Rights, ServiceConfig},
    executor::{Creation, Evidence, Execution, Executor, Observation, Report, Request},
    layout::Layout,
    native::NativeResolver,
    preflight::ExecutionUnavailable as Unavailable,
};
use crate::{
    development::{Cancellation, CapturedOutput},
    store::{ObservationPoint, StopReason, TreeBoundary},
};
use serde::{Deserialize, Serialize};
use std::{
    ffi::{CString, OsString},
    fs::{self, File, Metadata, OpenOptions, Permissions},
    io::{self, Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::{
            ffi::OsStrExt,
            fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt},
            net::UnixStream,
        },
    },
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream as ServerStream},
    sync::Semaphore,
    task::JoinSet,
    time::timeout,
};

const HANDSHAKE: Duration = Duration::from_secs(2);
const POLL: Duration = Duration::from_millis(50);
const MAX_FRAME: usize = 448 * 1024;
const MAX_CONNECTIONS: usize = 8;
const SOCKET: &str = "executor.sock";
const CANCEL: u8 = 1;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct Pin {
    uid: u32,
    gid: u32,
    slots: u8,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OwnerSettings {
    daemon_uid: u32,
    daemon_gid: u32,
    daemon_roots: Vec<PathBuf>,
    service: serde_json::Value,
}

struct Policy {
    daemon: (u32, u32),
    pin: Pin,
    socket: PathBuf,
    service: ServiceConfig,
    configuration: PathBuf,
    objects: Vec<(PathBuf, Fingerprint)>,
    content: Vec<u8>,
}

#[derive(Eq, PartialEq)]
struct Fingerprint(u64, u64, u32, u32, u32);

impl From<Metadata> for Fingerprint {
    fn from(m: Metadata) -> Self {
        Self(m.dev(), m.ino(), m.uid(), m.gid(), m.mode())
    }
}

fn bad() -> io::Error {
    io::ErrorKind::InvalidData.into()
}

fn canonical_path(path: &Path) -> bool {
    let bytes = path.as_os_str().as_bytes();
    bytes.len() <= 4096
        && !bytes.contains(&0)
        && path.is_absolute()
        && bytes[1..]
            .split(|b| *b == b'/')
            .all(|p| !p.is_empty() && p != b"." && p != b"..")
}

fn protected_directory(path: &Path) -> io::Result<Metadata> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.uid() != 0 || metadata.mode() & 0o022 != 0 {
        return Err(bad());
    }
    Ok(metadata)
}

fn ancestry(path: &Path) -> io::Result<()> {
    if path != Path::new("/") && !canonical_path(path) {
        return Err(bad());
    }
    // Check root first. Every passed directory is beyond an untrusted writer's
    // authority to rename, so pathname operations cannot follow its substitutions.
    let parts = path.ancestors().collect::<Vec<_>>();
    for parent in parts.into_iter().rev() {
        protected_directory(parent)?;
    }
    Ok(())
}

fn owner_file(path: &Path, private: bool) -> io::Result<File> {
    if !canonical_path(path) {
        return Err(bad());
    }
    ancestry(path.parent().ok_or_else(bad)?)?;
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    let metadata = file.metadata()?;
    let forbidden = if private { 0o077 } else { 0o022 };
    if !metadata.is_file() || metadata.uid() != 0 || metadata.mode() & forbidden != 0 {
        return Err(bad());
    }
    Ok(file)
}

fn configuration(path: &Path) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    owner_file(path, true)?
        .take(16385)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 16384 {
        return Err(bad());
    }
    Ok(bytes)
}

fn overlap(a: &Path, b: &Path) -> bool {
    a.starts_with(b) || b.starts_with(a)
}

impl Policy {
    fn open(configuration_path: &Path, binary: &Path) -> io::Result<Self> {
        let content = configuration(configuration_path)?;
        let owner: OwnerSettings = serde_json::from_slice(&content).map_err(|_| bad())?;
        let layout = Layout::system().map_err(|_| bad())?;
        let service = ServiceConfig::parse_inventory(
            &serde_json::to_vec(&owner.service).map_err(|_| bad())?,
            &layout,
            &NativeResolver,
        )
        .map_err(|_| bad())?;
        let pin = Pin {
            uid: service.build_identity().parse().map_err(|_| bad())?,
            gid: service.build_gid().ok_or_else(bad)?,
            slots: service.limits().0,
        };
        if service.rights() != Rights::Standard
            || service.cluster().is_some()
            || [owner.daemon_uid, owner.daemon_gid, pin.uid, pin.gid]
                .iter()
                .any(|id| *id == 0 || *id == u32::MAX)
            || owner.daemon_uid == pin.uid
            || owner.daemon_gid == pin.gid
            || service.client_identity() != owner.daemon_uid.to_string()
            || !(1..=8).contains(&owner.daemon_roots.len())
        {
            return Err(bad());
        }

        let private = configuration_path.parent().ok_or_else(bad)?;
        let private_meta = protected_directory(private)?;
        if private_meta.mode() & 0o7777 != 0o700 {
            return Err(bad());
        }
        let base = private.parent().ok_or_else(bad)?;
        let control = base.join("control");
        let control_meta = protected_directory(&control)?;
        if control_meta.mode() & 0o7777 != 0o710 || control_meta.gid() != owner.daemon_gid {
            return Err(bad());
        }
        let workspace = Path::new(service.workspace());
        if !canonical_path(workspace)
            || overlap(workspace, private)
            || overlap(workspace, &control)
            || binary.starts_with(private)
        {
            return Err(bad());
        }
        let binary_meta = owner_file(binary, false)?.metadata()?;
        if binary_meta.mode() & 0o111 != 0o111 {
            return Err(bad());
        }
        let mut objects = vec![
            (private.to_owned(), Fingerprint::from(private_meta)),
            (control.clone(), Fingerprint::from(control_meta)),
            (binary.to_owned(), Fingerprint::from(binary_meta)),
        ];
        for dir in owner.daemon_roots {
            if !canonical_path(&dir)
                || overlap(&dir, workspace)
                || objects.iter().any(|(path, _)| overlap(path, &dir))
            {
                return Err(bad());
            }
            ancestry(dir.parent().ok_or_else(bad)?)?;
            let metadata = fs::symlink_metadata(&dir)?;
            if !metadata.is_dir()
                || metadata.uid() != owner.daemon_uid
                || metadata.gid() != owner.daemon_gid
                || metadata.mode() & 0o7777 != 0o700
            {
                return Err(bad());
            }
            objects.push((dir, metadata.into()));
        }
        Ok(Self {
            daemon: (owner.daemon_uid, owner.daemon_gid),
            pin,
            socket: control.join(SOCKET),
            service,
            configuration: configuration_path.to_owned(),
            objects,
            content,
        })
    }

    fn check(&self) -> Result<(), Unavailable> {
        let stable = self.objects.iter().all(|(path, expected)| {
            path.parent().is_some_and(|p| ancestry(p).is_ok())
                && fs::symlink_metadata(path).is_ok_and(|m| Fingerprint::from(m) == *expected)
        });
        if !stable || !configuration(&self.configuration).is_ok_and(|c| c == self.content) {
            return Err(Unavailable::PreflightFailed);
        }
        self.service.execution_preflight(
            &Layout::system().map_err(|_| Unavailable::PreflightFailed)?,
            &NativeResolver,
        )
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
enum Operation {
    Status,
    Drain,
    Execute(Request),
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
enum Reply {
    Ready(Pin),
    Drained,
    Unavailable(String),
    Cancelled,
    Failed(Creation),
    Completed {
        uid: u32,
        gid: u32,
        stop: StopReason,
        exit_code: Option<i32>,
        cleanup: bool,
        stdout: Output,
        stderr: Output,
    },
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Output(Vec<u8>, Vec<u8>, u64);

impl From<CapturedOutput> for Output {
    fn from(output: CapturedOutput) -> Self {
        Self(output.head, output.tail, output.dropped_bytes)
    }
}

impl From<Output> for CapturedOutput {
    fn from(output: Output) -> Self {
        Self {
            head: output.0,
            tail: output.1,
            dropped_bytes: output.2,
        }
    }
}

impl Reply {
    fn from_report(report: Report) -> Self {
        match report {
            Report::Unavailable(reason) => Self::Unavailable(reason.as_str().into()),
            Report::CancelledBeforeSpawn => Self::Cancelled,
            Report::Failed { creation } => Self::Failed(creation),
            Report::Completed {
                evidence:
                    Evidence {
                        observation: Some(Observation::Unix { uid, gid }),
                        point: Some(ObservationPoint::PreExec),
                        stop: Some(stop),
                        exit_code,
                        tree_boundary: Some(TreeBoundary::LinuxCgroup),
                        cleanup_confirmed: Some(cleanup),
                    },
                stdout,
                stderr,
            } => Self::Completed {
                uid,
                gid,
                stop,
                exit_code,
                cleanup,
                stdout: stdout.into(),
                stderr: stderr.into(),
            },
            _ => Self::Failed(Creation::Unknown),
        }
    }

    fn into_report(self) -> Report {
        match self {
            Self::Unavailable(reason) => unavailable(&reason).map_or(
                Report::Failed {
                    creation: Creation::Unknown,
                },
                Report::Unavailable,
            ),
            Self::Cancelled => Report::CancelledBeforeSpawn,
            Self::Failed(creation) => Report::Failed { creation },
            Self::Completed {
                uid,
                gid,
                stop,
                exit_code,
                cleanup,
                stdout,
                stderr,
            } => Report::Completed {
                evidence: Evidence {
                    observation: Some(Observation::Unix { uid, gid }),
                    point: Some(ObservationPoint::PreExec),
                    stop: Some(stop),
                    exit_code,
                    tree_boundary: Some(TreeBoundary::LinuxCgroup),
                    cleanup_confirmed: Some(cleanup),
                },
                stdout: stdout.into(),
                stderr: stderr.into(),
            },
            Self::Ready(_) | Self::Drained => Report::Failed {
                creation: Creation::Unknown,
            },
        }
    }
}

fn unavailable(reason: &str) -> Option<Unavailable> {
    use Unavailable::*;
    [
        NotConfigured,
        ServiceModeRequired,
        PlatformUnsupported,
        AccountMissing,
        AccountIdentityChanged,
        RightsMismatch,
        AdminUnsupportedOnPlatform,
        BatchRightMissing,
        MechanismUnavailable,
        ServiceReconfigurationRequired,
        PreflightFailed,
        Quarantined,
        Busy,
        Stopping,
    ]
    .into_iter()
    .find(|r| r.as_str() == reason)
}

fn frame(value: &impl Serialize) -> io::Result<Vec<u8>> {
    let body = serde_json::to_vec(value).map_err(|_| bad())?;
    if body.len() > MAX_FRAME {
        return Err(bad());
    }
    let mut record = (body.len() as u32).to_be_bytes().to_vec();
    record.extend_from_slice(&body);
    Ok(record)
}

fn frame_len(header: [u8; 4]) -> io::Result<usize> {
    let n = u32::from_be_bytes(header) as usize;
    if !(1..=MAX_FRAME).contains(&n) {
        return Err(bad());
    }
    Ok(n)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Peer {
    pid: i32,
    uid: u32,
    gid: u32,
}

fn credentials(stream: &UnixStream) -> io::Result<Peer> {
    // SAFETY: SO_PEERCRED initializes a ucred when the syscall succeeds.
    unsafe {
        let mut credentials = std::mem::zeroed::<libc::ucred>();
        let mut size = std::mem::size_of_val(&credentials) as libc::socklen_t;
        if libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut credentials as *mut libc::ucred).cast(),
            &mut size,
        ) != 0
        {
            return Err(io::Error::last_os_error());
        }
        if size as usize != std::mem::size_of_val(&credentials) || credentials.pid <= 0 {
            return Err(bad());
        }
        Ok(Peer {
            pid: credentials.pid,
            uid: credentials.uid,
            gid: credentials.gid,
        })
    }
}

struct Client {
    socket: PathBuf,
    pin: Pin,
    peer: Peer,
    cleanup_unknown: AtomicBool,
}

fn open_socket(path: &Path) -> io::Result<UnixStream> {
    let mut address = {
        // SAFETY: the zeroed sockaddr has a valid representation.
        unsafe { std::mem::zeroed::<libc::sockaddr_un>() }
    };
    let name = path.as_os_str().as_bytes();
    if name.is_empty() || name.len() >= address.sun_path.len() || name.contains(&0) {
        return Err(bad());
    }
    address.sun_family = libc::AF_UNIX as libc::sa_family_t;
    for (slot, byte) in address.sun_path.iter_mut().zip(name) {
        *slot = *byte as libc::c_char;
    }
    let length = std::mem::offset_of!(libc::sockaddr_un, sun_path) + name.len() + 1;
    // SAFETY: connect uses the initialized, length-delimited sockaddr above.
    let fd = unsafe {
        let raw = libc::socket(
            libc::AF_UNIX,
            libc::SOCK_STREAM | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
            0,
        );
        if raw < 0 {
            return Err(io::Error::last_os_error());
        }
        let fd = OwnedFd::from_raw_fd(raw);
        // A full AF_UNIX backlog returns EAGAIN on Linux. Refuse without
        // queuing a blocking connect behind an unresponsive broker.
        if libc::connect(
            raw,
            (&address as *const libc::sockaddr_un).cast(),
            length as libc::socklen_t,
        ) != 0
        {
            return Err(io::Error::last_os_error());
        }
        fd
    };
    let stream = UnixStream::from(fd);
    stream.set_nonblocking(false)?;
    Ok(stream)
}

impl Client {
    fn transact(
        &self,
        operation: Operation,
        budget: Duration,
        stopping: impl Fn() -> bool,
    ) -> io::Result<Reply> {
        let reply = (|| {
            let stream = open_socket(&self.socket)?;
            if credentials(&stream)? != self.peer {
                return Err(bad());
            }
            exchange(&stream, &operation, budget, stopping)
        })();
        // Cancellation is sent on Execute's stream and needs the same
        // terminal evidence. A later successful exchange never clears this.
        let confirmed = match (&operation, &reply) {
            (Operation::Status, Ok(Reply::Ready(pin))) => *pin == self.pin,
            (Operation::Status | Operation::Execute(_), Ok(Reply::Unavailable(reason))) => {
                unavailable(reason).is_some()
            }
            (
                Operation::Execute(_),
                Ok(Reply::Completed {
                    uid,
                    gid,
                    cleanup: true,
                    ..
                }),
            ) => (*uid, *gid) == (self.pin.uid, self.pin.gid),
            (
                Operation::Execute(_),
                Ok(Reply::Cancelled | Reply::Failed(Creation::NotInvoked | Creation::ProvenNone)),
            )
            | (Operation::Drain, Ok(Reply::Drained)) => true,
            _ => false,
        };
        if !confirmed {
            self.cleanup_unknown.store(true, Ordering::Release);
        }
        reply
    }
}

fn exchange(
    mut stream: &UnixStream,
    operation: &Operation,
    budget: Duration,
    stopping: impl Fn() -> bool,
) -> io::Result<Reply> {
    stream.set_read_timeout(Some(POLL))?;
    stream.set_write_timeout(Some(HANDSHAKE))?;
    let deadline = Instant::now() + budget;
    stream.write_all(&frame(operation)?)?;
    let mut cancelled = false;
    let mut receive = |target: &mut [u8]| -> io::Result<()> {
        let mut offset = 0;
        while offset < target.len() {
            if Instant::now() >= deadline {
                return Err(io::ErrorKind::TimedOut.into());
            }
            if !cancelled && stopping() {
                stream.write_all(&[CANCEL])?;
                cancelled = true;
            }
            match stream.read(&mut target[offset..]) {
                Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
                Ok(n) => offset += n,
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock
                            | io::ErrorKind::TimedOut
                            | io::ErrorKind::Interrupted
                    ) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    };
    let mut header = [0; 4];
    receive(&mut header)?;
    let mut body = vec![0; frame_len(header)?];
    receive(&mut body)?;
    serde_json::from_slice(&body).map_err(|_| bad())
}

fn status(reply: io::Result<Reply>) -> Result<Pin, Unavailable> {
    match reply {
        Ok(Reply::Ready(pin)) => Ok(pin),
        Ok(Reply::Unavailable(reason)) => {
            Err(unavailable(&reason).unwrap_or(Unavailable::MechanismUnavailable))
        }
        _ => Err(Unavailable::MechanismUnavailable),
    }
}

/// Authenticate the local broker and bind the existing single admission owner
/// to the installation's build UID before the daemon exposes any route.
pub(super) fn connect(socket: PathBuf, build_uid: u32) -> Result<Arc<Execution>, Unavailable> {
    if !canonical_path(&socket) || socket.file_name().is_none_or(|n| n != SOCKET) {
        return Err(Unavailable::PreflightFailed);
    }
    let stream = open_socket(&socket).map_err(|_| Unavailable::MechanismUnavailable)?;
    let peer = credentials(&stream).map_err(|_| Unavailable::MechanismUnavailable)?;
    if (peer.uid, peer.gid) != (0, 0) {
        return Err(Unavailable::PreflightFailed);
    }
    let pin = status(exchange(&stream, &Operation::Status, HANDSHAKE, || false))?;
    if pin.uid != build_uid || pin.uid == 0 || pin.gid == 0 || !(1..=4).contains(&pin.slots) {
        return Err(Unavailable::PreflightFailed);
    }
    Ok(Execution::cluster_broker(
        Arc::new(Client {
            socket,
            pin,
            peer,
            cleanup_unknown: AtomicBool::new(false),
        }),
        pin.uid,
        pin.gid,
        pin.slots,
    ))
}

impl Executor for Client {
    fn preflight(&self) -> Result<(), Unavailable> {
        if status(self.transact(Operation::Status, HANDSHAKE, || false))? != self.pin {
            return Err(Unavailable::AccountIdentityChanged);
        }
        Ok(())
    }

    fn requires_broker_drain(&self) -> bool {
        true
    }

    fn confirm_cleanup(&self) -> bool {
        let acknowledged = matches!(
            self.transact(Operation::Drain, HANDSHAKE, || false),
            Ok(Reply::Drained)
        );
        acknowledged && !self.cleanup_unknown.load(Ordering::Acquire)
    }

    fn run(
        &self,
        request: Option<&Request>,
        cancel: &Cancellation,
        shutdown: &Cancellation,
    ) -> Report {
        let Some(request) = request.filter(|r| r.validate()) else {
            return Report::Failed {
                creation: Creation::NotInvoked,
            };
        };
        if cancel.is_cancelled() || shutdown.is_cancelled() {
            return Report::CancelledBeforeSpawn;
        }
        let budget = Duration::from_secs(request.timeout_s + 6);
        self.transact(Operation::Execute(request.clone()), budget, || {
            cancel.is_cancelled() || shutdown.is_cancelled()
        })
        .map_or(
            Report::Failed {
                creation: Creation::Unknown,
            },
            Reply::into_report,
        )
    }
}

async fn serve(
    mut stream: ServerStream,
    bridge: Arc<impl Executor>,
    pin: Pin,
    shutdown: Cancellation,
    capacity: Arc<Semaphore>,
) {
    let operation = timeout(HANDSHAKE, async {
        let mut header = [0; 4];
        stream.read_exact(&mut header).await?;
        let mut body = vec![0; frame_len(header)?];
        stream.read_exact(&mut body).await?;
        serde_json::from_slice::<Operation>(&body).map_err(|_| bad())
    })
    .await;
    let Ok(Ok(operation)) = operation else { return };
    let reply = if matches!(operation, Operation::Drain) {
        // Seal admission before inspecting the live Runner inventory. The
        // worker owns its permit through cleanup, even after connection loss.
        capacity.close();
        if !shutdown.is_cancelled()
            && capacity.available_permits() == usize::from(pin.slots)
            && bridge.confirm_cleanup()
        {
            Reply::Drained
        } else {
            Reply::Failed(Creation::Unknown)
        }
    } else {
        match (bridge.preflight(), operation) {
            (Err(reason), _) => Reply::Unavailable(reason.as_str().into()),
            (_, _) if shutdown.is_cancelled() || capacity.is_closed() => {
                Reply::Unavailable(Unavailable::Stopping.as_str().into())
            }
            (Ok(()), Operation::Status) => Reply::Ready(pin),
            (Ok(()), Operation::Execute(request)) if request.validate() => {
                // The daemon owns normal admission. Mirror its exact configured
                // bound at the privilege boundary for direct socket callers.
                let Ok(slot) = capacity.try_acquire_owned() else {
                    let reply = Reply::Unavailable(Unavailable::Busy.as_str().into());
                    if let Ok(record) = frame(&reply) {
                        let _ = timeout(HANDSHAKE, stream.write_all(&record)).await;
                    }
                    return;
                };
                let cancel = Cancellation::default();
                let child_cancel = cancel.clone();
                let mut run = tokio::task::spawn_blocking(move || {
                    let _slot = slot;
                    bridge.run(Some(&request), &child_cancel, &shutdown)
                });
                let result = tokio::select! {
                    biased;
                    _ = stream.read_u8() => {
                        // Explicit cancellation, malformed extra data and observer
                        // loss all stop the run. This task still owns its reaping.
                        cancel.cancel();
                        run.await
                    }
                    result = &mut run => result,
                };
                result.map_or(Reply::Failed(Creation::Unknown), Reply::from_report)
            }
            (Ok(()), Operation::Execute(_)) => Reply::Failed(Creation::NotInvoked),
            (Ok(()), Operation::Drain) => unreachable!("handled before admission"),
        }
    };
    if let Ok(record) = frame(&reply) {
        let _ = timeout(HANDSHAKE, stream.write_all(&record)).await;
    }
}

struct Endpoint {
    path: PathBuf,
    identity: (u64, u64),
}

impl Endpoint {
    fn bind(path: &Path, group: u32) -> io::Result<(Self, UnixListener)> {
        let listener = UnixListener::bind(path)?;
        let metadata = fs::symlink_metadata(path)?;
        let endpoint = Self {
            path: path.to_owned(),
            identity: (metadata.dev(), metadata.ino()),
        };
        if !metadata.file_type().is_socket() || metadata.uid() != 0 {
            return Err(bad());
        }
        let name = CString::new(path.as_os_str().as_bytes()).map_err(|_| bad())?;
        // SAFETY: immutable C string in a protected parent, created by bind.
        if unsafe { libc::lchown(name.as_ptr(), 0, group) } != 0 {
            return Err(io::Error::last_os_error());
        }
        fs::set_permissions(path, Permissions::from_mode(0o660))?;
        Ok((endpoint, listener))
    }

    fn close(&self) -> io::Result<()> {
        let m = fs::symlink_metadata(&self.path)?;
        if m.file_type().is_socket() && (m.dev(), m.ino()) == self.identity {
            fs::remove_file(&self.path)
        } else {
            Err(bad())
        }
    }
}

impl Drop for Endpoint {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

async fn broker(policy: Arc<Policy>, binary: PathBuf) -> Result<(), &'static str> {
    let stop = super::Shutdown::default();
    tokio::spawn(
        super::unix::Signals::install()
            .map_err(|_| "signals")?
            .forward(stop.clone()),
    );
    let root = policy
        .configuration
        .parent()
        .ok_or("configuration")?
        .to_owned();
    let check_policy = policy.clone();
    let bridge = Arc::new(super::unix_bridge::broker(
        (binary, root),
        &policy.service,
        Box::new(move || check_policy.check()),
    )?);
    let (endpoint, listener) =
        Endpoint::bind(&policy.socket, policy.daemon.1).map_err(|_| "bind")?;
    println!("ARX_EXEC_SETUP ready");
    let mut work = JoinSet::new();
    let capacity = Arc::new(Semaphore::new(usize::from(policy.pin.slots)));
    let shutdown = Cancellation::default();
    let mut failed = false;
    loop {
        tokio::select! {
            biased;
            _ = stop.cancelled() => break,
            result = work.join_next(), if !work.is_empty() => {
                if result.is_some_and(|r| r.is_err()) {
                    failed = true;
                    break;
                }
            }
            stream = listener.accept(), if work.len() < MAX_CONNECTIONS => {
                let Ok((stream, _)) = stream else {
                    failed = true;
                    break;
                };
                if stream.peer_cred().is_ok_and(|cred| (cred.uid(), cred.gid()) == policy.daemon) {
                    work.spawn(serve(stream, bridge.clone(), policy.pin, shutdown.clone(), capacity.clone()));
                }
            }
        }
    }
    shutdown.cancel();
    drop(listener);
    let removed = endpoint.close().is_ok();
    while let Some(result) = work.join_next().await {
        failed |= result.is_err();
    }
    let drained = super::unix_bridge::drained(&bridge);
    if failed || !drained || !removed {
        return Err("drain");
    }
    println!("ARX_EXEC_DRAIN confirmed");
    Ok(())
}

pub(super) fn entry(args: &[OsString]) -> Option<i32> {
    if args.first().is_none_or(|s| s != "cluster-build-broker") {
        return None;
    }
    let result = (|| {
        let [_, configuration] = args else {
            return Err("arguments");
        };
        // SAFETY: identity getters take no pointers.
        if unsafe {
            (
                libc::getuid(),
                libc::geteuid(),
                libc::getgid(),
                libc::getegid(),
            )
        } != (0, 0, 0, 0)
        {
            return Err("identity");
        }
        let binary = std::env::current_exe().map_err(|_| "binary")?;
        let policy =
            Arc::new(Policy::open(Path::new(configuration), &binary).map_err(|_| "configuration")?);
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| "runtime")?
            .block_on(broker(policy, binary))
    })();
    Some(match result {
        Ok(()) => 0,
        Err(reason) => {
            eprintln!("ARX_EXEC_REFUSED {reason}");
            1
        }
    })
}

#[cfg(test)]
mod tests;
