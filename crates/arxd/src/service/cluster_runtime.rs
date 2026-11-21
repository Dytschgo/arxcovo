//! Linux ownership of the cluster worker and owner-local endpoint.
use super::{Shutdown, cluster_owner};
use crate::cluster::consensus::bound_start::runtime::{
    self as activation, Admission, Installation, Intent, Route, Running,
};
use std::{
    fs::OpenOptions,
    io::Read,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::sync::{Mutex, watch};

const OBSERVE: Duration = Duration::from_millis(40);
const CLOSE: Duration = Duration::from_secs(10);

use super::config::ServiceLaunch;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    workspace: PathBuf,
    state: PathBuf,
    credentials: PathBuf,
    wrap: PathBuf,
    custody: PathBuf,
    history: PathBuf,
    owner_socket: PathBuf,
    peer: String,
    enrollment: String,
    issuer_peer: Option<String>,
    founder_cluster: Option<[u8; 16]>,
    owner_uid: u32,
    daemon_uid: u32,
    daemon_gid: u32,
    agent_uid: u32,
    build_uid: u32,
    owner_gid: u32,
}

struct Checked {
    installation: Installation,
    routes: Vec<Route>,
    issuer: Option<std::net::SocketAddr>,
    socket: PathBuf,
    roles: cluster_owner::Roles,
}

fn refusal() -> anyhow::Error {
    anyhow::anyhow!("cluster-configuration")
}

fn disjoint(paths: &[&Path]) -> anyhow::Result<()> {
    let mut objects = std::collections::HashSet::new();
    for (i, path) in paths.iter().enumerate() {
        crate::config::checked_path(path).map_err(|_| refusal())?;
        if paths[..i]
            .iter()
            .any(|other| path.starts_with(other) || other.starts_with(path))
        {
            return Err(refusal());
        }
        let metadata = std::fs::metadata(path).map_err(|_| refusal())?;
        if !objects.insert((metadata.dev(), metadata.ino())) {
            return Err(refusal());
        }
    }
    Ok(())
}

impl Settings {
    fn read(path: &Path, uid: u32) -> anyhow::Result<Self> {
        crate::config::checked_path(path).map_err(|_| refusal())?;
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)
            .map_err(|_| refusal())?;
        let metadata = file.metadata().map_err(|_| refusal())?;
        if uid == 0
            || !metadata.is_file()
            || metadata.nlink() != 1
            || metadata.uid() != uid
            || metadata.mode() & 0o077 != 0
        {
            return Err(refusal());
        }
        let mut data = Vec::new();
        file.take(16385)
            .read_to_end(&mut data)
            .map_err(|_| refusal())?;
        if data.len() > 16384 {
            return Err(refusal());
        }
        serde_json::from_slice(&data).map_err(|_| refusal())
    }

    fn layout(&self, path: &Path, ordinary: &[&Path]) -> anyhow::Result<()> {
        let socket_dir = self.owner_socket.parent().ok_or_else(refusal)?;
        let paths = [
            path,
            &self.workspace,
            &self.state,
            &self.credentials,
            &self.wrap,
            &self.custody,
            &self.history,
            socket_dir,
        ];
        disjoint(&paths)?;
        for path in paths {
            for &root in ordinary {
                if path.starts_with(root) || root.starts_with(path) {
                    return Err(refusal());
                }
                if root.exists() {
                    disjoint(&[path, root])?;
                } else if let Some(parent) = root.ancestors().skip(1).find(|p| p.exists()) {
                    crate::config::checked_path(parent).map_err(|_| refusal())?;
                }
            }
        }
        for directory in [
            &self.workspace,
            &self.state,
            &self.credentials,
            &self.wrap,
            &self.custody,
            &self.history,
            self.custody.parent().ok_or_else(refusal)?,
        ] {
            let metadata = std::fs::symlink_metadata(directory).map_err(|_| refusal())?;
            if !metadata.is_dir()
                || metadata.uid() != self.daemon_uid
                || metadata.mode() & 0o7777 != 0o700
            {
                return Err(refusal());
            }
        }
        Ok(())
    }

    fn checked(path: &Path, intent: Intent) -> anyhow::Result<Checked> {
        // SAFETY: these identity getters take no pointers.
        let (uid, gid) = unsafe { (libc::geteuid(), libc::getegid()) };
        let settings = Self::read(path, uid)?;
        if settings.daemon_uid != uid || settings.daemon_gid != gid {
            return Err(refusal());
        }
        settings.layout(path, &[])?;
        let peer = activation::peer_endpoint(&settings.peer).ok_or_else(refusal)?;
        let enrollment = activation::peer_endpoint(&settings.enrollment).ok_or_else(refusal)?;
        if peer.port() == enrollment.port() || peer.ip() != enrollment.ip() {
            return Err(refusal());
        }
        let interfaces = interfaces()?;
        if !interfaces.iter().any(|(address, _)| *address == peer.ip()) {
            return Err(refusal());
        }
        let routes = interfaces
            .into_iter()
            .filter(|(address, _)| *address == peer.ip())
            .map(|(_, route)| route)
            .collect::<Vec<_>>();
        let issuer = match (intent, settings.issuer_peer) {
            (Intent::Join, Some(issuer)) => {
                let address = activation::peer_endpoint(&issuer).ok_or_else(refusal)?;
                if address == peer
                    || address == enrollment
                    || !routes.iter().any(|route| covers(*route, address.ip()))
                {
                    return Err(refusal());
                }
                Some(address)
            }
            (Intent::Join, None) | (Intent::CreateFounder | Intent::RecoverFounder, Some(_)) => {
                return Err(refusal());
            }
            // The immutable join installation keeps its route. Recovery never uses it.
            _ => None,
        };
        if settings.founder_cluster.is_some()
            != matches!(intent, Intent::CreateFounder | Intent::RecoverFounder)
            || settings.founder_cluster == Some([0; 16])
        {
            return Err(refusal());
        }
        Ok(Checked {
            installation: Installation {
                config: path.into(),
                workspace: settings.workspace,
                state: settings.state,
                credentials: settings.credentials,
                history: settings.history,
                wrap: settings.wrap,
                custody: settings.custody,
                peer,
                enrollment,
                founder_cluster: settings.founder_cluster,
            },
            routes,
            issuer,
            socket: settings.owner_socket,
            roles: cluster_owner::Roles {
                owner: settings.owner_uid,
                daemon: settings.daemon_uid,
                agent: settings.agent_uid,
                build: settings.build_uid,
                group: settings.owner_gid,
            },
        })
    }
}

