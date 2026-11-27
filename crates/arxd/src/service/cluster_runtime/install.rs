//! Owner publication of the private installation file (ADR 0046, 0047).
//!
//! Root only. The bytes must pass the runtime's own schema, endpoint, role and
//! route checks, the owner-socket route must pass the ADR 0046 owner, mode and
//! ACL policy, and the complete layout must pass `Settings::vetted` with the
//! installed service roots as ordinary roots, before a non-replacing atomic
//! publication bound to the qualified directory and file descriptors. A
//! refusal leaves no new file behind.
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
    io::Write,
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
        |path| vetted_objects(path, ids.daemon_uid, &ordinary),
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

/// Runs the runtime loader on `path` and reports the identities it pinned.
fn vetted_objects(path: &Path, uid: u32, ordinary: &[&Path]) -> Option<Vetted> {
    let (_, _, pass) = Settings::vetted_pass(path, uid, ordinary, None).ok()?;
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

/// Removes `name` from `dir` only while it is still `expected`.
fn remove_if(dir: &File, name: &CString, expected: Object) {
    if entry(dir, name).ok().flatten() == Some(expected) {
        // SAFETY: dir is live and name is a single NUL-terminated component.
        unsafe { libc::unlinkat(dir.as_raw_fd(), name.as_ptr(), 0) };
    }
}

/// One directory component: owned by root or the daemon, and not writable by
/// group or others unless root-owned and sticky (the native startup rule).
fn qualify_directory(dir: &File, daemon: u32) -> Result<(), InstallError> {
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

/// Opens `parent` one component at a time from `/`, never following a
/// symlink, and qualifies every component before anything is created.
fn open_parent(parent: &Path, daemon: u32) -> Result<File, InstallError> {
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
    let mut dir = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open("/")
        .map_err(|_| InstallError::Layout)?;
    qualify_directory(&dir, daemon)?;
    for (index, name) in names.iter().enumerate() {
        // Intermediate components are path handles; the final directory is
        // opened for reading so it can be synced after publication.
        let access = if index + 1 == names.len() {
            libc::O_RDONLY
        } else {
            libc::O_PATH
        };
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
        qualify_directory(&dir, daemon)?;
    }
    Ok(dir)
}

/// Creates `out` with mode 0600 owned by `owner`, never replacing an entry.
///
/// The parent is walked and qualified before anything is created. Its
/// descriptor and the created file's descriptor stay open throughout. `vet`
/// runs on the complete temporary file, then on the published name, and must
/// report exactly those two descriptors' identities each time. Rename and
/// rollback act through the qualified directory descriptor, and rollback
/// removes a name only while it still refers to the created file.
fn write_private(
    out: &Path,
    bytes: &[u8],
    owner: (u32, u32),
    mut vet: impl FnMut(&Path) -> Option<Vetted>,
) -> Result<(), InstallError> {
    let parent = out.parent().ok_or(InstallError::Argument)?;
    let name = CString::new(out.file_name().ok_or(InstallError::Argument)?.as_bytes())
        .map_err(|_| InstallError::Argument)?;
    let dir = open_parent(parent, owner.0)?;
    let parent_object = identity(&dir)?;
    if entry(&dir, &name)?.is_some() {
        return Err(InstallError::Exists);
    }
    let mut random = [0u8; 8];
    getrandom::fill(&mut random).map_err(|_| InstallError::Io)?;
    let temporary = format!(".arxd-cluster-config.{:016x}", u64::from_be_bytes(random));
    let temporary_name = CString::new(temporary.as_bytes()).map_err(|_| InstallError::Io)?;
    // SAFETY: dir is live and temporary_name is one NUL-terminated component.
    let fd = unsafe {
        libc::openat(
            dir.as_raw_fd(),
            temporary_name.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600 as libc::c_uint,
        )
    };
    if fd == -1 {
        return Err(InstallError::Io);
    }
    // SAFETY: openat returned a new descriptor owned here.
    let mut file = unsafe { File::from_raw_fd(fd) };
    let file_object = match identity(&file) {
        Ok(object) => object,
        Err(error) => {
            // SAFETY: O_CREAT|O_EXCL just created this name in dir.
            unsafe { libc::unlinkat(dir.as_raw_fd(), temporary_name.as_ptr(), 0) };
            return Err(error);
        }
    };
    let mut guard = Temporary {
        dir: &dir,
        name: &temporary_name,
        object: file_object,
        armed: true,
    };
    file.write_all(bytes).map_err(|_| InstallError::Io)?;
    // SAFETY: fd is the live descriptor owned by file.
    if unsafe { libc::fchown(fd, owner.0, owner.1) } != 0 || unsafe { libc::fchmod(fd, 0o600) } != 0
    {
        return Err(InstallError::Io);
    }
    file.sync_all().map_err(|_| InstallError::Io)?;
    let expected = Vetted {
        file: file_object,
        parent: parent_object,
    };
    if vet(&parent.join(&temporary)) != Some(expected) {
        return Err(InstallError::Layout);
    }
    // SAFETY: both names are single NUL-terminated components in dir.
    let rc = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            dir.as_raw_fd(),
            temporary_name.as_ptr(),
            dir.as_raw_fd(),
            name.as_ptr(),
            libc::RENAME_NOREPLACE,
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
    guard.armed = false;
    drop(guard);
    let refusal = if entry(&dir, &name)? != Some(file_object) {
        // Another writer replaced the published name; it is not ours to remove.
        return Err(InstallError::Io);
    } else if dir.sync_all().is_err() {
        Some(InstallError::Io)
    } else if vet(out) != Some(expected) {
        Some(InstallError::Layout)
    } else {
        None
    };
    if let Some(error) = refusal {
        remove_if(&dir, &name, file_object);
        return Err(error);
    }
    drop(file);
    Ok(())
}

struct Temporary<'a> {
    dir: &'a File,
    name: &'a CString,
    object: Object,
    armed: bool,
}

impl Drop for Temporary<'_> {
    fn drop(&mut self) {
        if self.armed {
            remove_if(self.dir, self.name, self.object);
        }
    }
}

#[cfg(test)]
mod tests;
