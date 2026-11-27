//! Owner publication of the private installation file (ADR 0046, 0047).
//!
//! Root only. The bytes must pass the runtime's own schema, endpoint, role and
//! route checks, the owner-socket route must pass the ADR 0046 owner, mode and
//! ACL policy, and the complete layout must pass `Settings::vetted` with the
//! installed service roots as ordinary roots, and the daemon and operator must
//! be able to search every directory they traverse at run time. The content
//! is staged as an unnamed file in a caller-owned directory and published
//! once with a non-replacing link. A refusal leaves no new file behind.
use super::{Route, Settings, cluster_owner, covers, interfaces, object};
use crate::cluster::consensus::activation::peer_endpoint;
use crate::service::{
    cluster_install::{self as install, DAEMON_ACCOUNT, Ids, InstallError, Request, Role},
    config::{AccountKind, Resolver},
    layout::Layout,
    native::NativeResolver,
};
use std::{
    ffi::CString,
    fs::{File, OpenOptions},
    io::{Seek, Write},
    net::IpAddr,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{
            ffi::OsStrExt,
            fs::{MetadataExt, OpenOptionsExt},
        },
    },
    path::{Component, Path, PathBuf},
};

pub(in crate::service) fn publish(request: &Request, layout: &Layout) -> Result<u32, InstallError> {
    install::check(request)?;
    crate::service::owner_unix::require_owner().map_err(|_| InstallError::OwnerRequired)?;
    let ids = resolve(request, &NativeResolver, group)?;
    let founder = match request.role {
        Role::Founder => Some(founder_cluster()?),
        Role::Joiner => None,
    };
    let bytes = install::document(request, &ids, founder)?;
    let settings = parity(&bytes)?;
    roles(&ids)?;
    let local = interfaces().map_err(|_| InstallError::Address)?;
    if !reachable(&settings, &local) {
        return Err(InstallError::Address);
    }
    socket_route(&settings)?;
    // The daemon as the native worker runs (primary GID only); the operator
    // with its current groups, as it runs `cluster-control` interactively.
    let mut access = Access::worker(ids.daemon_uid, ids.daemon_gid);
    access.owner = Some(Identity {
        uid: ids.owner_uid,
        groups: cluster_owner::account_groups(ids.owner_uid).ok_or(InstallError::Account)?,
    });
    // The same ordinary roots `Child::spawn` uses, except the service
    // workspace, which `service configure` checks once it names this file.
    let mut ordinary = layout
        .private_dirs()
        .iter()
        .map(PathBuf::as_path)
        .collect::<Vec<_>>();
    ordinary.push(layout.binary().parent().ok_or(InstallError::Layout)?);
    ordinary.push(layout.endpoint().parent().ok_or(InstallError::Layout)?);
    write_private(
        Path::new(&request.out),
        &bytes,
        (ids.daemon_uid, ids.daemon_gid),
        |path, staged| vetted_objects(path, staged, ids.daemon_uid, &ordinary, &access),
    )?;
    Ok(ids.daemon_uid)
}

fn resolve(
    r: &Request,
    resolver: &impl Resolver,
    group: impl Fn(&str) -> Result<u32, InstallError>,
) -> Result<Ids, InstallError> {
    let user = |name: &str, administrative: bool| {
        let account = resolver.account(name).map_err(|_| InstallError::Account)?;
        let uid = account
            .identity
            .parse::<u32>()
            .map_err(|_| InstallError::Account)?;
        if account.kind != AccountKind::User || (account.administrative && !administrative) {
            return Err(InstallError::Account);
        }
        Ok((uid, account.gid.ok_or(InstallError::Account)?))
    };
    // ADR 0047: the service worker account is fixed and never an administrator.
    let (daemon_uid, daemon_gid) = user(DAEMON_ACCOUNT, false)?;
    let (owner_uid, _) = user(&r.operator_account, true)?;
    let (agent_uid, _) = user(&r.agent_account, true)?;
    // The standard-rights build account never has administrator membership.
    let (build_uid, _) = user(&r.build_account, false)?;
    Ok(Ids {
        owner_uid,
        owner_gid: group(&r.control_group)?,
        daemon_uid,
        daemon_gid,
        agent_uid,
        build_uid,
    })
}