fn prefix(address: IpAddr, mask: IpAddr) -> Option<Route> {
    match (address, mask) {
        (IpAddr::V4(address), IpAddr::V4(mask)) => {
            let bits = u32::from(mask);
            let count = bits.leading_ones();
            (count > 0 && bits.count_ones() == count).then_some(Route {
                network: Ipv4Addr::from(u32::from(address) & bits).into(),
                prefix: count as u8,
            })
        }
        (IpAddr::V6(address), IpAddr::V6(mask)) => {
            let bits = u128::from(mask);
            let count = bits.leading_ones();
            (count > 0 && bits.count_ones() == count).then_some(Route {
                network: Ipv6Addr::from(u128::from(address) & bits).into(),
                prefix: count as u8,
            })
        }
        _ => None,
    }
}

fn covers(route: Route, address: IpAddr) -> bool {
    match (route.network, address) {
        (IpAddr::V4(network), IpAddr::V4(address)) => {
            let mask = u32::MAX << (32 - route.prefix);
            let ip = u32::from(address);
            let host = ip & !mask;
            (ip & mask == u32::from(network))
                && (route.prefix >= 31 || (host != 0 && host != !mask))
        }
        (IpAddr::V6(network), IpAddr::V6(address)) => {
            let mask = u128::MAX << (128 - route.prefix);
            u128::from(address) & mask == u128::from(network)
        }
        _ => false,
    }
}

struct Interfaces(*mut libc::ifaddrs);
impl Drop for Interfaces {
    fn drop(&mut self) {
        // SAFETY: getifaddrs owns this linked list until it is freed here.
        unsafe { libc::freeifaddrs(self.0) };
    }
}

