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
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
static SERIAL: AtomicU64 = AtomicU64::new(0);
pub(super) struct Group {
    parent: OwnedFd,
    dir: OwnedFd,
    name: CString,
    pub procs: File,
    removed: std::cell::Cell<bool>,
    residue: Arc<AtomicBool>,
}
// Keep this helper syscall-free after unlinkat: the failed-openat caller
// deliberately reads errno after cleanup, preserving the legacy error.
fn release_leaf(parent: i32, name: &std::ffi::CStr, residue: &AtomicBool) {
    if unsafe { libc::unlinkat(parent, name.as_ptr(), libc::AT_REMOVEDIR) } != 0 {
        residue.store(true, Ordering::Release);
    }
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
    pub fn create(c: &OwnerConfig, residue: Arc<AtomicBool>) -> Result<Self, ExecError> {
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
        let Ok(name) = CString::new(format!(
            "arx-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        )) else {
            unreachable!("numeric name")
        };
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
            release_leaf(fd, &name, &residue);
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
                removed: std::cell::Cell::new(false),
                residue,
            }),
            Err(e) => {
                release_leaf(fd, &name, &residue);
                Err(e.into())
            }
        }
    }
    pub fn kill_and_confirm(&self) -> bool {
        if self.removed.get() {
            return true;
        }
        if put(self.dir.as_raw_fd(), c"cgroup.kill", b"1").is_err() {
            return false;
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match text(self.dir.as_raw_fd(), c"cgroup.events") {
                Ok(s) if s.lines().any(|l| l == "populated 0") => {
                    let removed = unsafe {
                        libc::unlinkat(
                            self.parent.as_raw_fd(),
                            self.name.as_ptr(),
                            libc::AT_REMOVEDIR,
                        )
                    } == 0;
                    self.removed.set(removed);
                    return removed;
                }
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
        if !self.kill_and_confirm() {
            self.residue.store(true, Ordering::Release);
        }
        // Normal completion reports removal failure through kill_and_confirm.
        // Error unwinding remains best effort; caller must quarantine errors.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    impl Group {
        #[cfg(all(test, target_os = "linux"))]
        pub(crate) fn owned_test_group(base: &tempfile::TempDir, residue: Arc<AtomicBool>) -> Self {
            let leaf = base.path().join("leaf");
            std::fs::create_dir(&leaf).unwrap();
            std::fs::write(leaf.join("cgroup.kill"), b"").unwrap();
            std::fs::write(leaf.join("cgroup.events"), b"populated 0\n").unwrap();
            Self {
                parent: File::open(base.path()).unwrap().into(),
                dir: File::open(&leaf).unwrap().into(),
                name: CString::new("leaf").unwrap(),
                procs: File::open(leaf.join("cgroup.events")).unwrap(),
                removed: std::cell::Cell::new(false),
                residue,
            }
        }
    }

    #[test]
    fn release_leaf_tracks_failed_unlink_without_reset_or_errno_change() {
        let base = tempfile::tempdir().unwrap();
        let parent = File::open(base.path()).unwrap();
        let flag = AtomicBool::new(false);
        std::fs::create_dir(base.path().join("empty")).unwrap();
        release_leaf(parent.as_raw_fd(), c"empty", &flag);
        assert!(!base.path().join("empty").exists());
        assert!(!flag.load(Ordering::Acquire));

        std::fs::create_dir(base.path().join("full")).unwrap();
        std::fs::write(base.path().join("full/owned-file"), b"").unwrap();
        release_leaf(parent.as_raw_fd(), c"full", &flag);
        let error = std::io::Error::last_os_error().raw_os_error();
        assert_eq!(error, Some(libc::ENOTEMPTY));
        assert!(flag.load(Ordering::Acquire));
        assert!(base.path().join("full/owned-file").exists());

        // A later confirmed removal cannot clear earlier uncertainty.
        std::fs::create_dir(base.path().join("later")).unwrap();
        release_leaf(parent.as_raw_fd(), c"later", &flag);
        assert!(!base.path().join("later").exists());
        assert!(flag.load(Ordering::Acquire));
    }

    #[test]
    fn group_drop_signals_nonempty_leaf_but_not_already_removed() {
        for removed in [false, true] {
            let base = tempfile::tempdir().unwrap();
            let leaf = base.path().join("leaf");
            std::fs::create_dir(&leaf).unwrap();
            std::fs::write(leaf.join("cgroup.kill"), b"").unwrap();
            std::fs::write(leaf.join("cgroup.events"), b"populated 0\n").unwrap();
            let residue = Arc::new(AtomicBool::new(false));
            let group = Group {
                parent: File::open(base.path()).unwrap().into(),
                dir: File::open(&leaf).unwrap().into(),
                name: CString::new("leaf").unwrap(),
                procs: File::open(leaf.join("cgroup.events")).unwrap(),
                removed: std::cell::Cell::new(removed),
                residue: residue.clone(),
            };
            drop(group);
            assert_eq!(residue.load(Ordering::Acquire), !removed);
            assert_eq!(
                std::fs::read(leaf.join("cgroup.kill")).unwrap(),
                if removed {
                    b"".as_slice()
                } else {
                    b"1".as_slice()
                }
            );
        }
    }
    #[test]
    fn populated_zero_with_removal_failure_is_not_confirmed() {
        // Ordinary temporary files only; never a real cgroup or privileged write.
        let base = std::env::temp_dir().join(format!(
            "arx-cgroup-test-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&base).unwrap();
        let leaf = base.join("leaf");
        std::fs::create_dir(&leaf).unwrap();
        std::fs::write(leaf.join("cgroup.kill"), b"").unwrap();
        std::fs::write(leaf.join("cgroup.events"), b"populated 0\n").unwrap();
        let group = Group {
            parent: File::open(&base).unwrap().into(),
            dir: File::open(&leaf).unwrap().into(),
            name: CString::new("leaf").unwrap(),
            procs: File::open(leaf.join("cgroup.events")).unwrap(),
            removed: std::cell::Cell::new(false),
            residue: Arc::new(AtomicBool::new(false)),
        };
        assert!(!group.kill_and_confirm());
        drop(group);
        std::fs::remove_dir_all(base).unwrap();
    }
}