/// Name or canonical numeric GID of an existing local group.
fn group(value: &str) -> Result<u32, InstallError> {
    let numeric = value
        .parse::<u32>()
        .ok()
        .filter(|gid| gid.to_string() == value);
    let name = CString::new(value).map_err(|_| InstallError::Account)?;
    let mut record = std::mem::MaybeUninit::<libc::group>::uninit();
    let mut result = std::ptr::null_mut();
    let mut buffer = vec![0 as libc::c_char; 65536];
    // SAFETY: the output record, buffer and result pointer stay valid for the
    // whole lookup; name is NUL terminated.
    let status = unsafe {
        match numeric {
            Some(gid) => libc::getgrgid_r(
                gid,
                record.as_mut_ptr(),
                buffer.as_mut_ptr(),
                buffer.len(),
                &mut result,
            ),
            None => libc::getgrnam_r(
                name.as_ptr(),
                record.as_mut_ptr(),
                buffer.as_mut_ptr(),
                buffer.len(),
                &mut result,
            ),
        }
    };
    if status != 0 || result.is_null() {
        return Err(InstallError::Account);
    }
    // SAFETY: a successful lookup initialized the record.
    let gid = unsafe { record.assume_init() }.gr_gid;
    if numeric.is_some_and(|expected| expected != gid) {
        return Err(InstallError::Account);
    }
    Ok(gid)
}

fn founder_cluster() -> Result<[u8; 16], InstallError> {
    for _ in 0..4 {
        let mut id = [0; 16];
        getrandom::fill(&mut id).map_err(|_| InstallError::Io)?;
        if id != [0; 16] {
            return Ok(id);
        }
    }
    Err(InstallError::Io)
}

/// The runtime parser accepts the exact bytes, with its own endpoint and
/// owner-socket qualification.
fn parity(bytes: &[u8]) -> Result<Settings, InstallError> {
    let settings: Settings = serde_json::from_slice(bytes).map_err(|_| InstallError::Argument)?;
    if !cluster_owner::address(&settings.owner_socket) {
        return Err(InstallError::Argument);
    }
    let endpoints = [Some(&settings.peer), Some(&settings.enrollment)];
    if endpoints
        .into_iter()
        .chain([settings.issuer_peer.as_ref()])
        .flatten()
        .any(|address| peer_endpoint(address).is_none())
    {
        return Err(InstallError::Address);
    }
    Ok(settings)
}

fn roles(ids: &Ids) -> Result<(), InstallError> {
    cluster_owner::Roles {
        owner: ids.owner_uid,
        daemon: ids.daemon_uid,
        agent: ids.agent_uid,
        build: ids.build_uid,
        group: ids.owner_gid,
    }
    .qualify()
    .map_err(|_| InstallError::Account)
}

/// Mirrors `Settings::checked`: the peer address is on an up interface, and a
/// joiner's issuer lies inside one of that interface's prefixes.
fn reachable(settings: &Settings, local: &[(IpAddr, Route)]) -> bool {
    let Some(peer) = peer_endpoint(&settings.peer) else {
        return false;
    };
    let routes = local
        .iter()
        .filter(|(address, _)| *address == peer.ip())
        .map(|(_, route)| *route)
        .collect::<Vec<_>>();
    if routes.is_empty() {
        return false;
    }
    match &settings.issuer_peer {
        None => true,
        Some(issuer) => peer_endpoint(issuer)
            .is_some_and(|address| routes.iter().any(|route| covers(*route, address.ip()))),
    }
}

/// A filesystem object as `(device, inode)`.
type Object = (u64, u64);

/// The identities a layout check observed for the file and its directory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Vetted {
    file: Object,
    parent: Object,
}

/// A runtime identity: a UID with the GIDs it holds when it traverses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Identity {
    uid: u32,
    groups: Vec<u32>,
}

impl Identity {
    /// ADR 0047's native worker clears its supplementary groups, so the daemon
    /// traverses with its UID and primary GID only.
    pub(super) fn worker(uid: u32, gid: u32) -> Self {
        Self {
            uid,
            groups: vec![gid],
        }
    }