fn sockaddr(address: *const libc::sockaddr) -> Option<IpAddr> {
    // SAFETY: both pointers come from a live getifaddrs list. Inspect the
    // family before interpreting the object as its concrete address format.
    unsafe {
        match i32::from(address.as_ref()?.sa_family) {
            libc::AF_INET => {
                let a = address.cast::<libc::sockaddr_in>().as_ref()?;
                Some(Ipv4Addr::from(a.sin_addr.s_addr.to_ne_bytes()).into())
            }
            libc::AF_INET6 => {
                let a = address.cast::<libc::sockaddr_in6>().as_ref()?;
                Some(Ipv6Addr::from(a.sin6_addr.s6_addr).into())
            }
            _ => None,
        }
    }
}

fn interfaces() -> anyhow::Result<Vec<(IpAddr, Route)>> {
    let mut head = std::ptr::null_mut();
    // SAFETY: head is a valid output pointer and is retained by Interfaces.
    if unsafe { libc::getifaddrs(&mut head) } != 0 {
        return Err(refusal());
    }
    let interfaces = Interfaces(head);
    let mut next = interfaces.0;
    let mut routes = Vec::new();
    for _ in 0..512 {
        // SAFETY: next is null or an entry in the owned linked list.
        let Some(interface) = (unsafe { next.as_ref() }) else {
            return Ok(routes);
        };
        next = interface.ifa_next;
        if interface.ifa_flags & libc::IFF_UP as u32 == 0 {
            continue;
        }
        if let Some(address) = sockaddr(interface.ifa_addr)
            && let Some(mask) = sockaddr(interface.ifa_netmask)
            && let Some(route) = prefix(address, mask)
        {
            routes.push((address, route));
        }
    }
    Err(refusal())
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Status {
    Starting,
    Pending,
    Peer,
    Strict,
    Issuer,
    Transferred,
    Refused,
    Closing,
}

impl Status {
    fn code(self) -> u8 {
        match self {
            Self::Starting => 0,
            Self::Pending => 1,
            Self::Peer => 2,
            Self::Strict => 3,
            Self::Issuer => 4,
            Self::Transferred => 5,
            Self::Refused => 6,
            Self::Closing => 7,
        }
    }
}

struct Work {
    waiting: Option<Installation>,
    running: Option<Running>,
    first: Option<activation::Error>,
}

pub(crate) struct Lifecycle {
    work: Arc<Mutex<Work>>,
    fence: Arc<Admission>,
    joined: bool,
    status: watch::Receiver<Status>,
    control: cluster_owner::Control,
    supervisor: tokio::task::JoinHandle<()>,
}

impl Lifecycle {
    pub(crate) async fn start(
        path: &Path,
        intent: Intent,
        fence: Arc<Admission>,
    ) -> anyhow::Result<Self> {
        let checked = Settings::checked(path, intent)?;
        let endpoint = cluster_owner::Server::bind(checked.socket, checked.roles)
            .map_err(|_| anyhow::anyhow!("cluster-owner-bind"))?;
        let (status, receiver) = watch::channel(Status::Starting);
        let stop = fence.signal();
        let waiting = intent == Intent::Join;
        let (waiting, running) = if waiting || stop.is_cancelled() {
            (waiting.then_some(checked.installation), None)
        } else {
            let running = Running::start(checked.installation, intent, &fence)
                .await
                .map_err(|_| anyhow::anyhow!("cluster-startup"))?;
            (None, Some(running))
        };
        let work = Arc::new(Mutex::new(Work {
            waiting,
            running,
            first: None,
        }));
        let routes = Arc::new(checked.routes);
        let issuer = checked.issuer;
        let dispatch = {
            let work = work.clone();
            let stop = stop.clone();
            let fence = fence.clone();
            let status = status.clone();
            move |operator, request| {
                let work = work.clone();
                let stop = stop.clone();
                let fence = fence.clone();
                let status = status.clone();
                let routes = routes.clone();
                async move {
                    let mut work = work.lock().await;
                    if stop.is_cancelled() || work.first.is_some() {
                        return cluster_owner::Reply::Refused;
                    }
                    use cluster_owner::{Reply, Request};
                    match request {
                        Request::Mint => match &work.running {
                            Some(running) => running.mint(operator).await,
                            None => Reply::Refused,
                        },
                        Request::Join(paste) => {
                            let (Some(installation), Some(issuer)) = (work.waiting.take(), issuer)
                            else {
                                return Reply::Refused;
                            };
                            status.send_replace(Status::Starting);
                            match Running::join(installation, paste, &routes, &fence).await {
                                Ok(running) => work.running = Some(running),
                                Err(reason) => {
                                    work.first = Some(reason);
                                    status.send_replace(Status::Refused);
                                    return Reply::Unknown;
                                }
                            }
                            let running = work.running.as_ref().unwrap();
                            let result = tokio::select! {
                                biased;
                                _ = stop.cancelled() => return Reply::Unknown,
                                result = running.attached() => result,
                            };
                            let result = match result {
                                Ok(()) if !stop.is_cancelled() => running.admit(issuer).await,
                                Ok(()) => return Reply::Unknown,
                                Err(reason) => Err(reason),
                            };
                            match result {
                                Ok(()) => Reply::Joined,
                                Err(reason) => {
                                    work.first = Some(reason);
                                    status.send_replace(Status::Refused);
                                    Reply::Unknown
                                }
                            }
                        }
                    }
                }
            }
        };
        let control = endpoint.start_with_stop(
            dispatch,
            {
                let receiver = receiver.clone();
                let fence = fence.clone();
                move || {
                    let [anchors, redeems] = fence.exchanges();
                    [receiver.borrow().code(), anchors, redeems]
                }
            },
            stop.clone(),
        );
        if work.lock().await.waiting.is_some() {
            status.send_replace(Status::Pending);
        }
        let supervisor = tokio::spawn(hold(work.clone(), status, stop.clone()));
        Ok(Self {
            work,
            fence,
            joined: false,
            status: receiver,
            control,
            supervisor,
        })
    }

    pub(crate) async fn stopped_or_failed(&mut self) {
        let _ = self
            .status
            .wait_for(|status| *status == Status::Refused)
            .await;
    }

    pub(crate) async fn ready(&mut self) -> Result<(), ()> {
        let value = self
            .status
            .wait_for(|status| *status != Status::Starting)
            .await
            .map_err(|_| ())?;
        (*value != Status::Refused).then_some(()).ok_or(())
    }

    pub(crate) async fn quiesce(&mut self) -> Result<(), ()> {
        self.fence.stop();
        let (owner, observer) = tokio::join!(
            self.control.close(CLOSE),
            tokio::time::timeout(CLOSE, &mut self.supervisor),
        );
        self.joined = self.control.joined() && observer.is_ok();
        (owner.is_ok() && matches!(observer, Ok(Ok(()))))
            .then_some(())
            .ok_or(())
    }

    pub(crate) async fn close(self) -> Result<(), ()> {
        if !self.joined {
            return Err(());
        }
        let mut work = self.work.lock().await;
        let running = work.running.take();
        let failed = work.first.is_some();
        drop(work);
        let closed = match running {
            Some(running) => tokio::time::timeout(CLOSE, running.close())
                .await
                .is_ok_and(|r| r.is_ok()),
            None => true,
        };
        (closed && !failed).then_some(()).ok_or(())
    }
}

async fn hold(work: Arc<Mutex<Work>>, status: watch::Sender<Status>, stop: Shutdown) {
    let attach = {
        let work = work.lock().await;
        if let Some(running) = &work.running {
            tokio::select! {
                biased;
                _ = stop.cancelled() => return,
                result = running.attached() => result,
            }
        } else {
            Ok(())
        }
    };
    if let Err(reason) = attach {
        work.lock().await.first = Some(reason);
        status.send_replace(Status::Refused);
        return;
    }
    let mut intervals = tokio::time::interval(OBSERVE);
    intervals.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            biased;
            _ = stop.cancelled() => {
                status.send_replace(Status::Closing);
                return;
            }
            _ = intervals.tick() => {}
        }
        let mut work = work.lock().await;
        if work.first.is_some() {
            return;
        }
        if let Some(running) = work.running.as_mut() {
            match running.observe().await {
                Ok(ready) => {
                    let phase = if ready.issuer && ready.transferred {
                        Status::Transferred
                    } else if ready.issuer {
                        Status::Issuer
                    } else if ready.strict {
                        Status::Strict
                    } else {
                        Status::Peer
                    };
                    status.send_if_modified(|stored| {
                        let changed = *stored != phase;
                        *stored = phase;
                        changed
                    });
                }
                Err(reason) => {
                    work.first = Some(reason);
                    status.send_replace(Status::Refused);
                    return;
                }
            }
        }
    }
}

