//! Owner publication of the private installation file (ADR 0046, 0047).
//!
//! Root only. The bytes must pass the runtime's own schema, endpoint, role and
//! route checks, and the complete layout must pass `Settings::vetted` with the
//! installed service roots as ordinary roots, before a non-replacing atomic
//! publication. A refusal leaves no new file behind.
use super::{Route, Settings, cluster_owner, covers, interfaces};
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
        unix::{ffi::OsStrExt, fs::OpenOptionsExt},
    },
    path::{Path, PathBuf},
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
        |path| Settings::vetted(path, ids.daemon_uid, &ordinary, None).is_ok(),
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

struct Temporary<'a> {
    dir: &'a File,
    name: &'a CString,
    armed: bool,
}

impl Drop for Temporary<'_> {
    fn drop(&mut self) {
        if self.armed {
            // SAFETY: dir is a live directory descriptor and name a single
            // NUL-terminated component created by this process.
            unsafe { libc::unlinkat(self.dir.as_raw_fd(), self.name.as_ptr(), 0) };
        }
    }
}

fn exists(dir: &File, name: &CString) -> Result<bool, InstallError> {
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
    match rc {
        0 => Ok(true),
        _ if std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT) => Ok(false),
        _ => Err(InstallError::Io),
    }
}

/// Creates `out` with mode 0600 owned by `owner`, never replacing an existing
/// entry. `vet` sees the complete temporary file at its final directory, then
/// the published file; a refusal at either point removes it.
fn write_private(
    out: &Path,
    bytes: &[u8],
    owner: (u32, u32),
    mut vet: impl FnMut(&Path) -> bool,
) -> Result<(), InstallError> {
    let parent = out.parent().ok_or(InstallError::Argument)?;
    let name = CString::new(out.file_name().ok_or(InstallError::Argument)?.as_bytes())
        .map_err(|_| InstallError::Argument)?;
    // The final component is opened without following a symlink; the vetting
    // walk refuses a symlink anywhere in the path.
    let dir = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(parent)
        .map_err(|_| InstallError::Layout)?;
    if exists(&dir, &name)? {
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
    let mut guard = Temporary {
        dir: &dir,
        name: &temporary_name,
        armed: true,
    };
    // SAFETY: openat returned a new descriptor owned here.
    let mut file = unsafe { File::from_raw_fd(fd) };
    file.write_all(bytes).map_err(|_| InstallError::Io)?;
    // SAFETY: fd is the live descriptor owned by file.
    if unsafe { libc::fchown(fd, owner.0, owner.1) } != 0 || unsafe { libc::fchmod(fd, 0o600) } != 0
    {
        return Err(InstallError::Io);
    }
    file.sync_all().map_err(|_| InstallError::Io)?;
    drop(file);
    if !vet(&parent.join(&temporary)) {
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
    let durable = dir.sync_all().is_ok();
    if !durable || !vet(out) {
        // SAFETY: dir is live; name is the component this call just created.
        unsafe { libc::unlinkat(dir.as_raw_fd(), name.as_ptr(), 0) };
        return Err(if durable {
            InstallError::Layout
        } else {
            InstallError::Io
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests;