    /// Effective POSIX search permission on a directory from its owner, group
    /// and other bits. Root publication bypasses these bits, so they are
    /// checked for the identity that must traverse the path at run time.
    /// They decide alone only where `no_extended_acl` holds.
    fn searches(&self, metadata: &std::fs::Metadata) -> bool {
        let bit = if self.uid == 0 {
            return metadata.is_dir();
        } else if metadata.uid() == self.uid {
            0o100
        } else if self.groups.contains(&metadata.gid()) {
            0o010
        } else {
            0o001
        };
        metadata.is_dir() && metadata.mode() & bit != 0
    }
}

/// The pinned object carries no access or default POSIX ACL, so its owner,
/// group and other bits are authoritative: no named entry can deny what the
/// bits grant. The attributes are read through the descriptor's
/// `/proc/self/fd` link, which names exactly the pinned object and also works
/// for `O_PATH` and `O_TMPFILE` handles. ENODATA means no ACL; EOPNOTSUPP means
/// the filesystem cannot carry one. Any other result refuses.
fn no_extended_acl(file: &File) -> bool {
    let Ok(path) = CString::new(format!("/proc/self/fd/{}", file.as_raw_fd())) else {
        return false;
    };
    [c"system.posix_acl_access", c"system.posix_acl_default"]
        .iter()
        .all(|attribute| {
            // SAFETY: both strings are NUL terminated; a null, zero-length
            // buffer only queries the attribute size.
            let size = unsafe {
                libc::getxattr(path.as_ptr(), attribute.as_ptr(), std::ptr::null_mut(), 0)
            };
            size < 0
                && matches!(
                    std::io::Error::last_os_error().raw_os_error(),
                    Some(libc::ENODATA | libc::EOPNOTSUPP)
                )
        })
}

/// Every pinned component from `/` down to `path`'s parent, and `path`
/// itself when `inclusive`, carries no extended ACL and is searchable by
/// `who`.
fn chain_reachable(pass: &super::LayoutPass, path: &Path, inclusive: bool, who: &Identity) -> bool {
    let start = if inclusive { Some(path) } else { path.parent() };
    start.is_some_and(|dir| {
        dir.ancestors().all(|component| {
            pass.0
                .get(component)
                .is_some_and(|(file, metadata)| who.searches(metadata) && no_extended_acl(file))
        })
    })
}

/// The runtime identities whose access the layout must grant.
pub(super) struct Access {
    daemon: Identity,
    owner: Option<Identity>,
}

impl Access {
    /// The native worker's view, used by `Child::qualify`: the daemon with its
    /// primary GID only. The operator is checked at publication.
    pub(super) fn worker(uid: u32, gid: u32) -> Self {
        Self {
            daemon: Identity::worker(uid, gid),
            owner: None,
        }
    }

    /// The daemon reaches the configuration file, every private directory and
    /// the control directory; the operator, when given, reaches the control
    /// directory. Every component on those chains, and the file itself,
    /// carries no extended ACL.
    pub(super) fn permits(
        &self,
        pass: &super::LayoutPass,
        settings: &Settings,
        config: &Path,
    ) -> bool {
        let Some(control) = settings.owner_socket.parent() else {
            return false;
        };
        pass.0
            .get(config)
            .is_some_and(|(file, _)| no_extended_acl(file))
            && chain_reachable(pass, config, false, &self.daemon)
            && [
                &settings.workspace,
                &settings.state,
                &settings.credentials,
                &settings.wrap,
                &settings.custody,
                &settings.history,
            ]
            .iter()
            .all(|dir| chain_reachable(pass, dir, true, &self.daemon))
            && chain_reachable(pass, control, true, &self.daemon)
            && self
                .owner
                .as_ref()
                .is_none_or(|owner| chain_reachable(pass, control, true, owner))
    }
}