pub fn entry(args: &[std::ffi::OsString]) -> Option<i32> {
    let (mode, rest) = args.split_first()?;
    let worker = mode == "cluster-service-worker";
    if !worker && mode != "--cluster-config" {
        return None;
    }
    let [config, flag, intent] = rest else {
        return Some(super::ExitCode::Usage as i32);
    };
    if flag != "--cluster-intent" {
        return Some(super::ExitCode::Usage as i32);
    }
    let Some(intent) = intent.to_str().and_then(Intent::parse) else {
        return Some(super::ExitCode::Config as i32);
    };
    let result = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|_| ())
        .and_then(|runtime| {
            let result = runtime.block_on(process(Path::new(config), intent, worker));
            runtime.shutdown_background();
            result
        });
    if result.is_err() {
        eprintln!("arxd: cluster-lifecycle-refused-or-unconfirmed");
    }
    Some(if result.is_ok() {
        0
    } else {
        super::ExitCode::Store as i32
    })
}

const CONTROL_FD: std::os::fd::RawFd = 3;
const READY: u8 = b'R';
const STOP: u8 = b'S';
const JOINED: u8 = b'J';
const CLOSE_NOW: u8 = b'C';
const CLOSED: u8 = b'X';

fn worker_control() -> Result<tokio::net::UnixStream, ()> {
    use std::os::fd::FromRawFd;
    // SAFETY: validation below checks the fixed inherited descriptor and
    // credentials before adopting ownership. Identity queries take no user pointers.
    let valid = unsafe { libc::fcntl(CONTROL_FD, libc::F_GETFD) >= 0 && libc::geteuid() != 0 };
    if !valid {
        return Err(());
    }
    // SAFETY: the worker exclusively owns the inherited descriptor.
    let socket = unsafe { std::os::unix::net::UnixStream::from_raw_fd(CONTROL_FD) };
    // SAFETY: FD_CLOEXEC applies to the already validated descriptor.
    if unsafe { libc::fcntl(CONTROL_FD, libc::F_SETFD, libc::FD_CLOEXEC) } != 0 {
        return Err(());
    }
    socket.set_nonblocking(true).map_err(|_| ())?;
    let stream = tokio::net::UnixStream::from_std(socket).map_err(|_| ())?;
    let peer = stream.peer_cred().map_err(|_| ())?;
    // SAFETY: getppid takes no arguments.
    if peer.uid() != 0 || peer.pid() != Some(unsafe { libc::getppid() }) {
        return Err(());
    }
    Ok(stream)
}

