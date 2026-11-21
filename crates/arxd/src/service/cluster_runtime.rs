//! Linux ownership of the cluster worker and owner-local endpoint.
use super::{Shutdown, cluster_owner};
use crate::cluster::consensus::activation::{
    self as activation, Admission, InheritedListeners, Installation, Intent, Route, Running,
};
use std::{
    collections::BTreeMap,
    ffi::CString,
    fs::{File, Metadata, OpenOptions},
    io::Read,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{
            ffi::OsStrExt,
            fs::{MetadataExt, OpenOptionsExt},
        },
    },
    path::{Component, Path, PathBuf},
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
    executor_socket: Option<PathBuf>,
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
    executor_socket: Option<PathBuf>,
    roles: cluster_owner::Roles,
}

fn refusal() -> anyhow::Error {
    anyhow::anyhow!("cluster-configuration")
}

fn overlaps(left: &Path, right: &Path) -> bool {
    left.starts_with(right) || right.starts_with(left)
}

fn object(metadata: &Metadata) -> (u64, u64) {
    (metadata.dev(), metadata.ino())
}

// Each shared ancestor is opened once. All later lookups are relative to
// those directory handles, including the configuration read and alias checks.
struct LayoutPass(BTreeMap<PathBuf, (File, Metadata)>);

