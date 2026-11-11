#[cfg(target_os = "linux")]
use super::super::config::AccountKind;
use super::*;
use std::{
    ffi::CString,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::MetadataExt,
    },
};
#[cfg(target_os = "linux")]
pub const PLATFORM: super::super::layout::Platform = super::super::layout::Platform::Linux;
#[cfg(target_os = "macos")]
pub const PLATFORM: super::super::layout::Platform = super::super::layout::Platform::Macos;

pub(in crate::service) fn local(file: &File) -> Result<(), Error> {
    let mut stat = std::mem::MaybeUninit::<libc::statfs>::uninit();
    if unsafe { libc::fstatfs(file.as_raw_fd(), stat.as_mut_ptr()) } != 0 {
        return Err(Error::Layout);
    }
    let stat = unsafe { stat.assume_init() };
    #[cfg(target_os = "linux")]
    let ok = matches!(
        stat.f_type as u64,
        0xef53 | 0x58465342 | 0x9123683e | 0x01021994 | 0xf2f52010
    );
    #[cfg(target_os = "macos")]
    let ok = stat.f_flags & libc::MNT_LOCAL as u32 != 0;
    if ok { Ok(()) } else { Err(Error::Layout) }
}
pub fn protection(file: &File) -> Result<Protection, Error> {
    // Mode bits cannot describe additional ACL grants. Refuse ACLs rather than
    // interpreting a private mode as sufficient evidence.
    #[cfg(target_os = "linux")]
    {
        let n = unsafe {
            libc::fgetxattr(
                file.as_raw_fd(),
                c"system.posix_acl_access".as_ptr(),
                std::ptr::null_mut(),
                0,
            )
        };
        if n >= 0 || std::io::Error::last_os_error().raw_os_error() != Some(libc::ENODATA) {
            return Err(Error::Permissions);
        }
    }
    #[cfg(target_os = "macos")]
    super::macos::empty_acl(file.as_raw_fd())?;
    let m = file.metadata().map_err(|_| Error::Permissions)?;
    Ok(Protection::Unix {
        uid: m.uid(),
        gid: m.gid(),
        mode: m.mode() & 0o7777,
    })
}
pub fn open(path: &Path, directory: bool, _read: bool, trusted: bool) -> Result<Observed, Error> {
    let parts = components(path, false)?;
    let fd = unsafe {
        libc::open(
            c"/".as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if fd < 0 {
        return Err(Error::Layout);
    }
    let mut file = unsafe { File::from_raw_fd(fd) };
    let mut ancestors = Vec::new();
    for (i, part) in parts.iter().enumerate() {
        local(&file)?;
        if trusted {
            super::super::layout::check_unix_ancestor(PLATFORM, &protection(&file)?)?;
        }
        let name = CString::new(part.as_str()).map_err(|_| Error::Layout)?;
        let dir = i + 1 < parts.len() || directory;
        let flags = libc::O_RDONLY
            | libc::O_CLOEXEC
            | libc::O_NOFOLLOW
            | libc::O_NONBLOCK
            | if dir { libc::O_DIRECTORY } else { 0 };
        let fd = unsafe { libc::openat(file.as_raw_fd(), name.as_ptr(), flags) };
        if fd < 0 {
            return Err(Error::Layout);
        }
        ancestors.push(file);
        file = unsafe { File::from_raw_fd(fd) };
    }
    local(&file)?;
    let m = file.metadata().map_err(|_| Error::Layout)?;
    let expected_type = if directory {
        m.is_dir()
    } else {
        m.is_file() && m.nlink() == 1
    };
    if !expected_type {
        return Err(Error::Layout);
    }
    let p = if trusted {
        protection(&file)?
    } else {
        Protection::Unix {
            uid: m.uid(),
            gid: m.gid(),
            mode: m.mode() & 0o7777,
        }
    };
    Ok(Observed {
        file,
        _ancestors: ancestors,
        protection: p,
    })
}

// The parent is already protected and pinned. Only the final missing directory
// is created. A racing EEXIST follows the exact same no-repair check.
#[cfg(any(target_os = "macos", test))]
fn endpoint_child(
    mut parent: Observed,
    name: &std::ffi::CStr,
    #[cfg(test)] owner: (u32, u32),
) -> Result<Observed, Error> {
    let created = unsafe { libc::mkdirat(parent.file.as_raw_fd(), name.as_ptr(), 0o755) } == 0;
    if !created && std::io::Error::last_os_error().raw_os_error() != Some(libc::EEXIST) {
        return Err(Error::Permissions);
    }
    let raw = unsafe {
        libc::openat(
            parent.file.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if raw < 0 {
        return Err(Error::Permissions);
    }
    let file = unsafe { File::from_raw_fd(raw) };
    local(&file)?;
    protection(&file)?; // ACL must be known empty before any mode/owner operation.
    #[cfg(not(test))]
    let owner = (0, 0);
    if created {
        if unsafe { libc::fchown(file.as_raw_fd(), owner.0, owner.1) } != 0
            || unsafe { libc::fchmod(file.as_raw_fd(), 0o755) } != 0
        {
            return Err(Error::Permissions);
        }
        file.sync_all().map_err(|_| Error::Permissions)?;
        parent.file.sync_all().map_err(|_| Error::Permissions)?;
    }
    let facts = protection(&file)?;
    #[cfg(test)]
    let facts = match facts {
        Protection::Unix { uid, gid, mode } => Protection::Unix {
            uid: if uid == owner.0 { 0 } else { 1 },
            gid: if gid == owner.1 { 0 } else { 1 },
            mode,
        },
        other => other,
    };
    check_protection(PLATFORM, Policy::EndpointDirectory, true, &facts)?;
    parent._ancestors.push(parent.file);
    Ok(Observed {
        file,
        _ancestors: parent._ancestors,
        protection: facts,
    })
}
#[cfg(target_os = "macos")]
pub(super) fn ensure_endpoint_directory(layout: &Layout) -> Result<Observed, Error> {
    if layout.platform() != PLATFORM {
        return Err(Error::Layout);
    }
    let directory = layout.endpoint().parent().ok_or(Error::Layout)?;
    let parent = open(directory.parent().ok_or(Error::Layout)?, true, false, true)?;
    super::super::layout::check_unix_ancestor(PLATFORM, parent.protection())?;
    let name = CString::new(
        directory
            .file_name()
            .ok_or(Error::Layout)?
            .as_encoded_bytes(),
    )
    .map_err(|_| Error::Layout)?;
    endpoint_child(
        parent,
        &name,
        #[cfg(test)]
        (0, 0),
    )
}

// Linux retains the conservative protected local-files database requirement.
// getgrouplist still observes current supplementary membership (bounded).
#[cfg(target_os = "linux")]
pub fn account(value: &str) -> Result<Account, Error> {
    let mut passwd =
        open(Path::new("/etc/passwd"), false, true, true).map_err(|_| Error::Identity)?;
    check_protection(PLATFORM, Policy::BinaryIntegrity, true, &passwd.protection)
        .map_err(|_| Error::Identity)?;
    let mut data = Vec::new();
    passwd
        .file
        .by_ref()
        .take(1024 * 1024 + 1)
        .read_to_end(&mut data)
        .map_err(|_| Error::Identity)?;
    if data.len() > 1024 * 1024 {
        return Err(Error::Identity);
    }
    let text = std::str::from_utf8(&data).map_err(|_| Error::Identity)?;
    let mut found = None;
    for line in text.lines() {
        let p: Vec<_> = line.split(':').collect();
        if p.len() != 7 {
            return Err(Error::Identity);
        }
        if p[0] == value || p[2] == value {
            if found.is_some() {
                return Err(Error::Identity);
            }
            let uid = p[2].parse::<u32>().map_err(|_| Error::Identity)?;
            let gid = p[3].parse::<u32>().map_err(|_| Error::Identity)?;
            if uid < 1000 || p[6].ends_with("nologin") || p[6].ends_with("false") || p[6].is_empty()
            {
                return Err(Error::Identity);
            }
            found = Some((CString::new(p[0]).map_err(|_| Error::Identity)?, uid, gid));
        }
    }
    let (name, uid, gid) = found.ok_or(Error::Identity)?;
    // NSS must resolve the same user as the protected local database. Refuse
    // a directory-service shadow or an undersized/ambiguous native record.
    let mut record = std::mem::MaybeUninit::<libc::passwd>::uninit();
    let mut result = std::ptr::null_mut();
    let mut buffer = vec![0i8; 65536];
    if unsafe {
        libc::getpwnam_r(
            name.as_ptr(),
            record.as_mut_ptr(),
            buffer.as_mut_ptr(),
            buffer.len(),
            &mut result,
        )
    } != 0
        || result.is_null()
    {
        return Err(Error::Identity);
    }
    let record = unsafe { record.assume_init() };
    if record.pw_uid != uid || record.pw_gid != gid {
        return Err(Error::Identity);
    }
    #[cfg(target_os = "linux")]
    type GroupId = libc::gid_t;
    #[cfg(target_os = "macos")]
    type GroupId = i32;
    let primary = GroupId::try_from(gid).map_err(|_| Error::Identity)?;
    let mut groups = vec![0 as GroupId; 1024];
    let mut count = groups.len() as i32;
    if unsafe { libc::getgrouplist(name.as_ptr(), primary, groups.as_mut_ptr(), &mut count) } < 0
        || !(1..=1024).contains(&count)
    {
        return Err(Error::Identity);
    }
    groups.truncate(count as usize);
    let mut admin = groups.contains(&0);
    #[cfg(target_os = "linux")]
    let admin_groups: &[_] = &[c"sudo", c"wheel"];
    #[cfg(target_os = "macos")]
    let admin_groups: &[_] = &[c"admin"];
    for group in admin_groups {
        let mut record = std::mem::MaybeUninit::<libc::group>::uninit();
        let mut result = std::ptr::null_mut();
        let mut buf = vec![0i8; 65536];
        if unsafe {
            libc::getgrnam_r(
                group.as_ptr(),
                record.as_mut_ptr(),
                buf.as_mut_ptr(),
                buf.len(),
                &mut result,
            )
        } != 0
        {
            return Err(Error::Identity);
        }
        if !result.is_null() {
            admin |= groups.contains(
                &GroupId::try_from(unsafe { record.assume_init() }.gr_gid)
                    .map_err(|_| Error::Identity)?,
            );
        }
    }
    Ok(Account {
        identity: uid.to_string(),
        gid: Some(gid),
        kind: AccountKind::User,
        administrative: admin,
    })
}

#[cfg(target_os = "macos")]
pub use super::macos::account;

#[cfg(test)]
mod tests {
    use super::*;
    fn owned_parent(path: &Path) -> Observed {
        let file = File::open(path).unwrap();
        let facts = protection(&file).unwrap();
        Observed {
            file,
            _ancestors: vec![],
            protection: facts,
        }
    }
    #[test]
    fn endpoint_creation_is_exclusive_and_never_repairs() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        assert!(
            unsafe { libc::geteuid() } != 0,
            "ordinary-user fixtures only"
        );
        let dir = tempfile::tempdir().unwrap();
        let owner = unsafe { (libc::geteuid(), libc::getegid()) };
        let mut groups = [0 as libc::gid_t; 1024];
        let count = unsafe { libc::getgroups(groups.len() as i32, groups.as_mut_ptr()) };
        assert!(count >= 0, "fixture groups available");
        let other = groups[..count as usize]
            .iter()
            .copied()
            .find(|g| *g != owner.1)
            .expect("fixture requires another own member group");
        let parent = owned_parent(dir.path());
        assert!(unsafe { libc::fchown(parent.file.as_raw_fd(), owner.0, other) } == 0);
        let first = endpoint_child(owned_parent(dir.path()), c"run", owner).unwrap();
        let before = first.file.metadata().unwrap();
        assert!(
            before.uid() == owner.0 && before.gid() == owner.1 && before.mode() & 0o7777 == 0o755
        );
        let again = endpoint_child(owned_parent(dir.path()), c"run", owner).unwrap();
        assert!(again.file.metadata().unwrap().ino() == before.ino());
        assert!(unsafe { libc::fchown(first.file.as_raw_fd(), owner.0, other) } == 0);
        assert!(endpoint_child(owned_parent(dir.path()), c"run", owner).is_err());
        assert!(first.file.metadata().unwrap().gid() == other);
        assert!(unsafe { libc::fchown(first.file.as_raw_fd(), owner.0, owner.1) } == 0);
        std::fs::set_permissions(
            dir.path().join("run"),
            std::fs::Permissions::from_mode(0o775),
        )
        .unwrap();
        assert!(endpoint_child(owned_parent(dir.path()), c"run", owner).is_err());
        assert!(first.file.metadata().unwrap().mode() & 0o7777 == 0o775);
        std::fs::write(dir.path().join("file"), b"fixed").unwrap();
        assert!(endpoint_child(owned_parent(dir.path()), c"file", owner).is_err());
        symlink("run", dir.path().join("link")).unwrap();
        assert!(endpoint_child(owned_parent(dir.path()), c"link", owner).is_err());
    }
    #[test]
    fn root_and_unknown_local_accounts_refuse() {
        assert!(account("0").is_err());
        assert!(account("arxcovo-test-nonexistent").is_err());
    }
    #[test]
    fn linked_workspace_refuses() {
        let tmp = tempfile::tempdir().unwrap();
        let link = tmp.path().join("link");
        std::os::unix::fs::symlink(tmp.path(), &link).unwrap();
        assert!(open(&link, true, false, false).is_err());
    }
}