async fn process(path: &Path, intent: Intent, worker: bool) -> Result<(), ()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let fence = Arc::new(Admission::default());
    let stop = fence.signal();
    let signals = super::unix::Signals::install().map_err(|_| ())?;
    let saw_signal = Shutdown::default();
    let forwarding = tokio::spawn(signals.forward(saw_signal.clone()));
    let parent = worker.then(worker_control).transpose()?;
    let (mut reader, mut writer) = parent.map_or((None, None), |stream| {
        let (reader, writer) = stream.into_split();
        (Some(reader), Some(writer))
    });
    let signal_fence = fence.clone();
    let signal_stop = tokio::spawn(async move {
        saw_signal.cancelled().await;
        signal_fence.stop();
    });
    let parent_fence = fence.clone();
    let mut parent_stop = reader.take().map(|mut reader| {
        tokio::spawn(async move {
            let valid = matches!(reader.read_u8().await, Ok(STOP));
            parent_fence.stop();
            valid.then_some(reader).ok_or(())
        })
    });
    let mut lifecycle = Lifecycle::start(path, intent, fence)
        .await
        .map_err(|_| ())?;
    let mut result = Ok(());
    if let Some(stream) = &mut writer {
        result = tokio::select! {
            biased;
            _ = stop.cancelled() => Ok(()),
            ready = lifecycle.ready() => {
                if ready.is_ok() && !stop.is_cancelled() {
                    stream.write_u8(READY).await.map_err(|_| ())
                } else {
                    ready
                }
            }
        };
    }
    if result.is_ok() && !stop.is_cancelled() {
        result = tokio::select! {
            biased;
            _ = stop.cancelled() => Ok(()),
            _ = lifecycle.stopped_or_failed() => Err(()),
        };
    }
    let joined = lifecycle.quiesce().await;
    result = result.and(joined);
    let mut parent_drained = writer.is_none();
    if lifecycle.joined
        && let (Some(stream), Some(reader)) = (&mut writer, &mut parent_stop)
    {
        let disposition = async {
            let mut reader = reader.await.map_err(|_| ())??;
            stream.write_u8(JOINED).await.map_err(|_| ())?;
            matches!(reader.read_u8().await, Ok(CLOSE_NOW))
                .then_some(())
                .ok_or(())
        };
        let disposition = tokio::time::timeout(CLOSE, disposition)
            .await
            .unwrap_or(Err(()));
        parent_drained = disposition.is_ok();
        result = result.and(disposition);
    }
    // A worker whose parent cannot confirm its ordinary drain exits
    // unconfirmed with the retained resources still owned until exit.
    let closed = if parent_drained {
        lifecycle.close().await
    } else {
        Err(())
    };
    result = result.and(closed);
    if let Some(stream) = writer.as_mut()
        && result.is_ok()
    {
        result = stream.write_u8(CLOSED).await.map_err(|_| ());
    }
    signal_stop.abort();
    forwarding.abort();
    if let Some(task) = parent_stop {
        task.abort();
    }
    result
}

