//! Owner-local executor. No CLI selector, identity override, account creation or repair.
#[cfg(unix)]
use super::owner_plan::OsManager;
use super::{layout::Error, owner_plan::Plan};
#[cfg(any(unix, test))]
use super::{
    layout::{Layout, Platform, Policy},
    owner_plan::{ConstantGate, DROPIN, DROPIN_PATH, Gate, Operation, PLIST, UNIT},
};
#[cfg(any(unix, test))]
use std::path::Path;

/// Execute an already validated plan, with fresh native authority checks. Installing
/// artifacts never starts the daemon. Partial failures are retained, not rolled back.
pub fn execute(plan: &Plan) -> Result<(), Error> {
    #[cfg(unix)]
    {
        let mut manager = imp::Manager::new()?;
        for op in plan.operations() {
            allowed(&manager.layout, op)?;
        }
        for op in plan.operations() {
            executable(op)?;
        }
        plan.execute(&mut manager)
    }
    #[cfg(not(unix))]
    {
        let _ = plan;
        Err(Error::NativeAdapterMissing)
    }
}

/// Prepare only empty, protected SQLite prerequisites. Existing files are never
/// overwritten; schema/host identity initialization remains Activity's transaction.
/// This is separate from install and may be retried after a partial failure.
pub fn provision_state() -> Result<(), Error> {
    #[cfg(unix)]
    {
        imp::Manager::new()?.provision()
    }
    #[cfg(not(unix))]
    {
        Err(Error::NativeAdapterMissing)
    }
}

#[cfg(any(unix, test))]
fn artifact(l: &Layout, path: &Path, bytes: &[u8]) -> bool {
    match l.platform() {
        Platform::Linux => {
            (path == l.artifact() && bytes == UNIT)
                || (path == Path::new(DROPIN_PATH) && bytes == DROPIN)
        }
        Platform::Macos => path == l.artifact() && bytes == PLIST,
        _ => false,
    }
}
#[cfg(any(unix, test))]
fn command(platform: Platform, program: &str, args: &[&str], when: Gate) -> bool {
    match platform {
        Platform::Linux => {
            program == "/usr/bin/systemctl"
                && (((args == ["daemon-reload"]
                    || args == ["enable", "arxd.service"]
                    || args == ["start", "arxd.service"])
                    && when == Gate::Always)
                    || (args == ["disable", "arxd.service"]
                        && matches!(when, Gate::Always | Gate::ArtifactPresent))
                    || (args == ["stop", "arxd.service"]
                        && matches!(when, Gate::Always | Gate::Loaded)))
        }
        Platform::Macos => {
            program == "/bin/launchctl"
                && ((args == ["bootstrap", "system", super::owner_query::PLIST_PATH]
                    && when == Gate::NotLoaded)
                    || (args == ["bootout", super::owner_query::LAUNCHD_TARGET]
                        && when == Gate::Loaded)
                    || (matches!(args, ["enable" | "disable", target] if *target == super::owner_query::LAUNCHD_TARGET)
                        && when == Gate::Always))
        }
        _ => false,
    }
}
/// Phase 3 must implement these semantics before native lifecycle execution.
#[cfg(any(unix, test))]
fn executable(op: &Operation) -> Result<(), Error> {
    match op {
        Operation::Expect(_)
        | Operation::StartPreflight
        | Operation::ProvisionState
        | Operation::Constant {
            when: ConstantGate::UnitAbsent,
            ..
        }
        | Operation::Command {
            when: Gate::Loaded | Gate::NotLoaded | Gate::ArtifactPresent,
            ..
        } => Err(Error::Manager),
        _ => Ok(()),
    }
}
#[cfg(any(unix, test))]
fn allowed(l: &Layout, op: &Operation) -> Result<(), Error> {
    let valid = match op {
        Operation::Verify { path, policy, .. } => match policy {
            Policy::BinaryIntegrity => path == l.binary(),
            Policy::PrivateDirectory => l.private_dirs().contains(path) || path == l.state(),
            Policy::PrivateFile => path == l.config(),
            Policy::EndpointDirectory => Some(path.as_path()) == l.endpoint().parent(),
        },
        Operation::EnsureDirectory { path, policy } => match policy {
            Policy::PrivateDirectory => l.private_dirs().contains(path),
            Policy::EndpointDirectory => false,
            _ => false,
        },
        Operation::Constant { path, bytes, when } => {
            artifact(l, path, bytes)
                && (*when == ConstantGate::Always
                    || (l.platform() == Platform::Linux && path == Path::new(DROPIN_PATH)))
        }
        Operation::Expect(_) | Operation::StartPreflight | Operation::ProvisionState => true,
        Operation::RemoveConstant { path, expected } => artifact(l, path, expected),
        Operation::AtomicConfig {
            path,
            bytes,
            expected,
        } => {
            path == l.config()
                && bytes.len() <= super::config::MAX_CONFIG_BYTES
                && expected
                    .as_ref()
                    .is_none_or(|v| v.len() <= super::config::MAX_CONFIG_BYTES)
        }
        Operation::Command {
            program,
            args,
            when,
        } => command(l.platform(), program, args, *when),
        _ => false,
    };
    if valid { Ok(()) } else { Err(Error::Layout) }
}