/// Runs the runtime loader on `path` (or on the unnamed `staged` file adopted
/// at `path`), requires the runtime identities' access, and reports the
/// identities the loader pinned.
fn vetted_objects(
    path: &Path,
    staged: Option<&File>,
    uid: u32,
    ordinary: &[&Path],
    access: &Access,
) -> Option<Vetted> {
    let staged = match staged {
        Some(file) => {
            let mut reader = file.try_clone().ok()?;
            reader.rewind().ok()?;
            Some(reader)
        }
        None => None,
    };
    let (settings, _, pass) = Settings::vetted_layout(path, staged, uid, ordinary, None).ok()?;
    if !access.permits(&pass, &settings, path) {
        return None;
    }
    Some(Vetted {
        file: object(&pass.0.get(path)?.1),
        parent: object(&pass.0.get(path.parent()?)?.1),
    })
}

/// ADR 0046 owner-route qualification, including the actual POSIX ACL
/// attributes of every owner-socket ancestor, before publication.
fn socket_route(settings: &Settings) -> Result<(), InstallError> {
    cluster_owner::route_qualified(
        &settings.owner_socket,
        settings.owner_uid,
        settings.daemon_uid,
        settings.owner_gid,
    )
    .then_some(())
    .ok_or(InstallError::Layout)
}

fn identity(file: &File) -> Result<Object, InstallError> {
    file.metadata()
        .map(|metadata| object(&metadata))
        .map_err(|_| InstallError::Io)
}