#[derive(Clone, Copy)]
enum WorkerIdentity {
    Dedicated(u32, u32),
    #[cfg(test)]
    Inherited(u32, u32),
}

impl WorkerIdentity {
    // Runs in the child after fork. No allocation or NSS.
    fn set_and_verify(self) -> bool {
        // SAFETY: fixed scalar IDs and a null groups list of length zero.
        unsafe {
            let (uid, gid) = match self {
                Self::Dedicated(uid, gid) => {
                    if libc::setgroups(0, std::ptr::null()) != 0
                        || libc::setresgid(gid, gid, gid) != 0
                        || libc::setresuid(uid, uid, uid) != 0
                        || libc::getgroups(0, std::ptr::null_mut()) != 0
                    {
                        return false;
                    }
                    (uid, gid)
                }
                #[cfg(test)]
                Self::Inherited(uid, gid) => (uid, gid),
            };
            libc::getuid() == uid
                && libc::geteuid() == uid
                && libc::getgid() == gid
                && libc::getegid() == gid
        }
    }
}

pub(super) struct Child {
    process: std::process::Child,
    control: tokio::net::UnixStream,
    stopping: bool,
    reaped: Option<std::process::ExitStatus>,
}

impl Child {
    pub(super) fn spawn(
        launch: &ServiceLaunch,
        layout: &super::layout::Layout,
        service: &super::config::ServiceConfig,
    ) -> Result<Self, super::ExitCode> {
        use super::ExitCode;
        use super::config::{AccountKind, Resolver};
        if !launch.config.is_absolute()
            || Intent::parse(&launch.intent).is_none()
            || [launch.daemon_uid, launch.daemon_gid]
                .iter()
                .any(|id| *id == 0 || *id == u32::MAX)
        {
            return Err(ExitCode::Config);
        }
        // SAFETY: geteuid reads the current effective UID.
        if unsafe { libc::geteuid() } != 0 {
            return Err(ExitCode::Permissions);
        }
        let account = super::native::NativeResolver
            .account("arxd-cluster")
            .map_err(|_| ExitCode::Config)?;
        let uid = launch.daemon_uid.to_string();
        if account.kind != AccountKind::User
            || account.identity != uid
            || account.gid != Some(launch.daemon_gid)
            || account.administrative
            || service.build_identity() == uid
            || service.client_identity() == uid
        {
            return Err(ExitCode::Config);
        }
        let settings =
            Settings::read(&launch.config, launch.daemon_uid).map_err(|_| ExitCode::Config)?;
        if settings.daemon_uid != launch.daemon_uid || settings.daemon_gid != launch.daemon_gid {
            return Err(ExitCode::Config);
        }
        let mut ordinary = layout
            .private_dirs()
            .iter()
            .map(PathBuf::as_path)
            .collect::<Vec<_>>();
        ordinary.push(layout.binary().parent().ok_or(ExitCode::Layout)?);
        ordinary.push(layout.endpoint().parent().ok_or(ExitCode::Layout)?);
        ordinary.push(Path::new(service.workspace()));
        settings
            .layout(&launch.config, &ordinary)
            .map_err(|_| ExitCode::Layout)?;

        let mut command =
            std::process::Command::new(std::env::current_exe().map_err(|_| ExitCode::Layout)?);
        command
            .arg("cluster-service-worker")
            .arg(&launch.config)
            .arg("--cluster-intent")
            .arg(&launch.intent);
        Self::spawn_worker(
            command,
            WorkerIdentity::Dedicated(launch.daemon_uid, launch.daemon_gid),
        )
    }

