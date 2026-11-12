use super::{ClientError, ClientHandle, PostCreateOps};
use std::{
    ffi::CString,
    fs::File,
    io::{self, Write},
    mem::MaybeUninit,
    os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd},
};
fn cv(value: i32) -> io::Result<()> {
    if value == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
fn stat(fd: ClientHandle<'_>) -> io::Result<libc::stat> {
    let mut s = MaybeUninit::uninit();
    cv(unsafe { libc::fstat(fd.as_raw_fd(), s.as_mut_ptr()) })?;
    Ok(unsafe { s.assume_init() })
}
pub(super) fn elevated() -> Result<bool, ClientError> {
    Ok(unsafe {
        elevated_ids(
            libc::getuid(),
            libc::geteuid(),
            libc::getgid(),
            libc::getegid(),
        )
    })
}
pub(super) fn elevated_ids(
    uid: libc::uid_t,
    euid: libc::uid_t,
    gid: libc::gid_t,
    egid: libc::gid_t,
) -> bool {
    uid == 0 || euid == 0 || uid != euid || gid != egid
}
fn open_at(
    fd: ClientHandle<'_>,
    name: &CString,
    flags: i32,
    mode: libc::mode_t,
) -> io::Result<OwnedFd> {
    let raw = unsafe {
        libc::openat(
            fd.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            libc::c_uint::from(mode),
        )
    };
    if raw == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(unsafe { OwnedFd::from_raw_fd(raw) })
    }
}
pub(super) struct Parent {
    pins: Vec<OwnedFd>,
    name: CString,
}
impl Parent {
    pub(super) fn open(out: &str) -> Result<Self, ClientError> {
        let mut names: Vec<_> = out[1..].split('/').collect();
        let name = CString::new(names.pop().ok_or(ClientError::Argument)?)
            .map_err(|_| ClientError::Argument)?;
        let root = unsafe {
            libc::open(
                c"/".as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if root == -1 {
            return Err(ClientError::Parent);
        }
        let mut pins = vec![unsafe { OwnedFd::from_raw_fd(root) }];
        for (i, part) in names.iter().enumerate() {
            let c = CString::new(*part).map_err(|_| ClientError::Argument)?;
            let fd = pins.last().unwrap().as_fd();
            let opened = open_at(fd, &c, libc::O_RDONLY | libc::O_DIRECTORY, 0);
            let (next, created) = match opened {
                Ok(f) => (f, false),
                Err(e) if e.kind() == io::ErrorKind::NotFound && i + 1 == names.len() => {
                    let result = unsafe { libc::mkdirat(fd.as_raw_fd(), c.as_ptr(), 0o700) };
                    let made = result == 0;
                    if !made && io::Error::last_os_error().raw_os_error() != Some(libc::EEXIST) {
                        return Err(ClientError::Parent);
                    }
                    (
                        open_at(fd, &c, libc::O_RDONLY | libc::O_DIRECTORY, 0)
                            .map_err(|_| ClientError::Parent)?,
                        made,
                    )
                }
                Err(_) => return Err(ClientError::Parent),
            };
            if i + 1 == names.len() {
                private_directory(next.as_fd()).map_err(|_| ClientError::Parent)?;
                if created {
                    cv(unsafe { libc::fchmod(next.as_raw_fd(), 0o700) })
                        .map_err(|_| ClientError::Parent)?;
                }
            }
            pins.push(next);
        }
        // Root as immediate parent is never private for a non-root caller.
        private_directory(pins.last().unwrap().as_fd()).map_err(|_| ClientError::Parent)?;
        Ok(Self { pins, name })
    }
    pub(super) fn handle(&self) -> ClientHandle<'_> {
        self.pins.last().unwrap().as_fd()
    }
    pub(super) fn create(&self) -> Result<File, ClientError> {
        open_at(
            self.handle(),
            &self.name,
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
            0o600,
        )
        .map(File::from)
        .map_err(|e| {
            if e.raw_os_error() == Some(libc::EEXIST) {
                ClientError::Exists
            } else {
                ClientError::Io
            }
        })
    }
}
fn private_directory(fd: ClientHandle<'_>) -> io::Result<()> {
    let s = stat(fd)?;
    if s.st_mode & libc::S_IFMT != libc::S_IFDIR
        || s.st_uid != unsafe { libc::geteuid() }
        || s.st_mode & 0o077 != 0
    {
        return Err(io::Error::other("private directory"));
    }
    Ok(())
}
pub(super) struct NativeFileOps {
    uid: libc::uid_t,
}
impl NativeFileOps {
    pub(super) fn new() -> io::Result<Self> {
        Ok(Self {
            uid: unsafe { libc::geteuid() },
        })
    }
    pub(super) fn verify_with(
        &self,
        facts: impl FnOnce() -> io::Result<libc::stat>,
        expected: u64,
    ) -> io::Result<()> {
        let s = facts()?;
        if s.st_mode & libc::S_IFMT != libc::S_IFREG
            || s.st_uid != self.uid
            || s.st_nlink != 1
            || s.st_mode & 0o777 != 0o600
            || s.st_size < 0
            || s.st_size as u64 != expected
        {
            return Err(io::Error::other("private file"));
        }
        Ok(())
    }
}
impl PostCreateOps for NativeFileOps {
    fn mode(&mut self, f: ClientHandle<'_>) -> io::Result<()> {
        cv(unsafe { libc::fchmod(f.as_raw_fd(), 0o600) })
    }
    fn write(&mut self, f: ClientHandle<'_>, json: &[u8]) -> io::Result<()> {
        let mut file = std::mem::ManuallyDrop::new(unsafe { File::from_raw_fd(f.as_raw_fd()) });
        file.write_all(json)
    }
    fn flush(&mut self, f: ClientHandle<'_>) -> io::Result<()> {
        cv(unsafe { libc::fsync(f.as_raw_fd()) })
    }
    fn sync_parent(&mut self, f: ClientHandle<'_>) -> io::Result<()> {
        cv(unsafe { libc::fsync(f.as_raw_fd()) })
    }
    fn verify(&mut self, f: ClientHandle<'_>, expected: u64) -> io::Result<()> {
        self.verify_with(|| stat(f), expected)
    }
}

#[cfg(test)]
#[test]
fn native_mode_sync_conversion() {
    assert!(cv(-1).is_err(), "failed native return");
    assert!(cv(0).is_ok(), "successful native return");
}
