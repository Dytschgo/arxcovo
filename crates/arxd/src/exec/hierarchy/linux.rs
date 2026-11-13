//! Inactive Linux descriptor adapter. No service wiring or provisioning.
use super::setup::{
    CGROUP_MAGIC, CgroupFs, Created, FsError, FsResult, PROC_MAGIC, ProcFile, RawEntry, Stat,
};
use std::{
    ffi::{CStr, CString},
    fs::File,
    io::{self, Read},
    mem::MaybeUninit,
    os::fd::{AsRawFd, FromRawFd, IntoRawFd, OwnedFd},
};

const TEXT_MAX: usize = 4096;
const STATUS_MAX: usize = 8192;
const RECORD_MAX: usize = 512;
const DIR_FLAGS: i32 = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;
// NONBLOCK prevents a substituted FIFO from hanging before fstat rejects it.
const FILE_FLAGS: i32 = libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK;
pub(crate) struct Dir(OwnedFd);
pub(crate) struct LinuxFs {
    uid: u32,
    proc_magic: u64,
    cgroup_magic: u64,
    #[cfg(test)]
    owned: bool,
    #[cfg(test)]
    observer: Option<fn(i32, i32)>,
}
fn error(e: io::Error) -> FsError {
    match e.raw_os_error() {
        Some(libc::ENOENT) => FsError::Missing,
        Some(libc::EBUSY) => FsError::Busy,
        Some(libc::EFBIG | libc::ERANGE) => FsError::TooBig,
        Some(libc::EINVAL) => FsError::Invalid,
        Some(libc::ELOOP | libc::ENOTDIR | libc::EACCES | libc::EPERM) => FsError::Refused,
        _ => FsError::Io,
    }
}
fn last() -> FsError {
    error(io::Error::last_os_error())
}
fn component(name: &[u8]) -> FsResult<CString> {
    if name.is_empty()
        || name.len() > 255
        || name == b"."
        || name == b".."
        || name.iter().any(|b| b"/\\\0".contains(b))
    {
        return Err(FsError::Invalid);
    }
    CString::new(name).map_err(|_| FsError::Invalid)
}
fn open(parent: i32, name: &CStr, flags: i32) -> FsResult<OwnedFd> {
    let fd = unsafe { libc::openat(parent, name.as_ptr(), flags) };
    if fd < 0 {
        return Err(last());
    }
    // Ownership transfers exactly once, including all subsequent error paths.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}
fn facts(fd: i32) -> FsResult<Stat> {
    let mut st = MaybeUninit::<libc::stat>::uninit();
    let mut fs = MaybeUninit::<libc::statfs>::uninit();
    if unsafe { libc::fstat(fd, st.as_mut_ptr()) } != 0 {
        return Err(last());
    }
    if unsafe { libc::fstatfs(fd, fs.as_mut_ptr()) } != 0 {
        return Err(last());
    }
    let st = unsafe { st.assume_init() };
    let fs = unsafe { fs.assume_init() };
    Ok(Stat {
        uid: st.st_uid,
        mode: st.st_mode,
        magic: fs.f_type as u64,
    })
}
fn close(fd: OwnedFd) -> FsResult<()> {
    // Linux releases the fd even on EINTR. Never retry close on a reused number.
    if unsafe { libc::close(fd.into_raw_fd()) } == 0 {
        Ok(())
    } else {
        Err(last())
    }
}
fn text(fd: OwnedFd, max: usize) -> FsResult<Vec<u8>> {
    let mut file = File::from(fd);
    let mut bytes = Vec::new();
    let result = (&mut file)
        .take((max + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(error);
    let closed = close(file.into());
    result?;
    closed?;
    if bytes.len() > max {
        return Err(FsError::TooBig);
    }
    Ok(bytes)
}
impl LinuxFs {
    pub(crate) fn system() -> Self {
        Self {
            uid: 0,
            proc_magic: PROC_MAGIC,
            cgroup_magic: CGROUP_MAGIC,
            #[cfg(test)]
            owned: false,
            #[cfg(test)]
            observer: None,
        }
    }
    #[cfg(test)]
    fn owned(uid: u32) -> Self {
        Self {
            uid,
            owned: true,
            ..Self::system()
        }
    }
    fn bypass(&self) -> bool {
        #[cfg(test)]
        {
            self.owned
        }
        #[cfg(not(test))]
        {
            false
        }
    }
    fn trust(&self, fd: i32, kind: u32, magic: Option<u64>) -> FsResult<Stat> {
        let st = facts(fd)?;
        if st.mode & libc::S_IFMT != kind
            || st.mode & 0o022 != 0
            || (!self.bypass() && (st.uid != self.uid || magic.is_some_and(|m| st.magic != m)))
        {
            return Err(FsError::Refused);
        }
        Ok(st)
    }
    fn root(&self) -> FsResult<Dir> {
        let fd = open(libc::AT_FDCWD, c"/", DIR_FLAGS)?;
        self.trust(fd.as_raw_fd(), libc::S_IFDIR, None)?;
        Ok(Dir(fd))
    }
    fn directory(&self, parent: &Dir, name: &CStr, magic: Option<u64>) -> FsResult<Dir> {
        let fd = open(parent.0.as_raw_fd(), name, DIR_FLAGS)?;
        self.trust(fd.as_raw_fd(), libc::S_IFDIR, magic)?;
        Ok(Dir(fd))
    }
    fn control(&self, dir: &Dir, file: &str, write: bool) -> FsResult<OwnedFd> {
        self.trust(dir.0.as_raw_fd(), libc::S_IFDIR, Some(self.cgroup_magic))?;
        let fd = open(
            dir.0.as_raw_fd(),
            &component(file.as_bytes())?,
            FILE_FLAGS
                | if write {
                    libc::O_WRONLY
                } else {
                    libc::O_RDONLY
                },
        )?;
        self.trust(fd.as_raw_fd(), libc::S_IFREG, Some(self.cgroup_magic))?;
        Ok(fd)
    }
    fn proc_file(&self, name: &CStr, max: usize) -> FsResult<ProcFile> {
        let root = self.root()?;
        let proc = self.directory(&root, c"proc", Some(self.proc_magic))?;
        // /proc/self is a kernel symlink: bind our numeric PID directory instead.
        let pid = component(unsafe { libc::getpid() }.to_string().as_bytes())?;
        let own = open(proc.0.as_raw_fd(), &pid, DIR_FLAGS)?;
        let st = facts(own.as_raw_fd())?;
        if st.mode & libc::S_IFMT != libc::S_IFDIR || st.magic != self.proc_magic {
            return Err(FsError::Refused);
        }
        let fd = open(own.as_raw_fd(), name, FILE_FLAGS | libc::O_RDONLY)?;
        let st = facts(fd.as_raw_fd())?;
        if st.mode & libc::S_IFMT != libc::S_IFREG || st.magic != self.proc_magic {
            return Err(FsError::Refused);
        }
        Ok(ProcFile {
            magic: st.magic,
            bytes: text(fd, max)?,
        })
    }
}
// A separate stream owns only the fresh listing description. Early failures
// close through Drop; successful listings also propagate closedir errors.
struct Stream(*mut libc::DIR);
impl Stream {
    fn finish(mut self) -> FsResult<()> {
        let ptr = std::mem::replace(&mut self.0, std::ptr::null_mut());
        if unsafe { libc::closedir(ptr) } == 0 {
            Ok(())
        } else {
            Err(last())
        }
    }
}
impl Drop for Stream {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                libc::closedir(self.0);
            }
        }
    }
}
impl CgroupFs for LinuxFs {
    type Dir = Dir;
    fn proc_cgroup(&self) -> FsResult<ProcFile> {
        self.proc_file(c"cgroup", TEXT_MAX)
    }
    fn proc_status(&self) -> FsResult<ProcFile> {
        self.proc_file(c"status", STATUS_MAX)
    }
    fn identity(&self) -> FsResult<(u32, u32)> {
        Ok(unsafe { (libc::getuid(), libc::geteuid()) })
    }
    fn pid(&self) -> FsResult<u32> {
        Ok(unsafe { libc::getpid() } as u32)
    }
    fn open_root(&self) -> FsResult<Dir> {
        let mut dir = self.root()?;
        for part in [c"sys", c"fs"] {
            dir = self.directory(&dir, part, None)?;
        }
        self.directory(&dir, c"cgroup", Some(self.cgroup_magic))
    }
    fn open_child(&self, dir: &Dir, name: &[u8]) -> FsResult<Dir> {
        self.trust(dir.0.as_raw_fd(), libc::S_IFDIR, Some(self.cgroup_magic))?;
        self.directory(dir, &component(name)?, Some(self.cgroup_magic))
    }
    fn stat(&self, dir: &Dir) -> FsResult<Stat> {
        facts(dir.0.as_raw_fd())
    }
    fn read(&self, dir: &Dir, file: &str, max: usize) -> FsResult<Vec<u8>> {
        text(self.control(dir, file, false)?, max.min(TEXT_MAX))
    }
    fn write(&self, dir: &Dir, file: &str, bytes: &[u8]) -> FsResult<()> {
        if bytes.is_empty() || bytes.len() > TEXT_MAX {
            return Err(FsError::TooBig);
        }
        let fd = self.control(dir, file, true)?;
        // One kernel control transaction; never retry a short write.
        let n = unsafe { libc::write(fd.as_raw_fd(), bytes.as_ptr().cast(), bytes.len()) };
        let result = if n < 0 {
            Err(last())
        } else if n as usize != bytes.len() {
            Err(FsError::Io)
        } else {
            Ok(())
        };
        let closed = close(fd);
        result.and(closed)
    }
    fn xattr_delegate(&self, dir: &Dir) -> FsResult<Option<Vec<u8>>> {
        self.trust(dir.0.as_raw_fd(), libc::S_IFDIR, Some(self.cgroup_magic))?;
        let mut bytes = [0u8; 2];
        let n = unsafe {
            libc::fgetxattr(
                dir.0.as_raw_fd(),
                c"user.delegate".as_ptr(),
                bytes.as_mut_ptr().cast(),
                bytes.len(),
            )
        };
        if n < 0 {
            return if io::Error::last_os_error().raw_os_error() == Some(libc::ENODATA) {
                Ok(None)
            } else {
                Err(last())
            };
        }
        Ok(Some(bytes[..n as usize].to_vec()))
    }
    fn mkdir_runs(&self, dir: &Dir) -> FsResult<Created> {
        self.trust(dir.0.as_raw_fd(), libc::S_IFDIR, Some(self.cgroup_magic))?;
        if unsafe { libc::mkdirat(dir.0.as_raw_fd(), c"runs".as_ptr(), 0o700) } == 0 {
            return Ok(Created::Created);
        }
        if io::Error::last_os_error().raw_os_error() == Some(libc::EEXIST) {
            Ok(Created::Exists)
        } else {
            Err(last())
        }
    }
    fn list(&self, dir: &Dir, max_records: usize) -> FsResult<Vec<RawEntry>> {
        self.trust(dir.0.as_raw_fd(), libc::S_IFDIR, Some(self.cgroup_magic))?;
        let fd = open(dir.0.as_raw_fd(), c".", DIR_FLAGS)?;
        self.trust(fd.as_raw_fd(), libc::S_IFDIR, Some(self.cgroup_magic))?;
        #[cfg(test)]
        if let Some(observe) = self.observer {
            observe(dir.0.as_raw_fd(), fd.as_raw_fd());
        }
        let stream = unsafe { libc::fdopendir(fd.as_raw_fd()) };
        if stream.is_null() {
            return Err(last());
        }
        let _transferred = fd.into_raw_fd();
        let stream = Stream(stream);
        let mut records = Vec::new();
        loop {
            unsafe {
                *libc::__errno_location() = 0;
            }
            let entry = unsafe { libc::readdir(stream.0) };
            if entry.is_null() {
                if io::Error::last_os_error().raw_os_error() != Some(0) {
                    return Err(last());
                }
                break;
            }
            if records.len() >= max_records.min(RECORD_MAX) {
                return Err(FsError::TooBig);
            }
            let entry = unsafe { &*entry };
            let name = unsafe { CStr::from_ptr(entry.d_name.as_ptr()) }
                .to_bytes()
                .to_vec();
            let mode = if entry.d_type == libc::DT_UNKNOWN {
                let mut st = MaybeUninit::<libc::stat>::uninit();
                let name_c = CString::new(name.as_slice()).map_err(|_| FsError::Invalid)?;
                Some(
                    if unsafe {
                        libc::fstatat(
                            dir.0.as_raw_fd(),
                            name_c.as_ptr(),
                            st.as_mut_ptr(),
                            libc::AT_SYMLINK_NOFOLLOW,
                        )
                    } == 0
                    {
                        Ok(unsafe { st.assume_init() }.st_mode)
                    } else {
                        Err(())
                    },
                )
            } else {
                None
            };
            records.push(RawEntry {
                name,
                d_type: entry.d_type,
                fstatat_mode: mode,
            });
        }
        stream.finish()?;
        Ok(records)
    }
    fn rmdir(&self, dir: &Dir, name: &[u8]) -> FsResult<()> {
        self.trust(dir.0.as_raw_fd(), libc::S_IFDIR, Some(self.cgroup_magic))?;
        let name = component(name)?;
        if unsafe { libc::unlinkat(dir.0.as_raw_fd(), name.as_ptr(), libc::AT_REMOVEDIR) } == 0 {
            Ok(())
        } else {
            Err(last())
        }
    }
    fn open_ro(&self, dir: &Dir, file: &str) -> FsResult<()> {
        close(self.control(dir, file, false)?)
    }
}