    fn spawn_worker(
        mut command: std::process::Command,
        identity: WorkerIdentity,
    ) -> Result<Self, super::ExitCode> {
        use super::ExitCode;
        use std::os::fd::AsRawFd;
        use std::{os::unix::process::CommandExt, process::Stdio};
        let (parent, child) =
            std::os::unix::net::UnixStream::pair().map_err(|_| ExitCode::Permissions)?;
        parent
            .set_nonblocking(true)
            .map_err(|_| ExitCode::Permissions)?;
        let control =
            tokio::net::UnixStream::from_std(parent).map_err(|_| ExitCode::Permissions)?;
        command
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit());
        // SAFETY: the post-fork closure uses only direct system calls and
        // fixed arguments. It accepts no input, performs no allocation, and
        // all descriptors outside the allowlist must close at exec.
        unsafe {
            command.pre_exec(move || {
                let fd = child.as_raw_fd();
                if libc::dup2(fd, CONTROL_FD) == -1
                    || libc::fcntl(CONTROL_FD, libc::F_SETFD, 0) == -1
                    || libc::syscall(
                        libc::SYS_close_range,
                        4u32,
                        u32::MAX,
                        libc::CLOSE_RANGE_CLOEXEC,
                    ) == -1
                    || !identity.set_and_verify()
                {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let process = command.spawn().map_err(|_| ExitCode::Permissions)?;
        Ok(Self {
            process,
            control,
            stopping: false,
            reaped: None,
        })
    }

    pub(super) async fn wait_ready(&mut self) -> Result<(), super::ExitCode> {
        use tokio::io::AsyncReadExt;
        matches!(self.control.read_u8().await, Ok(READY))
            .then_some(())
            .ok_or(super::ExitCode::Endpoint)
    }

    pub(super) async fn exited(&mut self) -> Result<(), super::ExitCode> {
        let _ = self.reap().await;
        Err(super::ExitCode::Endpoint)
    }

    async fn reap(&mut self) -> std::io::Result<std::process::ExitStatus> {
        loop {
            if let Some(status) = self.process.try_wait()? {
                self.reaped = Some(status);
                return Ok(status);
            }
            tokio::time::sleep(OBSERVE).await;
        }
    }

    pub(super) async fn quiesce(&mut self) -> Result<(), super::ExitCode> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        if self.stopping {
            return Ok(());
        }
        self.stopping = true;
        self.control
            .write_u8(STOP)
            .await
            .map_err(|_| super::ExitCode::Endpoint)?;
        loop {
            match self.control.read_u8().await {
                Ok(READY) => continue,
                Ok(JOINED) => return Ok(()),
                _ => return Err(super::ExitCode::Endpoint),
            }
        }
    }

    pub(super) async fn finish(&mut self) -> Result<(), super::ExitCode> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let closed = tokio::time::timeout(CLOSE, async {
            self.stopping
                && self.control.write_u8(CLOSE_NOW).await.is_ok()
                && matches!(self.control.read_u8().await, Ok(CLOSED))
        })
        .await
        .unwrap_or(false);
        let _ = self.control.shutdown().await;
        let reaped = self.observe_reap().await;
        (closed && reaped)
            .then_some(())
            .ok_or(super::ExitCode::Store)
    }
    pub(super) async fn abandon(&mut self) {
        use tokio::io::AsyncWriteExt;
        let _ = self.control.shutdown().await;
        self.observe_reap().await;
    }

    pub(super) async fn observe_reap(&mut self) -> bool {
        tokio::time::timeout(CLOSE, self.reap())
            .await
            .is_ok_and(|r| r.is_ok_and(|status| status.success()))
    }
}

#[cfg(test)]
pub(crate) fn publication_inputs(path: &Path) -> Result<(Installation, Vec<Route>), ()> {
    let checked = Settings::checked(path, Intent::Join).map_err(|_| ())?;
    Ok((checked.installation, checked.routes))
}

#[cfg(all(test, target_os = "linux"))]
mod tests;