/// The object a single name in `dir` refers to, without following a symlink.
fn entry(dir: &File, name: &CString) -> Result<Option<Object>, InstallError> {
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: dir is live, name is NUL terminated and stat is writable storage.
    let rc = unsafe {
        libc::fstatat(
            dir.as_raw_fd(),
            name.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if rc != 0 {
        return match std::io::Error::last_os_error().raw_os_error() {
            Some(libc::ENOENT) => Ok(None),
            _ => Err(InstallError::Io),
        };
    }
    // SAFETY: a successful fstatat initialized stat.
    let stat = unsafe { stat.assume_init() };
    Ok(Some((stat.st_dev, stat.st_ino)))
}

/// An ancestor component: owned by root or the daemon, and not writable by
/// group or others unless root-owned and sticky (the native startup rule).
fn qualify_ancestor(dir: &File, daemon: u32) -> Result<(), InstallError> {
    let metadata = dir.metadata().map_err(|_| InstallError::Layout)?;
    let mode = metadata.mode();
    if !metadata.is_dir()
        || ![0, daemon].contains(&metadata.uid())
        || (mode & 0o022 != 0 && !(metadata.uid() == 0 && mode & 0o1000 != 0))
    {
        return Err(InstallError::Layout);
    }
    Ok(())
}

/// The output directory: owned by the publishing caller (root in production)
/// and writable by nobody else, with no sticky exception. Only the caller can
/// then create, rename or remove names in it, so the single link and any
/// rollback cannot race another writer.
fn qualify_parent(dir: &File, caller: u32) -> Result<(), InstallError> {
    let metadata = dir.metadata().map_err(|_| InstallError::Layout)?;
    if !metadata.is_dir() || metadata.uid() != caller || metadata.mode() & 0o022 != 0 {
        return Err(InstallError::Layout);
    }
    Ok(())
}

/// Opens `parent` one component at a time from `/`, never following a
/// symlink, and qualifies every component before anything is created.
fn open_parent(parent: &Path, daemon: u32, caller: u32) -> Result<File, InstallError> {
    let mut components = parent.components();
    if components.next() != Some(Component::RootDir) {
        return Err(InstallError::Argument);
    }
    let names = components
        .map(|component| match component {
            Component::Normal(name) => CString::new(name.as_bytes()).ok(),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()
        .ok_or(InstallError::Argument)?;
    if names.is_empty() {
        return Err(InstallError::Argument);
    }
    let mut dir = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open("/")
        .map_err(|_| InstallError::Layout)?;
    qualify_ancestor(&dir, daemon)?;
    for (index, name) in names.iter().enumerate() {
        let last = index + 1 == names.len();
        // Intermediate components are path handles; the output directory is
        // opened for reading so it can stage, link and sync.
        let access = if last { libc::O_RDONLY } else { libc::O_PATH };
        // SAFETY: dir is a live directory descriptor and name is one
        // NUL-terminated component. The returned descriptor is owned.
        let fd = unsafe {
            libc::openat(
                dir.as_raw_fd(),
                name.as_ptr(),
                access | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd == -1 {
            return Err(InstallError::Layout);
        }
        // SAFETY: openat succeeded and returned a new descriptor.
        dir = unsafe { File::from_raw_fd(fd) };
        if last {
            qualify_parent(&dir, caller)?;
        } else {
            qualify_ancestor(&dir, daemon)?;
        }
    }
    Ok(dir)
}

/// Creates `out` with mode 0600 owned by `owner`, never replacing an entry.
///
/// The parent is walked and qualified before anything is created, and only
/// the caller can change names in it. The content is staged as an unnamed
/// `O_TMPFILE` inode in that directory, so no temporary name ever exists.
/// `vet` first checks the staged file adopted at `out`, then the published
/// name, and must report exactly the staged file's and the directory
/// descriptor's identities each time. A single `linkat` through
/// `/proc/self/fd` publishes that inode and fails with `EEXIST` rather than
/// replace anything. A failure before the link leaves nothing to remove.
fn write_private(
    out: &Path,
    bytes: &[u8],
    owner: (u32, u32),
    mut vet: impl FnMut(&Path, Option<&File>) -> Option<Vetted>,
) -> Result<(), InstallError> {
    let parent = out.parent().ok_or(InstallError::Argument)?;
    let name = CString::new(out.file_name().ok_or(InstallError::Argument)?.as_bytes())
        .map_err(|_| InstallError::Argument)?;
    // SAFETY: geteuid takes no pointers.
    let caller = unsafe { libc::geteuid() };
    let dir = open_parent(parent, owner.0, caller)?;
    let parent_object = identity(&dir)?;
    if entry(&dir, &name)?.is_some() {
        return Err(InstallError::Exists);
    }
    // SAFETY: dir is live and "." names that directory.
    let fd = unsafe {
        libc::openat(
            dir.as_raw_fd(),
            c".".as_ptr(),
            libc::O_TMPFILE | libc::O_RDWR | libc::O_CLOEXEC,
            0o600 as libc::c_uint,
        )
    };
    if fd == -1 {
        // No O_TMPFILE support on this filesystem: refuse rather than stage
        // under a mutable name.
        return Err(InstallError::Io);
    }
    // SAFETY: openat returned a new descriptor owned here.
    let mut file = unsafe { File::from_raw_fd(fd) };
    file.write_all(bytes).map_err(|_| InstallError::Io)?;
    // SAFETY: fd is the live descriptor owned by file.
    if unsafe { libc::fchown(fd, owner.0, owner.1) } != 0 || unsafe { libc::fchmod(fd, 0o600) } != 0
    {
        return Err(InstallError::Io);
    }
    file.sync_all().map_err(|_| InstallError::Io)?;
    let expected = Vetted {
        file: identity(&file)?,
        parent: parent_object,
    };
    if vet(out, Some(&file)) != Some(expected) {
        return Err(InstallError::Layout);
    }
    let source = CString::new(format!("/proc/self/fd/{fd}")).map_err(|_| InstallError::Io)?;
    // SAFETY: source names this process's live staged descriptor, dir is the
    // qualified directory and name is one NUL-terminated component.
    let rc = unsafe {
        libc::linkat(
            libc::AT_FDCWD,
            source.as_ptr(),
            dir.as_raw_fd(),
            name.as_ptr(),
            libc::AT_SYMLINK_FOLLOW,
        )
    };
    if rc != 0 {
        let error = std::io::Error::last_os_error();
        return Err(if error.raw_os_error() == Some(libc::EEXIST) {
            InstallError::Exists
        } else {
            InstallError::Io
        });
    }
    let refusal = if entry(&dir, &name)? != Some(expected.file) || dir.sync_all().is_err() {
        Some(InstallError::Io)
    } else if vet(out, None) != Some(expected) {
        Some(InstallError::Layout)
    } else {
        None
    };
    if let Some(error) = refusal {
        // Only the caller can change names in the qualified directory, so the
        // name still refers to the linked file when it is removed.
        if entry(&dir, &name).ok().flatten() == Some(expected.file) {
            // SAFETY: dir is live and name is a single NUL-terminated component.
            unsafe { libc::unlinkat(dir.as_raw_fd(), name.as_ptr(), 0) };
        }
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