#[cfg(test)]
mod tests {
    use super::super::parse::{self, Entry};
    use super::*;
    use std::{
        os::unix::{
            ffi::OsStrExt,
            fs::{PermissionsExt, symlink},
            net::UnixListener,
        },
        path::Path,
    };
    fn ordinary() -> bool {
        if unsafe { libc::geteuid() } == 0 {
            eprintln!("SKIP E3 owned native tests: root");
            false
        } else {
            true
        }
    }
    fn fixture() -> Option<(tempfile::TempDir, LinuxFs, Dir)> {
        if !ordinary() {
            return None;
        }
        let temp = tempfile::tempdir().unwrap();
        let name = CString::new(temp.path().as_os_str().as_bytes()).unwrap();
        let fd = open(libc::AT_FDCWD, &name, DIR_FLAGS).unwrap();
        Some((temp, LinuxFs::owned(unsafe { libc::geteuid() }), Dir(fd)))
    }
    fn file(path: &Path) {
        std::fs::write(path, b"safe").unwrap();
    }
    #[test]
    fn l1_bounds_and_bounded_writes() {
        let Some((t, fs, dir)) = fixture() else {
            return;
        };
        let p = t.path().join("cgroup.events");
        std::fs::write(&p, vec![b'x'; 4096]).unwrap();
        assert!(fs.read(&dir, "cgroup.events", 4096).unwrap().len() == 4096);
        std::fs::write(&p, vec![b'x'; 4097]).unwrap();
        assert!(matches!(
            fs.read(&dir, "cgroup.events", 4096),
            Err(FsError::TooBig)
        ));
        let fd = fs.control(&dir, "cgroup.events", false).unwrap();
        assert!(matches!(text(fd, 4096), Err(FsError::TooBig)));
        assert!(matches!(
            fs.write(&dir, "cgroup.events", &vec![0; 4097]),
            Err(FsError::TooBig)
        ));
        assert!(std::fs::read(&p).unwrap().len() == 4097);
        fs.write(&dir, "cgroup.events", b"ok").unwrap();
        assert!(std::fs::read(&p).unwrap().starts_with(b"ok"));
        assert!(matches!(
            fs.read(&dir, "../escape", 4),
            Err(FsError::Invalid)
        ));
    }
    #[test]
    fn l2_nofollow_and_nonblocking_fifo() {
        let Some((t, fs, dir)) = fixture() else {
            return;
        };
        file(&t.path().join("target"));
        symlink("target", t.path().join("cgroup.events")).unwrap();
        std::fs::create_dir(t.path().join("child")).unwrap();
        symlink("child", t.path().join("alias")).unwrap();
        assert!(matches!(
            fs.read(&dir, "cgroup.events", 4096),
            Err(FsError::Refused)
        ));
        assert!(matches!(
            fs.write(&dir, "cgroup.events", b"bad"),
            Err(FsError::Refused)
        ));
        assert!(matches!(
            fs.open_child(&dir, b"alias"),
            Err(FsError::Refused)
        ));
        assert!(std::fs::read(t.path().join("target")).unwrap() == b"safe");
        let fifo = CString::new(t.path().join("fifo").as_os_str().as_bytes()).unwrap();
        assert!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) } == 0);
        assert!(fs.read(&dir, "fifo", 4096).is_err());
        assert!(fs.open_ro(&dir, "fifo").is_err());
        assert!(fs.write(&dir, "fifo", b"x").is_err());
    }
    #[test]
    fn l3_raw_budget_includes_dots() {
        let Some((t, fs, dir)) = fixture() else {
            return;
        };
        for n in 0..510 {
            file(&t.path().join(format!("control.{n}")));
        }
        let records = fs.list(&dir, 512).unwrap();
        assert!(records.len() == 512);
        assert!(records.iter().any(|r| r.name == b"."));
        assert!(records.iter().any(|r| r.name == b".."));
        file(&t.path().join("extra.control"));
        assert!(matches!(fs.list(&dir, 512), Err(FsError::TooBig)));
    }
    #[test]
    fn l4_entry_kinds() {
        for (kind, mode, expected) in [
            (libc::DT_DIR, None, Entry::Child),
            (libc::DT_REG, None, Entry::Control),
            (libc::DT_LNK, None, Entry::Refused),
            (libc::DT_UNKNOWN, Some(Ok(libc::S_IFDIR)), Entry::Child),
            (libc::DT_UNKNOWN, Some(Ok(libc::S_IFLNK)), Entry::Refused),
            (libc::DT_UNKNOWN, Some(Err(())), Entry::Refused),
        ] {
            assert!(parse::classify_entry(b"cgroup.events", kind, mode) == expected);
        }
    }
    #[test]
    fn l5_system_constants_and_shared_mode_trust() {
        let fs = LinuxFs::system();
        assert!(
            fs.uid == 0 && fs.proc_magic == 0x9fa0 && fs.cgroup_magic == 0x63677270 && !fs.bypass()
        );
        let Some((t, owned, dir)) = fixture() else {
            return;
        };
        assert!(fs.stat(&dir).unwrap().uid == unsafe { libc::geteuid() });
        assert!(fs.trust(dir.0.as_raw_fd(), libc::S_IFDIR, None).is_err());
        std::fs::set_permissions(t.path(), std::fs::Permissions::from_mode(0o720)).unwrap();
        assert!(owned.list(&dir, 512).is_err());
        std::fs::set_permissions(t.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(owned.mkdir_runs(&dir).unwrap() == Created::Created);
        let runs = owned.open_child(&dir, b"runs").unwrap();
        assert!(owned.stat(&runs).unwrap().mode & 0o777 == 0o700);
        assert!(owned.mkdir_runs(&dir).unwrap() == Created::Exists);
        owned.rmdir(&dir, b"runs").unwrap();
        assert!(owned.open_child(&dir, b"runs").is_err());
    }
    #[test]
    fn l7_xattr_bounded() {
        let Some((_t, fs, dir)) = fixture() else {
            return;
        };
        let rc = unsafe {
            libc::fsetxattr(
                dir.0.as_raw_fd(),
                c"user.delegate".as_ptr(),
                b"1".as_ptr().cast(),
                1,
                0,
            )
        };
        if rc != 0 {
            let e = io::Error::last_os_error().raw_os_error();
            if matches!(e, Some(libc::ENOTSUP | libc::EPERM | libc::EACCES)) {
                eprintln!("SKIP L7: user xattrs refused");
                return;
            }
            panic!("xattr fixture failure");
        }
        assert!(fs.xattr_delegate(&dir).unwrap().as_deref() == Some(b"1"));
        assert!(
            unsafe {
                libc::fsetxattr(
                    dir.0.as_raw_fd(),
                    c"user.delegate".as_ptr(),
                    b"111".as_ptr().cast(),
                    3,
                    0,
                )
            } == 0
        );
        assert!(matches!(fs.xattr_delegate(&dir), Err(FsError::TooBig)));
    }
    fn observe(handle: i32, listing: i32) {
        assert!(handle != listing);
        assert!(unsafe { libc::fcntl(listing, libc::F_GETFD) } & libc::FD_CLOEXEC != 0);
        assert!(unsafe { libc::lseek(handle, 0, libc::SEEK_CUR) } == 0);
    }
    #[test]
    fn l9_fresh_descriptions_and_owned_lifetime() {
        let Some((t, mut fs, dir)) = fixture() else {
            return;
        };
        fs.observer = Some(observe);
        for n in ["one", "two", "three"] {
            file(&t.path().join(n));
        }
        for _ in 0..2 {
            assert!(fs.list(&dir, 512).unwrap().len() == 5);
        }
        file(&t.path().join("four"));
        assert!(
            fs.list(&dir, 512)
                .unwrap()
                .iter()
                .any(|r| r.name == b"four")
        );
        std::fs::remove_file(t.path().join("two")).unwrap();
        assert!(!fs.list(&dir, 512).unwrap().iter().any(|r| r.name == b"two"));
        observe(
            dir.0.as_raw_fd(),
            open(dir.0.as_raw_fd(), c".", DIR_FLAGS)
                .unwrap()
                .as_raw_fd(),
        );
        // The owned handle remains bound when its path entry is renamed.
        let moved = t.path().with_extension("moved");
        std::fs::rename(t.path(), &moved).unwrap();
        assert!(fs.list(&dir, 512).unwrap().len() == 5);
        std::fs::rename(&moved, t.path()).unwrap();
    }
    #[test]
    fn l10_real_entry_kinds() {
        let Some((t, fs, dir)) = fixture() else {
            return;
        };
        std::fs::create_dir(t.path().join("child")).unwrap();
        for n in ["cgroup.events", "arx-1-2", "target"] {
            file(&t.path().join(n));
        }
        symlink("target", t.path().join("link")).unwrap();
        let fifo = CString::new(t.path().join("fifo").as_os_str().as_bytes()).unwrap();
        assert!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) } == 0);
        let _socket = UnixListener::bind(t.path().join("socket")).unwrap();
        let records = fs.list(&dir, 512).unwrap();
        for (name, expected) in [
            (b"child".as_slice(), Entry::Child),
            (b"cgroup.events", Entry::Control),
            (b"arx-1-2", Entry::Refused),
            (b"link", Entry::Refused),
            (b"fifo", Entry::Refused),
            (b"socket", Entry::Refused),
        ] {
            let r = records.iter().find(|r| r.name == name).unwrap();
            assert!(parse::classify_entry(&r.name, r.d_type, r.fstatat_mode) == expected);
        }
        assert!(std::fs::read(t.path().join("target")).unwrap() == b"safe");
    }
    struct Count {
        fs: LinuxFs,
        changes: std::cell::Cell<usize>,
        status_reads: std::cell::Cell<usize>,
    }
    // Any observation beyond S0 is an error in L8; all mutations are counted.
    impl CgroupFs for Count {
        type Dir = Dir;
        fn proc_status(&self) -> FsResult<ProcFile> {
            self.status_reads.set(self.status_reads.get() + 1);
            self.fs.proc_status()
        }
        fn proc_cgroup(&self) -> FsResult<ProcFile> {
            panic!("L8 beyond S0");
        }
        fn identity(&self) -> FsResult<(u32, u32)> {
            panic!("L8 beyond S0");
        }
        fn pid(&self) -> FsResult<u32> {
            panic!("L8 beyond S0");
        }
        fn open_root(&self) -> FsResult<Dir> {
            panic!("L8 beyond S0");
        }
        fn open_child(&self, _: &Dir, _: &[u8]) -> FsResult<Dir> {
            panic!("L8 beyond S0");
        }
        fn stat(&self, _: &Dir) -> FsResult<Stat> {
            panic!("L8 beyond S0");
        }
        fn read(&self, _: &Dir, _: &str, _: usize) -> FsResult<Vec<u8>> {
            panic!("L8 beyond S0");
        }
        fn write(&self, _: &Dir, _: &str, _: &[u8]) -> FsResult<()> {
            self.changes.set(self.changes.get() + 1);
            Err(FsError::Io)
        }
        fn mkdir_runs(&self, _: &Dir) -> FsResult<Created> {
            self.changes.set(self.changes.get() + 1);
            Err(FsError::Io)
        }
        fn rmdir(&self, _: &Dir, _: &[u8]) -> FsResult<()> {
            self.changes.set(self.changes.get() + 1);
            Err(FsError::Io)
        }
        fn list(&self, _: &Dir, _: usize) -> FsResult<Vec<RawEntry>> {
            panic!("L8 beyond S0");
        }
        fn xattr_delegate(&self, _: &Dir) -> FsResult<Option<Vec<u8>>> {
            panic!("L8 beyond S0");
        }
        fn open_ro(&self, _: &Dir, _: &str) -> FsResult<()> {
            panic!("L8 beyond S0");
        }
    }
    struct NoWait;
    impl super::super::setup::Clock for NoWait {
        fn now(&self) -> std::time::Duration {
            panic!("L8 beyond S0");
        }
        fn sleep(&self, _: std::time::Duration) {
            panic!("L8 beyond S0");
        }
    }
    #[test]
    fn l8_unprivileged_hardening_zero_changes() {
        if !ordinary() {
            return;
        }
        let fs = LinuxFs::system();
        let status = fs.proc_status().unwrap();
        if parse::hardening(&status.bytes) == parse::Hardening::Standard {
            eprintln!("SKIP L8: unexpected Standard hardening");
            return;
        }
        let count = Count {
            fs,
            changes: 0.into(),
            status_reads: 0.into(),
        };
        let location = parse::locate(b"0::/arxd.service/supervisor\n").unwrap();
        let result = super::super::setup::setup(
            &count,
            &location,
            &NoWait,
            &crate::development::Cancellation::default(),
        );
        assert!(
            result
                == super::super::setup::SetupOutcome::Unavailable(
                    super::super::setup::SetupFailure::Hardening
                )
        );
        assert!(count.changes.get() == 0 && count.status_reads.get() == 1);
    }
}
