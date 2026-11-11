//! Root-controlled cgroup v2 delegation. Never enable controllers or alter owner
//! delegation. Missing facilities are an unavailable capability, not a fallback.
use super::{
    ExecError, OwnerConfig,
    unix::{control, put, trusted_root},
};
use std::{
    ffi::CString,
    fs::File,
    io::Read,
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
static SERIAL: AtomicU64 = AtomicU64::new(0);
pub(super) struct Group {
    parent: OwnedFd,
    dir: OwnedFd,
    name: CString,
    pub procs: File,
}
fn text(fd: i32, name: &std::ffi::CStr) -> std::io::Result<String> {
    let mut s = String::new();
    control(fd, name, false)?
        .take(4097)
        .read_to_string(&mut s)?;
    if s.len() > 4096 {
        return Err(std::io::Error::from_raw_os_error(libc::EFBIG));
    }
    Ok(s)
}
impl Group {
    pub fn create(c: &OwnerConfig) -> Result<Self, ExecError> {
        let path = c.cgroup_parent.as_ref().ok_or(ExecError::Unavailable(
            "verified Linux cgroup delegation required",
        ))?;
        if !super::clean_path(path, true) {
            return Err(ExecError::Invalid("cgroup path"));
        }
        let parent = trusted_root(path)?;
        let fd = parent.as_raw_fd();
        let mut fs = std::mem::MaybeUninit::<libc::statfs>::uninit();
        let mut st = std::mem::MaybeUninit::<libc::stat>::uninit();
        if unsafe { libc::fstatfs(fd, fs.as_mut_ptr()) } != 0
            || unsafe { libc::fstat(fd, st.as_mut_ptr()) } != 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        let fs = unsafe { fs.assume_init() };
        let st = unsafe { st.assume_init() };
        if fs.f_type != 0x63677270 || st.st_uid != 0 || st.st_mode & 0o022 != 0 {
            return Err(ExecError::Unavailable(
                "cgroup parent must be protected root-owned cgroup2",
            ));
        }
        let controllers = text(fd, c"cgroup.subtree_control")?;
        if !["pids", "memory"]
            .iter()
            .all(|c| controllers.split_whitespace().any(|x| x == *c))
            || !text(fd, c"cgroup.procs")?.trim().is_empty()
        {
            return Err(ExecError::Unavailable(
                "empty delegated subtree with pids+memory required",
            ));
        }
        let name = CString::new(format!(
            "arx-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ))
        .expect("numeric name");
        if unsafe { libc::mkdirat(fd, name.as_ptr(), 0o700) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let raw = unsafe {
            libc::openat(
                fd,
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if raw < 0 {
            unsafe {
                libc::unlinkat(fd, name.as_ptr(), libc::AT_REMOVEDIR);
            }
            return Err(std::io::Error::last_os_error().into());
        }
        let dir = unsafe { OwnedFd::from_raw_fd(raw) };
        let setup = || -> std::io::Result<File> {
            put(raw, c"pids.max", c.pids.to_string().as_bytes())?;
            put(raw, c"memory.max", c.memory_bytes.to_string().as_bytes())?;
            // Prevent swap from escaping the configured resident-memory quota.
            put(raw, c"memory.swap.max", b"0")?;
            // Verify the required kill interface exists before any process starts.
            let _ = control(raw, c"cgroup.kill", true)?;
            control(raw, c"cgroup.procs", true)
        };
        match setup() {
            Ok(procs) => Ok(Self {
                parent,
                dir,
                name,
                procs,
            }),
            Err(e) => {
                unsafe {
                    libc::unlinkat(fd, name.as_ptr(), libc::AT_REMOVEDIR);
                }
                Err(e.into())
            }
        }
    }
    pub fn kill_and_confirm(&self) -> bool {
        if put(self.dir.as_raw_fd(), c"cgroup.kill", b"1").is_err() {
            return false;
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match text(self.dir.as_raw_fd(), c"cgroup.events") {
                Ok(s) if s.lines().any(|l| l == "populated 0") => return true,
                Err(_) => return false,
                _ => (),
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
impl Drop for Group {
    fn drop(&mut self) {
        self.kill_and_confirm();
        unsafe {
            libc::unlinkat(
                self.parent.as_raw_fd(),
                self.name.as_ptr(),
                libc::AT_REMOVEDIR,
            );
        }
    }
}