#[cfg(unix)]
mod imp {
    use super::*;
    use crate::service::{config::ServiceConfig, native, native::NativeResolver};
    use std::{
        ffi::CString,
        fs::File,
        io::{Read, Write},
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::fs::MetadataExt,
        },
        process::{Command, Stdio},
        time::{Duration, Instant},
    };

    // Only tests can choose a different root or expected identity. Production
    // always starts at / and requires native root:root, including real IDs.
    struct Fs {
        root: File,
        uid: u32,
        gid: u32,
    }
    fn fd(result: i32) -> Result<File, Error> {
        if result < 0 {
            Err(Error::Permissions)
        } else {
            // SAFETY: successful open/openat transfers a new owned descriptor.
            Ok(unsafe { File::from_raw_fd(result) })
        }
    }
    fn name(value: &str) -> Result<CString, Error> {
        CString::new(value).map_err(|_| Error::Layout)
    }
    fn parts(path: &Path) -> Result<Vec<&str>, Error> {
        let s = path.to_str().ok_or(Error::Layout)?;
        if !s.starts_with('/') || s.len() > 4096 || s.contains(['\0', '\\']) {
            return Err(Error::Layout);
        }
        let parts: Vec<_> = s[1..].split('/').collect();
        if parts.iter().any(|v| matches!(*v, "" | "." | "..")) {
            return Err(Error::Layout);
        }
        Ok(parts)
    }
    impl Fs {
        fn check(&self, f: &File, directory: bool, mode: Option<u32>) -> Result<(), Error> {
            native::os::local(f)?;
            let m = f.metadata().map_err(|_| Error::Permissions)?;
            let p = native::os::protection(f)?; // Reject ACLs on the opened object.
            #[cfg(target_os = "linux")]
            if directory {
                let n = unsafe {
                    libc::fgetxattr(
                        f.as_raw_fd(),
                        c"system.posix_acl_default".as_ptr(),
                        std::ptr::null_mut(),
                        0,
                    )
                };
                if n >= 0 || std::io::Error::last_os_error().raw_os_error() != Some(libc::ENODATA) {
                    return Err(Error::Permissions);
                }
            }
            let crate::service::layout::Protection::Unix {
                uid,
                gid,
                mode: bits,
            } = p
            else {
                return Err(Error::Permissions);
            };
            if uid != self.uid
                || gid != self.gid
                || if directory {
                    !m.is_dir()
                } else {
                    !m.is_file() || m.nlink() != 1
                }
                || mode.map_or(bits & 0o7022 != 0, |mode| bits != mode)
            {
                return Err(Error::Permissions);
            }
            Ok(())
        }
        fn open_at(&self, parent: &File, n: &CString, dir: bool) -> Result<Option<File>, Error> {
            let raw = unsafe {
                libc::openat(
                    parent.as_raw_fd(),
                    n.as_ptr(),
                    libc::O_RDONLY
                        | libc::O_CLOEXEC
                        | libc::O_NOFOLLOW
                        | libc::O_NONBLOCK
                        | if dir { libc::O_DIRECTORY } else { 0 },
                )
            };
            if raw < 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT) {
                return Ok(None);
            }
            Ok(Some(fd(raw)?))
        }
        fn parent(&self, path: &Path) -> Result<Option<(File, CString)>, Error> {
            let mut parts = parts(path)?;
            let leaf = name(parts.pop().ok_or(Error::Layout)?)?;
            let mut parent = self.root.try_clone().map_err(|_| Error::Permissions)?;
            self.check(&parent, true, None)?;
            for part in parts {
                let Some(next) = self.open_at(&parent, &name(part)?, true)? else {
                    return Ok(None);
                };
                self.check(&next, true, None)?;
                parent = next;
            }
            Ok(Some((parent, leaf)))
        }
        fn verify(&self, path: &Path, policy: Policy, required: bool) -> Result<(), Error> {
            let Some((p, n)) = self.parent(path)? else {
                return if required { Err(Error::Layout) } else { Ok(()) };
            };
            let dir = matches!(policy, Policy::PrivateDirectory | Policy::EndpointDirectory);
            let Some(f) = self.open_at(&p, &n, dir)? else {
                return if required { Err(Error::Layout) } else { Ok(()) };
            };
            self.check(
                &f,
                dir,
                match policy {
                    Policy::PrivateDirectory => Some(0o700),
                    Policy::PrivateFile => Some(0o600),
                    Policy::EndpointDirectory => Some(0o755),
                    Policy::BinaryIntegrity => None,
                },
            )
        }
        fn directory(&self, path: &Path, mode: u32) -> Result<(), Error> {
            let (p, n) = self.parent(path)?.ok_or(Error::Layout)?;
            match self.open_at(&p, &n, true)? {
                Some(f) => self.check(&f, true, Some(mode)),
                None => {
                    if unsafe { libc::mkdirat(p.as_raw_fd(), n.as_ptr(), mode as libc::mode_t) }
                        != 0
                    {
                        return Err(Error::Permissions);
                    }
                    let f = self.open_at(&p, &n, true)?.ok_or(Error::Permissions)?;
                    native::os::protection(&f)?;
                    if unsafe { libc::fchmod(f.as_raw_fd(), mode as libc::mode_t) } != 0 {
                        return Err(Error::Permissions);
                    }
                    self.check(&f, true, Some(mode))?;
                    f.sync_all().map_err(|_| Error::Permissions)?;
                    p.sync_all().map_err(|_| Error::Permissions)
                }
            }
        }
        fn bytes(&self, f: &mut File, mode: u32, bound: usize) -> Result<Vec<u8>, Error> {
            self.check(f, false, Some(mode))?;
            let mut b = Vec::new();
            f.take((bound + 1) as u64)
                .read_to_end(&mut b)
                .map_err(|_| Error::Permissions)?;
            self.check(f, false, Some(mode))?;
            if b.len() > bound {
                Err(Error::Config)
            } else {
                Ok(b)
            }
        }
        fn create(&self, p: &File, n: &CString, bytes: &[u8], mode: u32) -> Result<File, Error> {
            let mut f = fd(unsafe {
                libc::openat(
                    p.as_raw_fd(),
                    n.as_ptr(),
                    libc::O_WRONLY
                        | libc::O_CREAT
                        | libc::O_EXCL
                        | libc::O_NOFOLLOW
                        | libc::O_CLOEXEC,
                    mode,
                )
            })?;
            // Change protection only on this exclusively created handle.
            native::os::protection(&f)?;
            if unsafe { libc::fchmod(f.as_raw_fd(), mode as libc::mode_t) } != 0 {
                return Err(Error::Permissions);
            }
            self.check(&f, false, Some(mode))?;
            f.write_all(bytes).map_err(|_| Error::Permissions)?;
            f.sync_all().map_err(|_| Error::Permissions)?;
            Ok(f)
        }
        fn constant(&self, path: &Path, bytes: &[u8], remove: bool) -> Result<(), Error> {
            let (p, n) = self.parent(path)?.ok_or(Error::Layout)?;
            if let Some(mut f) = self.open_at(&p, &n, false)? {
                if self.bytes(&mut f, 0o644, bytes.len())? != bytes {
                    return Err(Error::Config);
                }
                if remove {
                    self.same(&p, &n, &f)?;
                    if unsafe { libc::unlinkat(p.as_raw_fd(), n.as_ptr(), 0) } != 0 {
                        return Err(Error::Permissions);
                    }
                }
            } else if !remove {
                self.create(&p, &n, bytes, 0o644)?;
            }
            p.sync_all().map_err(|_| Error::Permissions)
        }
        fn same(&self, p: &File, n: &CString, f: &File) -> Result<(), Error> {
            let other = self.open_at(p, n, false)?.ok_or(Error::Permissions)?;
            self.check(&other, false, Some(0o600))
                .or_else(|_| self.check(&other, false, Some(0o644)))?;
            let a = f.metadata().map_err(|_| Error::Permissions)?;
            let b = other.metadata().map_err(|_| Error::Permissions)?;
            if a.dev() == b.dev() && a.ino() == b.ino() {
                Ok(())
            } else {
                Err(Error::Permissions)
            }
        }
        fn config(
            &self,
            path: &Path,
            bytes: &[u8],
            expected: &Option<Vec<u8>>,
        ) -> Result<(), Error> {
            let (p, n) = self.parent(path)?.ok_or(Error::Layout)?;
            self.check(&p, true, Some(0o700))?;
            // Serialize all cooperating owner publishers on the pinned private
            // parent. Root in-place writes/mount changes are outside this fence.
            if unsafe { libc::flock(p.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
                return Err(Error::Permissions);
            }
            let old = self.open_at(&p, &n, false)?;
            match (&old, expected) {
                (None, None) => {}
                (Some(f), Some(want)) => {
                    let mut read = f.try_clone().map_err(|_| Error::Permissions)?;
                    if self.bytes(&mut read, 0o600, super::super::config::MAX_CONFIG_BYTES)?
                        != *want
                    {
                        return Err(Error::Config);
                    }
                }
                _ => return Err(Error::Config),
            }
            let mut random = [0; 16];
            getrandom::fill(&mut random).map_err(|_| Error::Permissions)?;
            let temp = name(&format!(".arxd-{}.tmp", hex::encode(random)))?;
            let staged = self.create(&p, &temp, bytes, 0o600)?;
            let result = (|| {
                if let Some(f) = &old {
                    self.same(&p, &n, f)?;
                }
                // Exclusive publication for a missing destination. For replacement,
                // the private parent lock + trusted ancestry protects the CAS.
                let rc = if old.is_none() {
                    unsafe {
                        libc::linkat(p.as_raw_fd(), temp.as_ptr(), p.as_raw_fd(), n.as_ptr(), 0)
                    }
                } else {
                    unsafe {
                        libc::renameat(p.as_raw_fd(), temp.as_ptr(), p.as_raw_fd(), n.as_ptr())
                    }
                };
                if rc != 0 {
                    return Err(Error::Permissions);
                }
                staged.sync_all().map_err(|_| Error::Permissions)
            })();
            // Only our unpredictable, exclusively created temp name is removed.
            let cleanup = unsafe { libc::unlinkat(p.as_raw_fd(), temp.as_ptr(), 0) };
            if cleanup != 0 && std::io::Error::last_os_error().raw_os_error() != Some(libc::ENOENT)
            {
                return Err(Error::Permissions);
            }
            p.sync_all().map_err(|_| Error::Permissions)?;
            result
        }
    }

    pub(super) struct Manager {
        pub(super) layout: Layout,
        fs: Fs,
    }
    impl Manager {
        pub(super) fn new() -> Result<Self, Error> {
            identity()?;
            let layout = Layout::system()?;
            if std::env::current_exe().map_err(|_| Error::Layout)? != layout.binary() {
                return Err(Error::Layout);
            }
            let fs = Fs {
                root: fd(unsafe {
                    libc::open(
                        c"/".as_ptr(),
                        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                    )
                })?,
                uid: 0,
                gid: 0,
            };
            fs.verify(layout.binary(), Policy::BinaryIntegrity, true)?;
            Ok(Self { layout, fs })
        }
        pub(super) fn provision(&self) -> Result<(), Error> {
            identity()?;
            self.fs.provision(self.layout.state())
        }
    }
    impl Fs {
        fn provision(&self, state: &Path) -> Result<(), Error> {
            self.verify(state, Policy::PrivateDirectory, true)?;
            let (p, _) = self
                .parent(&state.join("activity.lock"))?
                .ok_or(Error::Layout)?;
            // Refuse any existing state, including companions, before creating
            // anything. Provisioning is idempotent only for the empty triplet.
            for n in ["activity.sqlite3-wal", "activity.sqlite3-shm"] {
                if self.open_at(&p, &name(n)?, false)?.is_some() {
                    return Err(Error::Permissions);
                }
            }
            for n in [
                "activity.lock",
                "activity.sqlite3",
                "activity.sqlite3-journal",
            ] {
                if let Some(mut f) = self.open_at(&p, &name(n)?, false)? {
                    self.bytes(&mut f, 0o600, 0)?;
                }
            }
            let lock_name = name("activity.lock")?;
            let lock = match self.open_at(&p, &lock_name, false)? {
                Some(f) => f,
                None => self.create(&p, &lock_name, &[], 0o600)?,
            };
            lock.try_lock().map_err(|_| Error::Permissions)?;
            for n in ["activity.sqlite3", "activity.sqlite3-journal"] {
                let n = name(n)?;
                if let Some(mut f) = self.open_at(&p, &n, false)? {
                    self.bytes(&mut f, 0o600, 0)?;
                } else {
                    self.create(&p, &n, &[], 0o600)?;
                }
            }
            p.sync_all().map_err(|_| Error::Permissions)
        }
    }
    fn identity() -> Result<(), Error> {
        if unsafe {
            libc::getuid() == 0
                && libc::geteuid() == 0
                && libc::getgid() == 0
                && libc::getegid() == 0
        } {
            Ok(())
        } else {
            Err(Error::OwnerRequired)
        }
    }
    impl OsManager for Manager {
        fn apply(&mut self, op: &Operation) -> Result<(), Error> {
            identity()?;
            allowed(&self.layout, op)?;
            executable(op)?;
            match op {
                Operation::Verify {
                    path,
                    policy,
                    required,
                } => self.fs.verify(path, *policy, *required),
                Operation::EnsureDirectory { path, policy } => self.fs.directory(
                    path,
                    if *policy == Policy::PrivateDirectory {
                        0o700
                    } else {
                        0o755
                    },
                ),
                Operation::Constant { path, bytes, .. } => {
                    if path == Path::new(DROPIN_PATH) {
                        self.fs
                            .directory(path.parent().ok_or(Error::Layout)?, 0o755)?;
                    }
                    self.fs.constant(path, bytes, false)
                }
                Operation::RemoveConstant { path, expected } => {
                    if self.fs.parent(path)?.is_none() {
                        return Ok(());
                    }
                    self.fs.constant(path, expected, true)
                }
                Operation::AtomicConfig {
                    path,
                    bytes,
                    expected,
                } => {
                    ServiceConfig::parse(bytes, &self.layout, &NativeResolver)?;
                    self.fs.config(path, bytes, expected)
                }
                Operation::Command { program, args, .. } => {
                    self.fs
                        .verify(Path::new(program), Policy::BinaryIntegrity, true)?;
                    run(program, args)
                }
                _ => Err(Error::NativeAdapterMissing),
            }
        }
    }
    fn run(program: &str, args: &[&str]) -> Result<(), Error> {
        let mut child = Command::new(program)
            .args(args)
            .env_clear()
            .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
            .env("LANG", "C")
            .env("LC_ALL", "C")
            .env("SYSTEMD_PAGER", "cat")
            .env("SYSTEMD_COLORS", "0")
            .current_dir("/")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| Error::Manager)?;
        let deadline = Instant::now() + Duration::from_secs(35);
        loop {
            match child.try_wait() {
                Ok(Some(s)) => {
                    return if s.success() {
                        Ok(())
                    } else {
                        Err(Error::Manager)
                    };
                }
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(20))
                }
                _ => {
                    let _ = child.kill();
                    let reap_deadline = Instant::now() + Duration::from_secs(1);
                    while Instant::now() < reap_deadline {
                        if matches!(child.try_wait(), Ok(Some(_))) {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(20));
                    }
                    return Err(Error::Manager);
                }
            }
        }
    }
    #[cfg(test)]
    mod fixtures {
        use super::*;
        use std::os::unix::fs::{PermissionsExt, symlink};
        fn fixture() -> (tempfile::TempDir, Fs) {
            assert_ne!(unsafe { libc::geteuid() }, 0, "ordinary-user fixtures only");
            let dir = tempfile::tempdir().unwrap();
            std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
            let fs = Fs {
                root: File::open(dir.path()).unwrap(),
                uid: unsafe { libc::geteuid() },
                gid: unsafe { libc::getegid() },
            };
            (dir, fs)
        }
        #[test]
        fn cas_refuses_stale_missing_and_unsafe_existing_objects() {
            let (dir, fs) = fixture();
            fs.directory(Path::new("/config"), 0o700).unwrap();
            let path = Path::new("/config/arxd.json");
            fs.config(path, b"first", &None).unwrap();
            assert!(fs.config(path, b"takeover", &None).is_err());
            assert!(
                fs.config(path, b"takeover", &Some(b"stale".to_vec()))
                    .is_err()
            );
            assert_eq!(
                std::fs::read(dir.path().join("config/arxd.json")).unwrap(),
                b"first"
            );
            fs.config(path, b"second", &Some(b"first".to_vec()))
                .unwrap();
            assert_eq!(
                std::fs::read(dir.path().join("config/arxd.json")).unwrap(),
                b"second"
            );
            std::fs::hard_link(
                dir.path().join("config/arxd.json"),
                dir.path().join("alias"),
            )
            .unwrap();
            assert!(
                fs.config(path, b"third", &Some(b"second".to_vec()))
                    .is_err()
            );
            assert_eq!(std::fs::read(dir.path().join("alias")).unwrap(), b"second");
        }
        #[test]
        fn no_takeover_of_modes_symlinks_ancestors_or_wrong_identity() {
            let (dir, fs) = fixture();
            fs.directory(Path::new("/private"), 0o700).unwrap();
            fs.constant(Path::new("/unit"), b"fixed", false).unwrap();
            fs.constant(Path::new("/unit"), b"fixed", false).unwrap();
            assert!(fs.constant(Path::new("/unit"), b"other", false).is_err());
            assert!(fs.constant(Path::new("/unit"), b"other", true).is_err());
            std::fs::set_permissions(
                dir.path().join("unit"),
                std::fs::Permissions::from_mode(0o666),
            )
            .unwrap();
            assert!(fs.constant(Path::new("/unit"), b"fixed", false).is_err());
            assert_eq!(
                std::fs::metadata(dir.path().join("unit")).unwrap().mode() & 0o777,
                0o666
            );
            symlink("private", dir.path().join("link")).unwrap();
            assert!(fs.directory(Path::new("/link/new"), 0o700).is_err());
            std::fs::set_permissions(
                dir.path().join("private"),
                std::fs::Permissions::from_mode(0o777),
            )
            .unwrap();
            assert!(
                fs.config(Path::new("/private/config"), b"x", &None)
                    .is_err()
            );
            let wrong = Fs {
                root: fs.root.try_clone().unwrap(),
                uid: fs.uid + 1,
                gid: fs.gid,
            };
            assert!(
                wrong
                    .verify(Path::new("/unit"), Policy::BinaryIntegrity, true)
                    .is_err()
            );
            assert!(!dir.path().join("private/config").exists());
        }
        #[test]
        fn empty_store_prerequisites_are_private_and_never_replace_state() {
            let (dir, fs) = fixture();
            fs.directory(Path::new("/state"), 0o700).unwrap();
            fs.provision(Path::new("/state")).unwrap();
            fs.provision(Path::new("/state")).unwrap();
            for n in [
                "activity.lock",
                "activity.sqlite3",
                "activity.sqlite3-journal",
            ] {
                let m = std::fs::metadata(dir.path().join("state").join(n)).unwrap();
                assert_eq!(m.mode() & 0o777, 0o600);
                assert_eq!(m.len(), 0);
            }
            std::fs::write(dir.path().join("state/activity.sqlite3"), b"existing state").unwrap();
            assert!(fs.provision(Path::new("/state")).is_err());
            assert_eq!(
                std::fs::read(dir.path().join("state/activity.sqlite3")).unwrap(),
                b"existing state"
            );
        }
        #[test]
        fn publisher_and_state_locks_refuse_concurrent_writers() {
            let (dir, fs) = fixture();
            fs.directory(Path::new("/config"), 0o700).unwrap();
            let parent = File::open(dir.path().join("config")).unwrap();
            assert_eq!(
                unsafe { libc::flock(parent.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
                0
            );
            assert!(
                fs.config(Path::new("/config/arxd.json"), b"x", &None)
                    .is_err()
            );
            assert!(!dir.path().join("config/arxd.json").exists());
            fs.directory(Path::new("/state"), 0o700).unwrap();
            fs.provision(Path::new("/state")).unwrap();
            let lock = File::open(dir.path().join("state/activity.lock")).unwrap();
            lock.try_lock().unwrap();
            assert!(fs.provision(Path::new("/state")).is_err());
            assert_eq!(
                std::fs::metadata(dir.path().join("state/activity.sqlite3"))
                    .unwrap()
                    .len(),
                0
            );
        }
        #[test]
        fn caller_admin_claim_does_not_grant_native_authority() {
            assert_ne!(unsafe { libc::geteuid() }, 0, "ordinary-user refusal only");
            let l = Layout::system().unwrap();
            let p = super::super::super::owner_plan::owner_plan(
                &l,
                &super::super::super::owner_plan::OwnerContext {
                    admin: true,
                    running_binary: l.binary(),
                },
                super::super::super::owner_plan::Action::Start,
            )
            .unwrap();
            assert_eq!(super::super::execute(&p), Err(Error::OwnerRequired));
        }
        #[test]
        fn unsafe_lexical_paths_and_optional_symlinks_refuse() {
            let (dir, fs) = fixture();
            for p in ["relative", "//etc", "/a/../b", "/a/./b", "/a//b", "/a\\b"] {
                assert!(parts(Path::new(p)).is_err());
            }
            assert!(
                fs.verify(Path::new("/missing/child"), Policy::PrivateFile, false)
                    .is_ok()
            );
            symlink("missing", dir.path().join("link")).unwrap();
            assert!(
                fs.verify(Path::new("/link/child"), Policy::PrivateFile, false)
                    .is_err()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn typed_lifecycle_is_fail_closed_and_gates_are_allowlisted() {
        for os in [Platform::Linux, Platform::Macos] {
            let l = Layout::for_test(os);
            let c = super::super::owner_plan::OwnerContext {
                admin: true,
                running_binary: l.binary(),
            };
            for action in [
                super::super::owner_plan::Action::Install,
                super::super::owner_plan::Action::Start,
                super::super::owner_plan::Action::Stop,
                super::super::owner_plan::Action::Uninstall,
            ] {
                let plan = super::super::owner_plan::owner_plan(&l, &c, action).unwrap();
                assert!(plan.operations().iter().all(|op| allowed(&l, op).is_ok()));
                assert!(
                    plan.operations()
                        .iter()
                        .any(|op| executable(op) == Err(Error::Manager))
                );
            }
            assert_eq!(
                allowed(
                    &l,
                    &Operation::EnsureDirectory {
                        path: l.endpoint().parent().unwrap().into(),
                        policy: Policy::EndpointDirectory
                    }
                ),
                Err(Error::Layout)
            );
        }
        assert!(!command(
            Platform::Macos,
            "/bin/launchctl",
            &["bootstrap", "system", super::super::owner_query::PLIST_PATH],
            Gate::Always
        ));
        assert!(!command(
            Platform::Linux,
            "/usr/bin/systemctl",
            super::super::owner_query::SYSTEMD_QUERY,
            Gate::Always
        ));
    }
    #[test]
    fn fixed_vectors_and_artifacts_reject_injection() {
        let l = Layout::for_test(Platform::Linux);
        assert!(command(
            Platform::Linux,
            "/usr/bin/systemctl",
            &["daemon-reload"],
            Gate::Always
        ));
        assert!(!command(
            Platform::Linux,
            "systemctl",
            &["start", "arxd.service"],
            Gate::Always
        ));
        for args in [
            vec!["start", "other.service"],
            vec!["enable", "--now", "arxd.service"],
            vec!["start", "arxd.service", "--root=/tmp"],
        ] {
            assert!(!command(
                Platform::Linux,
                "/usr/bin/systemctl",
                &args,
                Gate::Always
            ));
        }
        assert!(artifact(&l, l.artifact(), UNIT));
        assert!(!artifact(&l, l.artifact(), b"injected"));
        assert!(!artifact(&l, Path::new("/tmp/arxd.service"), UNIT));
        assert!(!command(
            Platform::Macos,
            "/bin/launchctl",
            &["bootstrap", "user/501", "/tmp/job.plist"],
            Gate::Always
        ));
        assert_eq!(
            allowed(
                &l,
                &Operation::AtomicConfig {
                    path: "/tmp/config".into(),
                    bytes: vec![],
                    expected: None
                }
            ),
            Err(Error::Layout)
        );
    }
    #[test]
    fn all_unix_plans_are_allowlisted_and_failure_preserves_the_suffix() {
        use super::super::owner_plan::{Action, OsManager, OwnerContext, owner_plan};
        struct Failure {
            calls: usize,
            fail_at: usize,
        }
        impl OsManager for Failure {
            fn apply(&mut self, _: &Operation) -> Result<(), Error> {
                if self.calls == self.fail_at {
                    return Err(Error::Manager);
                }
                self.calls += 1;
                Ok(())
            }
        }
        for os in [Platform::Linux, Platform::Macos] {
            let l = Layout::for_test(os);
            let c = OwnerContext {
                admin: true,
                running_binary: l.binary(),
            };
            for action in [
                Action::Install,
                Action::Start,
                Action::Stop,
                Action::Uninstall,
            ] {
                let p = owner_plan(&l, &c, action).unwrap();
                for op in p.operations() {
                    allowed(&l, op).unwrap();
                }
                for fail_at in 0..p.operations().len() {
                    let mut m = Failure { calls: 0, fail_at };
                    assert_eq!(p.execute(&mut m), Err(Error::Manager));
                    assert_eq!(m.calls, fail_at);
                }
                if action == Action::Install {
                    assert!(p.operations().iter().all(|o| !matches!(o,Operation::Command {args,..} if args.iter().any(|a| matches!(*a,"start" | "enable" | "bootstrap")))));
                }
            }
        }
    }
    #[cfg(not(unix))]
    #[test]
    fn non_unix_is_unavailable_without_operations() {
        let l = Layout::for_test(Platform::Linux);
        let plan = super::super::owner_plan::owner_plan(
            &l,
            &super::super::owner_plan::OwnerContext {
                admin: true,
                running_binary: l.binary(),
            },
            super::super::owner_plan::Action::Install,
        )
        .unwrap();
        assert_eq!(execute(&plan), Err(Error::NativeAdapterMissing));
        assert_eq!(provision_state(), Err(Error::NativeAdapterMissing));
    }
}