impl LayoutPass {
    fn new() -> anyhow::Result<Self> {
        let root = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_PATH | libc::O_DIRECTORY | libc::O_NOFOLLOW)
            .open("/")
            .map_err(|_| refusal())?;
        let metadata = root.metadata().map_err(|_| refusal())?;
        Ok(Self(BTreeMap::from([(
            PathBuf::from("/"),
            (root, metadata),
        )])))
    }

    fn open(&mut self, path: &Path, config: bool) -> anyhow::Result<bool> {
        let mut components = path.components();
        if components.next() != Some(Component::RootDir)
            || path.as_os_str().as_bytes().len() >= libc::PATH_MAX as usize
        {
            return Err(refusal());
        }
        let mut prefix = PathBuf::from("/");
        for part in components {
            let Component::Normal(part) = part else {
                return Err(refusal());
            };
            let parent = &self.0[&prefix];
            if !parent.1.is_dir() {
                return Err(refusal());
            }
            let dir = parent.0.as_raw_fd();
            prefix.push(part);
            if self.0.contains_key(&prefix) {
                continue;
            }
            let name = CString::new(part.as_bytes()).map_err(|_| refusal())?;
            let access = if config && prefix == path {
                libc::O_RDONLY | libc::O_NONBLOCK
            } else {
                libc::O_PATH
            };
            // SAFETY: dir is a borrowed directory descriptor, and name is a
            // single NUL-terminated path component. The returned fd is owned.
            let fd = unsafe {
                libc::openat(
                    dir,
                    name.as_ptr(),
                    access | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                )
            };
            if fd == -1 {
                if std::io::Error::last_os_error().kind() == std::io::ErrorKind::NotFound {
                    return Ok(false);
                }
                return Err(refusal());
            }
            // SAFETY: openat succeeded and returned a new descriptor.
            let file = unsafe { File::from_raw_fd(fd) };
            let metadata = file.metadata().map_err(|_| refusal())?;
            if metadata.is_symlink() {
                return Err(refusal());
            }
            self.0.insert(prefix.clone(), (file, metadata));
        }
        Ok(true)
    }

    fn stamp(&self, data: &[u8]) -> ConfigStamp {
        use sha2::{Digest, Sha256};
        let mut digest = Sha256::new();
        digest.update((data.len() as u64).to_be_bytes());
        digest.update(data);
        for (path, (_, metadata)) in &self.0 {
            let path = path.as_os_str().as_bytes();
            digest.update((path.len() as u64).to_be_bytes());
            digest.update(path);
            for value in [
                metadata.dev(),
                metadata.ino(),
                metadata.mode().into(),
                metadata.uid().into(),
                metadata.gid().into(),
            ] {
                digest.update(value.to_be_bytes());
            }
        }
        ConfigStamp(digest.finalize().into())
    }

    fn ordinary_disjoint(&mut self, root: &Path, paths: &[&Path]) -> anyhow::Result<()> {
        if paths.iter().any(|path| overlaps(path, root)) {
            return Err(refusal());
        }
        let exists = self.open(root, false)?;
        for &path in paths {
            let entry = object(&self.0[path].1);
            // A differently named ordinary ancestor can still be the same
            // object as a protected root (for example through a bind mount).
            if root
                .ancestors()
                .filter_map(|ancestor| self.0.get(ancestor))
                .any(|(_, metadata)| object(metadata) == entry)
                || (exists
                    && path
                        .ancestors()
                        .any(|ancestor| object(&self.0[ancestor].1) == object(&self.0[root].1)))
            {
                return Err(refusal());
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct ConfigStamp([u8; 32]);

impl Settings {
    fn vetted(
        path: &Path,
        uid: u32,
        ordinary: &[&Path],
        expected: Option<ConfigStamp>,
    ) -> anyhow::Result<(Self, ConfigStamp)> {
        let mut pass = LayoutPass::new()?;
        if !pass.open(path, true)? {
            return Err(refusal());
        }
        let (file, metadata) = &pass.0[path];
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
        let settings: Self = serde_json::from_slice(&data).map_err(|_| refusal())?;
        if settings.daemon_uid != uid {
            return Err(refusal());
        }
        let socket_dir = settings.owner_socket.parent().ok_or_else(refusal)?;
        let private = [
            settings.workspace.as_path(),
            &settings.state,
            &settings.credentials,
            &settings.wrap,
            &settings.custody,
            &settings.history,
        ];
        let paths = [
            path, private[0], private[1], private[2], private[3], private[4], private[5],
            socket_dir,
        ];
        for (i, &path) in paths.iter().enumerate().skip(1) {
            if !pass.open(path, false)?
                || !pass.0[path].1.is_dir()
                || paths[..i].iter().any(|other| {
                    overlaps(path, other) || object(&pass.0[path].1) == object(&pass.0[*other].1)
                })
            {
                return Err(refusal());
            }
        }
        let custody_parent = settings.custody.parent().ok_or_else(refusal)?;
        for dir in private.into_iter().chain([custody_parent]) {
            let metadata = &pass.0[dir].1;
            if metadata.uid() != uid || metadata.mode() & 0o7777 != 0o700 {
                return Err(refusal());
            }
        }
        let socket_metadata = &pass.0[socket_dir].1;
        if socket_metadata.uid() != uid
            || socket_metadata.gid() != settings.owner_gid
            || socket_metadata.mode() & 0o7777 != 0o2710
        {
            return Err(refusal());
        }
        if expected.is_some() || !ordinary.is_empty() {
            // Native activation uses these paths after handoff. Forbid a
            // third-party writer from moving any component in that layout.
            for (_, metadata) in pass.0.values().filter(|(_, metadata)| metadata.is_dir()) {
                let mode = metadata.mode();
                if ![0, uid].contains(&metadata.uid())
                    || (mode & 0o022 != 0 && !(metadata.uid() == 0 && mode & 0o1000 != 0))
                {
                    return Err(refusal());
                }
            }
        }
        let stamp = pass.stamp(&data);
        if expected.is_some_and(|expected| expected != stamp) {
            return Err(refusal());
        }
        for root in ordinary {
            pass.ordinary_disjoint(root, &paths)?;
        }
        Ok((settings, stamp))
    }

    fn checked(
        path: &Path,
        intent: Intent,
        expected: Option<ConfigStamp>,
    ) -> anyhow::Result<Checked> {
        // SAFETY: these identity getters take no pointers.
        let (uid, gid) = unsafe { (libc::geteuid(), libc::getegid()) };
        let (settings, _) = Self::vetted(path, uid, &[], expected)?;
        if settings.daemon_gid != gid {
            return Err(refusal());
        }
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
            executor_socket: settings.executor_socket,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LifecycleFailure {
    RefusedOrUnconfirmed,
    LearnerLockRetained,
}

impl LifecycleFailure {
    fn category(self) -> &'static str {
        match self {
            Self::RefusedOrUnconfirmed => "cluster-lifecycle-refused-or-unconfirmed",
            Self::LearnerLockRetained => "cluster-learner-outer-lock-retained",
        }
    }
}

impl From<()> for LifecycleFailure {
    fn from(_: ()) -> Self {
        Self::RefusedOrUnconfirmed
    }
}

impl From<activation::Error> for LifecycleFailure {
    fn from(reason: activation::Error) -> Self {
        match reason {
            activation::Error::LearnerLockRetained => Self::LearnerLockRetained,
            _ => Self::RefusedOrUnconfirmed,
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
    async fn start(
        checked: Checked,
        intent: Intent,
        fence: Arc<Admission>,
    ) -> anyhow::Result<Self> {
        if !fence.check_addresses(checked.installation.peer, checked.installation.enrollment) {
            return Err(refusal());
        }
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
                    let mut evidence = fence.evidence();
                    evidence[0] = receiver.borrow().code();
                    evidence
                }
            },
            {
                let fence = fence.clone();
                move || fence.time_evidence()
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
        self.joined = owner.is_ok() && matches!(observer, Ok(Ok(())));
        self.joined.then_some(()).ok_or(())
    }

    // Ok records a confirmed owned close and whether work had already refused.
    async fn close(self) -> Result<bool, LifecycleFailure> {
        if !self.joined {
            return Err(LifecycleFailure::RefusedOrUnconfirmed);
        }
        let mut work = self.work.lock().await;
        let running = work.running.take();
        let failed = work.first.is_some();
        drop(work);
        let refused = match running {
            Some(running) => tokio::time::timeout(CLOSE, running.close())
                .await
                .map_err(|_| LifecycleFailure::RefusedOrUnconfirmed)??,
            None => false,
        };
        Ok(failed || refused)
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
    let (config, flag, intent, inherit) = match rest {
        [config, flag, intent] => (config, flag, intent, false),
        [config, flag, intent, opt] if !worker && opt == "--inherit-listeners" => {
            (config, flag, intent, true)
        }
        _ => return Some(super::ExitCode::Usage as i32),
    };
    if flag != "--cluster-intent" {
        return Some(super::ExitCode::Usage as i32);
    }
    let Some(intent) = intent.to_str().and_then(Intent::parse) else {
        return Some(super::ExitCode::Config as i32);
    };
    let inherited = match inherit.then(|| inherited_listeners(intent)).transpose() {
        Ok(listeners) => listeners,
        Err(()) => return Some(super::ExitCode::Config as i32),
    };
    let result = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|_| LifecycleFailure::RefusedOrUnconfirmed)
        .and_then(|runtime| {
            let result = runtime.block_on(process(Path::new(config), intent, worker, inherited));
            runtime.shutdown_background();
            result
        });
    Some(match result {
        Ok(()) => 0,
        Err(failure) => {
            eprintln!("arxd: {}", failure.category());
            super::ExitCode::Store as i32
        }
    })
}

const CONTROL_FD: std::os::fd::RawFd = 3;
const PEER_FD: std::os::fd::RawFd = 3;
const ENROLLMENT_FD: std::os::fd::RawFd = 4;

fn listener_option(fd: std::os::fd::RawFd, name: libc::c_int) -> Option<libc::c_int> {
    let mut value = 0;
    let mut len = std::mem::size_of_val(&value) as libc::socklen_t;
    // SAFETY: valid initialized output storage for an integer socket option.
    let rc = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            name,
            (&mut value as *mut libc::c_int).cast(),
            &mut len,
        )
    };
    (rc == 0 && len as usize == std::mem::size_of_val(&value)).then_some(value)
}

fn qualify_listener(
    fd: std::os::fd::RawFd,
    expected: Option<std::net::SocketAddr>,
    listening: bool,
) -> Result<(), ()> {
    if listener_option(fd, libc::SO_TYPE) != Some(libc::SOCK_STREAM)
        || listener_option(fd, libc::SO_PROTOCOL) != Some(libc::IPPROTO_TCP)
        || listener_option(fd, libc::SO_ACCEPTCONN) != Some(i32::from(listening))
        || (!listening && !unconnected(fd))
    {
        return Err(());
    }
    let domain = listener_option(fd, libc::SO_DOMAIN);
    match expected {
        Some(address) => {
            let family = if address.is_ipv4() {
                libc::AF_INET
            } else {
                libc::AF_INET6
            };
            (domain == Some(family)).then_some(()).ok_or(())
        }
        None => matches!(domain, Some(libc::AF_INET | libc::AF_INET6))
            .then_some(())
            .ok_or(()),
    }
}

fn unconnected(fd: std::os::fd::RawFd) -> bool {
    // A non-listening connected TCP socket cannot serve as a reservation.
    let mut peer: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of_val(&peer) as libc::socklen_t;
    // SAFETY: peer and len are writable, correctly sized output storage.
    let rc = unsafe {
        libc::getpeername(
            fd,
            (&mut peer as *mut libc::sockaddr_storage).cast(),
            &mut len,
        )
    };
    rc == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOTCONN)
}

pub(crate) fn validate_listener(
    listener: &std::net::TcpListener,
    address: std::net::SocketAddr,
    listening: bool,
) -> anyhow::Result<()> {
    use std::os::fd::AsRawFd;
    qualify_listener(listener.as_raw_fd(), Some(address), listening).map_err(|_| refusal())?;
    if listener.local_addr().ok() != Some(address) {
        return Err(refusal());
    }
    Ok(())
}

/// Called only at the retained peer or issuer listener registration boundary.
pub(crate) fn listen_inherited(listener: &std::net::TcpListener) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;
    let fd = listener.as_raw_fd();
    // SAFETY: fd is a live, checked TCP socket owned by the retained caller.
    if listener_option(fd, libc::SO_ACCEPTCONN) == Some(0) && unsafe { libc::listen(fd, 128) } != 0
    {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

fn inherited_listeners(intent: Intent) -> Result<InheritedListeners, ()> {
    use std::os::fd::FromRawFd;
    let listening = intent != Intent::Join;
    // Before starting any worker threads, check the exec-time FD whitelist.
    // read_dir transiently owns its own scan descriptor. That descriptor is
    // closed before checking whether any additional descriptors remain open.
    let descriptors = std::fs::read_dir("/proc/self/fd")
        .map_err(|_| ())?
        .take(10)
        .map(|entry| {
            entry
                .map_err(|_| ())?
                .file_name()
                .to_str()
                .ok_or(())?
                .parse::<i32>()
                .map_err(|_| ())
        })
        .collect::<Result<Vec<_>, _>>()?;
    if descriptors.len() >= 10
        || descriptors.iter().any(|&fd| {
            // SAFETY: fcntl only queries the scalar descriptor.
            fd > ENROLLMENT_FD && unsafe { libc::fcntl(fd, libc::F_GETFD) } >= 0
        })
    {
        return Err(());
    }
    for fd in [PEER_FD, ENROLLMENT_FD] {
        qualify_listener(fd, None, listening)?;
        // SAFETY: qualification proved a live TCP descriptor passed to this process.
        if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } != 0 {
            return Err(());
        }
    }
    // SAFETY: exec passed distinct owned descriptors at the two fixed positions.
    Ok(InheritedListeners {
        peer: Some(unsafe { std::net::TcpListener::from_raw_fd(PEER_FD) }),
        enrollment: Some(unsafe { std::net::TcpListener::from_raw_fd(ENROLLMENT_FD) }),
        listening,
    })
}
const READY: u8 = b'R';
const FAILED: u8 = b'F';
const UNJOINED: u8 = b'U';
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

async fn process(
    path: &Path,
    intent: Intent,
    worker: bool,
    inherited: Option<InheritedListeners>,
) -> Result<(), LifecycleFailure> {
    use tokio::io::AsyncReadExt;

    let fence = Arc::new(Admission::with_listeners(inherited));
    let stop = fence.signal();
    let signals = super::unix::Signals::install().map_err(|_| ())?;
    let saw_signal = Shutdown::default();
    let forwarding = tokio::spawn(signals.forward(saw_signal.clone()));
    let mut parent = worker.then(worker_control).transpose()?;
    let mut expected = None;
    if let Some(stream) = &mut parent {
        let mut stamp = ConfigStamp([0; 32]);
        tokio::time::timeout(CLOSE, stream.read_exact(&mut stamp.0))
            .await
            .map_err(|_| ())?
            .map_err(|_| ())?;
        expected = Some(stamp);
    }
    let mut checked = Settings::checked(path, intent, expected).map_err(|_| ())?;
    let build_uid = checked.roles.build;
    let executor = match checked.executor_socket.take() {
        Some(socket) => Some(
            tokio::task::spawn_blocking(move || {
                super::cluster_build_executor::connect(socket, build_uid)
            })
            .await
            .map_err(|_| ())?
            .map_err(|_| ())?,
        ),
        None => None,
    };
    let signal_fence = fence.clone();
    let signal_stop = tokio::spawn(async move {
        saw_signal.cancelled().await;
        signal_fence.stop();
    });
    let parent = parent.map(|stream| ParentControl::new(stream, fence.clone()));
    let lifecycle = Lifecycle::start(checked, intent, fence)
        .await
        .map_err(|_| ())?;
    let result = run_lifecycle_with_executor(lifecycle, stop, parent, executor).await;
    signal_stop.abort();
    forwarding.abort();
    result
}

struct ParentControl {
    stop_received: Arc<std::sync::atomic::AtomicBool>,
    writer: tokio::net::unix::OwnedWriteHalf,
    stop: tokio::task::JoinHandle<Result<tokio::net::unix::OwnedReadHalf, ()>>,
}

impl ParentControl {
    fn new(stream: tokio::net::UnixStream, fence: Arc<Admission>) -> Self {
        use tokio::io::AsyncReadExt;
        let (mut reader, writer) = stream.into_split();
        let stop_received = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let received = stop_received.clone();
        let stop = tokio::spawn(async move {
            let valid = matches!(reader.read_u8().await, Ok(STOP));
            received.store(valid, std::sync::atomic::Ordering::Release);
            fence.stop();
            valid.then_some(reader).ok_or(())
        });
        Self {
            writer,
            stop,
            stop_received,
        }
    }

    fn stop_received(&self) -> bool {
        self.stop_received
            .load(std::sync::atomic::Ordering::Acquire)
    }

    async fn authorize_close(&mut self, joined: bool) -> Result<(), ()> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        // The parent bounds quiesce and its ordinary drain. It closes control
        // if either is unconfirmed. Waiting on that channel avoids an
        // independent worker timer expiring during the parent's drain.
        let mut reader = (&mut self.stop).await.map_err(|_| ())??;
        self.writer
            .write_u8(if joined { JOINED } else { UNJOINED })
            .await
            .map_err(|_| ())?;
        (joined && matches!(reader.read_u8().await, Ok(CLOSE_NOW)))
            .then_some(())
            .ok_or(())
    }
}

impl Drop for ParentControl {
    fn drop(&mut self) {
        self.stop.abort();
    }
}

#[cfg(test)]
async fn run_lifecycle(
    lifecycle: Lifecycle,
    stop: Shutdown,
    parent: Option<ParentControl>,
) -> Result<(), LifecycleFailure> {
    run_lifecycle_with_executor(lifecycle, stop, parent, None).await
}

async fn run_lifecycle_with_executor(
    mut lifecycle: Lifecycle,
    stop: Shutdown,
    mut parent: Option<ParentControl>,
    executor: Option<Arc<super::executor::Execution>>,
) -> Result<(), LifecycleFailure> {
    use tokio::io::AsyncWriteExt;

    let mut result = Ok(());
    if let Some(parent) = &mut parent {
        result = tokio::select! {
            biased;
            _ = stop.cancelled() => Ok(()),
            ready = lifecycle.ready() => {
                if ready.is_ok() && !stop.is_cancelled() {
                    parent.writer.write_u8(READY).await.map_err(|_| ())
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
    lifecycle.fence.stop();
    if let Some(parent) = &mut parent
        && (result.is_err() || !parent.stop_received())
    {
        let notice = parent.writer.write_u8(FAILED).await.map_err(|_| ());
        result = result.and(notice);
    }
    let (joined, execution_closed) = tokio::join!(lifecycle.quiesce(), async {
        match executor {
            Some(execution) => matches!(
                tokio::time::timeout(CLOSE, execution.drain()).await,
                Ok(Ok(()))
            ),
            None => true,
        }
    });
    let joined = joined.and_then(|()| execution_closed.then_some(()).ok_or(()));
    let authorized = match &mut parent {
        Some(parent) => parent.authorize_close(joined.is_ok()).await,
        None => joined,
    };
    authorized?;
    let failed = lifecycle.close().await?;
    if let Some(parent) = &mut parent {
        parent.writer.write_u8(CLOSED).await.map_err(|_| ())?;
    }
    result
        .and((!failed).then_some(()).ok_or(()))
        .map_err(Into::into)
}

#[cfg(test)]
pub(super) async fn broker_refusal_close_control(
    executor: Arc<super::executor::Execution>,
    execute_received: tokio::sync::oneshot::Receiver<()>,
) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let fence = Arc::new(Admission::default());
    let (_status, receiver) = watch::channel(Status::Pending);
    let lifecycle = Lifecycle {
        work: Arc::new(Mutex::new(Work {
            waiting: None,
            running: None,
            first: None,
        })),
        fence: fence.clone(),
        joined: false,
        status: receiver,
        control: cluster_owner::Control::observe_task(tokio::spawn(async { Ok(()) })),
        supervisor: tokio::spawn(async {}),
    };
    let (mut parent, worker) = tokio::net::UnixStream::pair().unwrap();
    let stop = fence.signal();
    let worker = ParentControl::new(worker, fence);
    let run = tokio::spawn(run_lifecycle_with_executor(
        lifecycle,
        stop,
        Some(worker),
        Some(executor),
    ));
    tokio::time::timeout(CLOSE, async {
        assert_eq!(parent.read_u8().await.unwrap(), READY);
        execute_received.await.unwrap();
        parent.write_u8(STOP).await.unwrap();
        assert_eq!(parent.read_u8().await.unwrap(), UNJOINED);
        assert_eq!(parent.read(&mut [0; 1]).await.unwrap(), 0);
        assert_eq!(
            run.await.unwrap(),
            Err(LifecycleFailure::RefusedOrUnconfirmed)
        );
    })
    .await
    .unwrap();
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
    joined: bool,
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
        let mut ordinary = layout
            .private_dirs()
            .iter()
            .map(PathBuf::as_path)
            .collect::<Vec<_>>();
        ordinary.push(layout.binary().parent().ok_or(ExitCode::Layout)?);
        ordinary.push(layout.endpoint().parent().ok_or(ExitCode::Layout)?);
        ordinary.push(Path::new(service.workspace()));
        let (settings, stamp) =
            Settings::vetted(&launch.config, launch.daemon_uid, &ordinary, None)
                .map_err(|_| ExitCode::Config)?;
        if settings.daemon_gid != launch.daemon_gid {
            return Err(ExitCode::Config);
        }

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
            Some(stamp),
        )
    }

    fn spawn_worker(
        mut command: std::process::Command,
        identity: WorkerIdentity,
        stamp: Option<ConfigStamp>,
    ) -> Result<Self, super::ExitCode> {
        use super::ExitCode;
        use std::os::fd::AsRawFd;
        use std::{os::unix::process::CommandExt, process::Stdio};
        let (mut parent, child) =
            std::os::unix::net::UnixStream::pair().map_err(|_| ExitCode::Permissions)?;
        parent
            .set_nonblocking(true)
            .map_err(|_| ExitCode::Permissions)?;
        if let Some(stamp) = stamp {
            use std::io::Write;
            parent
                .write_all(&stamp.0)
                .map_err(|_| ExitCode::Permissions)?;
        }
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
            joined: false,
            reaped: None,
        })
    }

    pub(super) async fn wait_ready(&mut self) -> Result<(), super::ExitCode> {
        use tokio::io::AsyncReadExt;
        matches!(self.control.read_u8().await, Ok(READY))
            .then_some(())
            .ok_or(super::ExitCode::Endpoint)
    }

    pub(super) async fn failed_or_exited(&mut self) -> Result<(), super::ExitCode> {
        use tokio::io::AsyncReadExt;
        // A failure byte or a lost control connection starts ordinary drain.
        // Reap belongs to finish, after the drain and close decision.
        let _ = self.control.read_u8().await;
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
            return self.joined.then_some(()).ok_or(super::ExitCode::Endpoint);
        }
        self.stopping = true;
        self.control
            .write_u8(STOP)
            .await
            .map_err(|_| super::ExitCode::Endpoint)?;
        loop {
            match self.control.read_u8().await {
                Ok(READY | FAILED) => continue,
                Ok(JOINED) => {
                    self.joined = true;
                    return Ok(());
                }
                _ => return Err(super::ExitCode::Endpoint),
            }
        }
    }

    pub(super) async fn finish(&mut self) -> Result<(), super::ExitCode> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let closed = tokio::time::timeout(CLOSE, async {
            self.joined
                && self.control.write_u8(CLOSE_NOW).await.is_ok()
                && matches!(self.control.read_u8().await, Ok(CLOSED))
        })
        .await
        .unwrap_or(false);
        let _ = self.control.shutdown().await;
        let reaped = self.observe_reap().await;
        match (closed, self.reaped, reaped) {
            (true, _, true) => Ok(()),
            (true, Some(_), false) => Err(super::ExitCode::Endpoint),
            _ => Err(super::ExitCode::Store),
        }
    }
    pub(super) async fn finish_after_drain(
        &mut self,
        drained: bool,
    ) -> Result<(), super::ExitCode> {
        if drained {
            return self.finish().await;
        }
        self.abandon().await;
        Err(super::ExitCode::Store)
    }

    async fn abandon(&mut self) {
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
    let checked = Settings::checked(path, Intent::Join, None).map_err(|_| ())?;
    Ok((checked.installation, checked.routes))
}

#[cfg(all(test, target_os = "linux"))]
mod tests;
