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
    collections::HashMap,
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
        Arc, Mutex,
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
const BUILD_EXECUTE: u8 = 2;
const WORKSPACE_TTL: Duration = Duration::from_secs(30);
const MAX_WORKSPACE_TICKETS: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum BrokerRefusal {
    Arguments,
    Identity,
    Binary,
    SettingsRead,
    SettingsDecode,
    Layout,
    PolicyAccount,
    OwnerPolicy,
    RunnerConstruction,
    StartupRecheck,
    BuildAccount,
    Runtime,
    Signals,
    Membership,
    HierarchyWritableView,
    PreExistingRuns,
    HierarchySetup,
    HierarchyPin,
    Bind,
    Drain,
}

// Startup logs a fixed category. Live requests keep their original refusal token.
pub(super) type PolicyCheck = Result<(), (BrokerRefusal, Unavailable)>;

impl BrokerRefusal {
    fn category(self) -> &'static str {
        match self {
            Self::Arguments => "arguments",
            Self::Identity => "identity",
            Self::Binary => "binary",
            Self::SettingsRead => "settings-read",
            Self::SettingsDecode => "settings-decode",
            Self::Layout => "layout",
            Self::PolicyAccount => "policy-account",
            Self::OwnerPolicy => "owner-policy",
            Self::RunnerConstruction => "runner-construction",
            Self::StartupRecheck => "startup-recheck",
            Self::BuildAccount => "build-account",
            Self::Runtime => "runtime",
            Self::Signals => "signals",
            Self::Membership => "membership",
            Self::HierarchyWritableView => "hierarchy-writable-view",
            Self::PreExistingRuns => "pre-existing-runs",
            Self::HierarchySetup => "hierarchy-setup",
            Self::HierarchyPin => "hierarchy-pin",
            Self::Bind => "bind",
            Self::Drain => "drain",
        }
    }
}

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
    fn open(configuration_path: &Path, binary: &Path) -> Result<Self, BrokerRefusal> {
        let content = configuration(configuration_path).map_err(|_| BrokerRefusal::SettingsRead)?;
        let owner: OwnerSettings =
            serde_json::from_slice(&content).map_err(|_| BrokerRefusal::SettingsDecode)?;
        let layout = Layout::system().map_err(|_| BrokerRefusal::Layout)?;
        let service = ServiceConfig::parse_inventory(
            &serde_json::to_vec(&owner.service).map_err(|_| BrokerRefusal::SettingsDecode)?,
            &layout,
            &NativeResolver,
        )
        .map_err(|_| BrokerRefusal::PolicyAccount)?;
        let pin = Pin {
            uid: service
                .build_identity()
                .parse()
                .map_err(|_| BrokerRefusal::OwnerPolicy)?,
            gid: service.build_gid().ok_or(BrokerRefusal::OwnerPolicy)?,
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
            return Err(BrokerRefusal::OwnerPolicy);
        }

        let private = configuration_path
            .parent()
            .ok_or(BrokerRefusal::OwnerPolicy)?;
        let private_meta = protected_directory(private).map_err(|_| BrokerRefusal::OwnerPolicy)?;
        if private_meta.mode() & 0o7777 != 0o700 {
            return Err(BrokerRefusal::OwnerPolicy);
        }
        let base = private.parent().ok_or(BrokerRefusal::OwnerPolicy)?;
        let control = base.join("control");
        let control_meta = protected_directory(&control).map_err(|_| BrokerRefusal::OwnerPolicy)?;
        if control_meta.mode() & 0o7777 != 0o710 || control_meta.gid() != owner.daemon_gid {
            return Err(BrokerRefusal::OwnerPolicy);
        }
        let workspace = Path::new(service.workspace());
        if !canonical_path(workspace)
            || overlap(workspace, private)
            || overlap(workspace, &control)
            || binary.starts_with(private)
        {
            return Err(BrokerRefusal::OwnerPolicy);
        }
        let binary_meta = owner_file(binary, false)
            .and_then(|file| file.metadata())
            .map_err(|_| BrokerRefusal::OwnerPolicy)?;
        if binary_meta.mode() & 0o111 != 0o111 {
            return Err(BrokerRefusal::OwnerPolicy);
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
                return Err(BrokerRefusal::OwnerPolicy);
            }
            ancestry(dir.parent().ok_or(BrokerRefusal::OwnerPolicy)?)
                .map_err(|_| BrokerRefusal::OwnerPolicy)?;
            let metadata = fs::symlink_metadata(&dir).map_err(|_| BrokerRefusal::OwnerPolicy)?;
            if !metadata.is_dir()
                || metadata.uid() != owner.daemon_uid
                || metadata.gid() != owner.daemon_gid
                || metadata.mode() & 0o7777 != 0o700
            {
                return Err(BrokerRefusal::OwnerPolicy);
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

    fn check(&self) -> PolicyCheck {
        let stable = self.objects.iter().all(|(path, expected)| {
            path.parent().is_some_and(|p| ancestry(p).is_ok())
                && fs::symlink_metadata(path).is_ok_and(|m| Fingerprint::from(m) == *expected)
        });
        if !stable || !configuration(&self.configuration).is_ok_and(|c| c == self.content) {
            return Err((BrokerRefusal::StartupRecheck, Unavailable::PreflightFailed));
        }
        let layout =
            Layout::system().map_err(|_| (BrokerRefusal::Layout, Unavailable::PreflightFailed))?;
        self.service
            .execution_preflight(&layout, &NativeResolver)
            .map_err(|unavailable| (BrokerRefusal::BuildAccount, unavailable))
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
enum Operation {
    Status,
    Drain,
    Execute(Request),
    CheckWorkspace {
        slot: String,
    },
    PrepareBuild {
        slot: String,
        ticket: [u8; 32],
        request: Request,
    },
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
enum Reply {
    Ready(Pin),
    Drained,
    Workspace([u8; 32]),
    Prepared,
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
            Self::Ready(_) | Self::Drained | Self::Workspace(_) | Self::Prepared => {
                Report::Failed {
                    creation: Creation::Unknown,
                }
            }
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
    stream.set_write_timeout(Some(HANDSHAKE))?;
    stream.write_all(&frame(operation)?)?;
    receive_reply(stream, budget, stopping)
}

fn receive_reply(
    mut stream: &UnixStream,
    budget: Duration,
    stopping: impl Fn() -> bool,
) -> io::Result<Reply> {
    stream.set_read_timeout(Some(POLL))?;
    let deadline = Instant::now() + budget;
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
    connect_build(socket, build_uid).map(|(execution, _)| execution)
}

pub(super) fn connect_build(
    socket: PathBuf,
    build_uid: u32,
) -> Result<(Arc<Execution>, BuildBroker), Unavailable> {
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
    let client = Arc::new(Client {
        socket,
        pin,
        peer,
        cleanup_unknown: AtomicBool::new(false),
    });
    let execution = Execution::cluster_broker(client.clone(), pin.uid, pin.gid, pin.slots);
    Ok((execution, BuildBroker(client)))
}

#[derive(Clone)]
pub(super) struct BuildBroker(Arc<Client>);

pub(super) struct PreparedBuild {
    client: Arc<Client>,
    stream: UnixStream,
    budget: Duration,
}

fn build_reply(reply: &Reply) -> Unavailable {
    match reply {
        Reply::Unavailable(reason) => unavailable(reason).unwrap_or(Unavailable::PreflightFailed),
        _ => Unavailable::PreflightFailed,
    }
}

impl BuildBroker {
    fn connection(&self) -> Result<UnixStream, Unavailable> {
        let stream = open_socket(&self.0.socket).map_err(|_| Unavailable::MechanismUnavailable)?;
        if credentials(&stream).ok() != Some(self.0.peer) {
            return Err(Unavailable::PreflightFailed);
        }
        Ok(stream)
    }

    pub(super) fn check(&self, slot: String) -> Result<[u8; 32], Unavailable> {
        let stream = self.connection()?;
        let reply = exchange(
            &stream,
            &Operation::CheckWorkspace { slot },
            HANDSHAKE,
            || false,
        )
        .map_err(|_| Unavailable::MechanismUnavailable)?;
        match reply {
            Reply::Workspace(ticket) => Ok(ticket),
            _ => Err(build_reply(&reply)),
        }
    }

    pub(super) fn prepare(
        &self,
        slot: String,
        ticket: [u8; 32],
        request: Request,
    ) -> Result<PreparedBuild, Unavailable> {
        let stream = self.connection()?;
        let budget = Duration::from_secs(request.timeout_s + 6);
        let reply = exchange(
            &stream,
            &Operation::PrepareBuild {
                slot,
                ticket,
                request,
            },
            HANDSHAKE,
            || false,
        )
        .map_err(|_| Unavailable::MechanismUnavailable)?;
        if !matches!(reply, Reply::Prepared) {
            return Err(build_reply(&reply));
        }
        Ok(PreparedBuild {
            client: self.0.clone(),
            stream,
            budget,
        })
    }
}

impl PreparedBuild {
    pub(super) fn execute(mut self, cancel: &Cancellation, shutdown: &Cancellation) -> Report {
        if cancel.is_cancelled() || shutdown.is_cancelled() {
            return Report::CancelledBeforeSpawn;
        }
        let reply = self.stream.write_all(&[BUILD_EXECUTE]).and_then(|()| {
            receive_reply(&self.stream, self.budget, || {
                cancel.is_cancelled() || shutdown.is_cancelled()
            })
        });
        let confirmed = match &reply {
            Ok(Reply::Completed {
                uid,
                gid,
                cleanup: true,
                ..
            }) => (*uid, *gid) == (self.client.pin.uid, self.client.pin.gid),
            Ok(Reply::Unavailable(reason)) => unavailable(reason).is_some(),
            Ok(Reply::Cancelled | Reply::Failed(Creation::NotInvoked | Creation::ProvenNone)) => {
                true
            }
            _ => false,
        };
        if !confirmed {
            self.client.cleanup_unknown.store(true, Ordering::Release);
        }
        reply.map_or(
            Report::Failed {
                creation: Creation::Unknown,
            },
            Reply::into_report,
        )
    }
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

async fn read_operation(stream: &mut ServerStream) -> io::Result<Operation> {
    timeout(HANDSHAKE, async {
        let mut header = [0; 4];
        stream.read_exact(&mut header).await?;
        let mut body = vec![0; frame_len(header)?];
        stream.read_exact(&mut body).await?;
        serde_json::from_slice::<Operation>(&body).map_err(|_| bad())
    })
    .await?
}

#[cfg(test)]
async fn serve(
    mut stream: ServerStream,
    bridge: Arc<impl Executor>,
    pin: Pin,
    shutdown: Cancellation,
    capacity: Arc<Semaphore>,
) {
    if let Ok(operation) = read_operation(&mut stream).await {
        serve_operation(stream, bridge, pin, shutdown, capacity, operation).await;
    }
}

async fn serve_operation(
    mut stream: ServerStream,
    bridge: Arc<impl Executor>,
    pin: Pin,
    shutdown: Cancellation,
    capacity: Arc<Semaphore>,
    operation: Operation,
) {
    let reply = if matches!(operation, Operation::Drain) {
        // Seal admission before inspecting the live Runner inventory. The
        // worker owns its permit through cleanup, even after connection loss.
        capacity.close();
        let deadline = tokio::time::Instant::now() + HANDSHAKE - POLL * 2;
        while capacity.available_permits() != usize::from(pin.slots)
            && !shutdown.is_cancelled()
            && tokio::time::Instant::now() < deadline
        {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
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
            (Ok(()), Operation::CheckWorkspace { .. } | Operation::PrepareBuild { .. }) => {
                Reply::Failed(Creation::NotInvoked)
            }
        }
    };
    if let Ok(record) = frame(&reply) {
        let _ = timeout(HANDSHAKE, stream.write_all(&record)).await;
    }
}

struct Workspace {
    files: Vec<File>,
    identity: Vec<Fingerprint>,
}

fn slot_name(slot: &str) -> bool {
    (1..=48).contains(&slot.len())
        && slot
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}

impl Workspace {
    fn open(root: &Path, slot: &str) -> io::Result<Self> {
        if !canonical_path(root) || !slot_name(slot) {
            return Err(bad());
        }
        let path = root.join(slot);
        let names: Vec<_> = path.as_os_str().as_bytes()[1..]
            .split(|&b| b == b'/')
            .collect();
        if names.len() > 32 {
            return Err(bad());
        }
        let slot_index = names.len() - 1;
        let mut files = vec![File::open("/")?];
        for (i, name) in names.into_iter().enumerate() {
            let name = CString::new(name)?;
            // SAFETY: openat2 receives a live dirfd and this fixed ABI record.
            let how = [
                u64::try_from(libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC).unwrap(),
                0,
                (if i == slot_index { 0x01 } else { 0 }) | 0x04 | 0x08,
            ]; // NO_XDEV below workspace root, NO_SYMLINKS | BENEATH throughout
            let raw = unsafe {
                libc::syscall(
                    libc::SYS_openat2,
                    files.last().unwrap().as_raw_fd(),
                    name.as_ptr(),
                    &how,
                    std::mem::size_of_val(&how),
                )
            };
            if raw < 0 {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: a successful open returned a new owned descriptor.
            files.push(unsafe { File::from_raw_fd(raw as i32) });
        }
        let mut identity = Vec::with_capacity(files.len());
        for (i, file) in files.iter().enumerate() {
            let m = file.metadata()?;
            let access = if i >= slot_index { 0o005 } else { 0o001 };
            if !m.is_dir() || m.uid() != 0 || m.mode() & 0o022 != 0 || m.mode() & access != access {
                return Err(bad());
            }
            // With no ACL, the other read/search bits really apply to the build account.
            // SAFETY: only the existence of the named attribute is queried.
            for name in [c"system.posix_acl_access", c"system.posix_acl_default"] {
                let acl = unsafe {
                    libc::fgetxattr(file.as_raw_fd(), name.as_ptr(), std::ptr::null_mut(), 0)
                };
                if acl != -1 || io::Error::last_os_error().raw_os_error() != Some(libc::ENODATA) {
                    return Err(bad());
                }
            }
            identity.push(m.into());
        }
        Ok(Self { files, identity })
    }

    fn matches(&self, root: &Path, slot: &str) -> bool {
        Self::open(root, slot).is_ok_and(|now| now.identity == self.identity)
            && self
                .files
                .iter()
                .zip(&self.identity)
                .all(|(file, id)| file.metadata().is_ok_and(|m| &Fingerprint::from(m) == id))
    }
}

#[derive(Default)]
struct WorkspaceTickets(Mutex<HashMap<[u8; 32], (Instant, String, Workspace)>>);

impl WorkspaceTickets {
    fn issue(&self, root: &Path, slot: String) -> Result<[u8; 32], Unavailable> {
        let workspace = Workspace::open(root, &slot).map_err(|_| Unavailable::PreflightFailed)?;
        let mut ticket = [0; 32];
        getrandom::fill(&mut ticket).map_err(|_| Unavailable::PreflightFailed)?;
        let now = Instant::now();
        let mut entries = self.0.lock().map_err(|_| Unavailable::PreflightFailed)?;
        entries.retain(|_, (expiry, _, _)| *expiry > now);
        if entries.len() >= MAX_WORKSPACE_TICKETS || entries.contains_key(&ticket) {
            return Err(Unavailable::Busy);
        }
        entries.insert(ticket, (now + WORKSPACE_TTL, slot, workspace));
        Ok(ticket)
    }

    fn take(&self, root: &Path, slot: &str, ticket: [u8; 32]) -> Result<Workspace, Unavailable> {
        let entry = self
            .0
            .lock()
            .map_err(|_| Unavailable::PreflightFailed)?
            .remove(&ticket);
        let (expiry, selected, workspace) = entry.ok_or(Unavailable::PreflightFailed)?;
        (expiry > Instant::now() && selected == slot && workspace.matches(root, slot))
            .then_some(workspace)
            .ok_or(Unavailable::PreflightFailed)
    }
}

async fn send_reply(stream: &mut ServerStream, reply: Reply) -> io::Result<()> {
    let record = frame(&reply)?;
    timeout(HANDSHAKE, stream.write_all(&record)).await?
}

async fn serve_broker(
    mut stream: ServerStream,
    bridge: Arc<super::unix_bridge::UnixBridge>,
    policy: Arc<Policy>,
    shutdown: Cancellation,
    capacity: Arc<Semaphore>,
    workspaces: Arc<WorkspaceTickets>,
) {
    let Ok(operation) = read_operation(&mut stream).await else {
        return;
    };
    let root = Path::new(policy.service.workspace());
    let available = || {
        if shutdown.is_cancelled() || capacity.is_closed() {
            Err(Unavailable::Stopping)
        } else {
            bridge.preflight()
        }
    };
    let reply = match operation {
        Operation::CheckWorkspace { slot } => available()
            .and_then(|()| workspaces.issue(root, slot))
            .map(Reply::Workspace),
        Operation::PrepareBuild {
            slot,
            ticket,
            request,
        } => {
            let prepared = (|| {
                available()?;
                if request.tool.is_none() || !request.cwd.is_empty() || !request.validate() {
                    return Err(Unavailable::PreflightFailed);
                }
                let workspace = workspaces.take(root, &slot, ticket)?;
                let permit = capacity
                    .clone()
                    .try_acquire_owned()
                    .map_err(|_| Unavailable::Busy)?;
                Ok((workspace, permit))
            })();
            match prepared {
                Err(reason) => Err(reason),
                Ok((workspace, permit)) => {
                    if send_reply(&mut stream, Reply::Prepared).await.is_err()
                        || !matches!(
                            timeout(HANDSHAKE, stream.read_u8()).await,
                            Ok(Ok(BUILD_EXECUTE))
                        )
                    {
                        return;
                    }
                    let recheck = available().and_then(|()| {
                        workspace
                            .matches(root, &slot)
                            .then_some(())
                            .ok_or(Unavailable::PreflightFailed)
                    });
                    match recheck {
                        Err(reason) => Err(reason),
                        Ok(()) => {
                            let cancel = Cancellation::default();
                            let child_cancel = cancel.clone();
                            let mut work = tokio::task::spawn_blocking(move || {
                                let _permit = permit;
                                let _workspace = workspace;
                                bridge.run_build(
                                    &request,
                                    Path::new(&slot),
                                    &child_cancel,
                                    &shutdown,
                                )
                            });
                            let result = tokio::select! {
                                biased;
                                _ = stream.read_u8() => { cancel.cancel(); work.await }
                                result = &mut work => result,
                            };
                            Ok(result.map_or(Reply::Failed(Creation::Unknown), Reply::from_report))
                        }
                    }
                }
            }
        }
        other => {
            return serve_operation(stream, bridge, policy.pin, shutdown, capacity, other).await;
        }
    };
    let _ = send_reply(
        &mut stream,
        reply.unwrap_or_else(|r| Reply::Unavailable(r.as_str().into())),
    )
    .await;
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

async fn broker(policy: Arc<Policy>, binary: PathBuf) -> Result<(), BrokerRefusal> {
    let stop = super::Shutdown::default();
    tokio::spawn(
        super::unix::Signals::install()
            .map_err(|_| BrokerRefusal::Signals)?
            .forward(stop.clone()),
    );
    let root = policy
        .configuration
        .parent()
        .ok_or(BrokerRefusal::OwnerPolicy)?
        .to_owned();
    let check_policy = policy.clone();
    let bridge = Arc::new(super::unix_bridge::broker(
        (binary, root),
        &policy.service,
        move || check_policy.check(),
    )?);
    let (endpoint, listener) =
        Endpoint::bind(&policy.socket, policy.daemon.1).map_err(|_| BrokerRefusal::Bind)?;
    println!("ARX_EXEC_SETUP ready");
    let mut work = JoinSet::new();
    let workspaces = Arc::new(WorkspaceTickets::default());
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
                    work.spawn(serve_broker(
                        stream, bridge.clone(), policy.clone(), shutdown.clone(),
                        capacity.clone(), workspaces.clone(),
                    ));
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
        return Err(BrokerRefusal::Drain);
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
            return Err(BrokerRefusal::Arguments);
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
            return Err(BrokerRefusal::Identity);
        }
        let binary = std::env::current_exe().map_err(|_| BrokerRefusal::Binary)?;
        let policy = Arc::new(Policy::open(Path::new(configuration), &binary)?);
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| BrokerRefusal::Runtime)?
            .block_on(broker(policy, binary))
    })();
    Some(match result {
        Ok(()) => 0,
        Err(reason) => {
            eprintln!("ARX_EXEC_REFUSED {}", reason.category());
            1
        }
    })
}

#[cfg(test)]
mod tests;
