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
        manager.execute(plan.operations())
    }
    #[cfg(not(unix))]
    {
        let _ = plan;
        Err(Error::NativeAdapterMissing)
    }
}

/// Fixed read-only status. Production requires the same root identity and
/// installed-binary integrity boundary as execute; no caller-selected paths.
pub fn status() -> Result<super::owner_query::Status, Error> {
    #[cfg(unix)]
    {
        imp::Manager::new()?.status()
    }
    #[cfg(not(unix))]
    {
        Err(Error::NativeAdapterMissing)
    }
}

/// Prepare missing protected SQLite prerequisites. Existing files are never
/// overwritten; schema/host identity initialization remains Activity's transaction.
/// Install uses the same operation; it may be retried after a partial failure.
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

    use super::super::owner_query::{self, Artifact, Query, Status};

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
            self.check_role(f, directory, mode, false)
        }
        fn ancestor(&self, f: &File) -> Result<(), Error> {
            self.check_role(f, true, None, true)
        }
        fn check_role(
            &self,
            f: &File,
            directory: bool,
            mode: Option<u32>,
            ancestor: bool,
        ) -> Result<(), Error> {
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
            let ancestor_ok = if ancestor {
                #[cfg(not(test))]
                let facts = crate::service::layout::Protection::Unix {
                    uid,
                    gid,
                    mode: bits,
                };
                // Only unit tests normalize identities in their private owned root.
                #[cfg(test)]
                let facts = crate::service::layout::Protection::Unix {
                    uid: if uid == self.uid { 0 } else { 1 },
                    gid: if gid == self.gid { 0 } else { 1 },
                    mode: bits,
                };
                crate::service::layout::check_unix_ancestor(native::os::PLATFORM, &facts).is_ok()
            } else {
                uid == self.uid && gid == self.gid
            };
            if !ancestor_ok
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
            self.ancestor(&parent)?;
            for part in parts {
                let Some(next) = self.open_at(&parent, &name(part)?, true)? else {
                    return Ok(None);
                };
                self.ancestor(&next)?;
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
                    self.new_owner(&f)?;
                    if unsafe { libc::fchmod(f.as_raw_fd(), mode as libc::mode_t) } != 0 {
                        return Err(Error::Permissions);
                    }
                    self.check(&f, true, Some(mode))?;
                    f.sync_all().map_err(|_| Error::Permissions)?;
                    p.sync_all().map_err(|_| Error::Permissions)
                }
            }
        }
        fn new_owner(&self, f: &File) -> Result<(), Error> {
            // This is called only after exclusive creation, never on EEXIST.
            #[cfg(not(test))]
            let owner = (0, 0);
            #[cfg(test)]
            let owner = (self.uid, self.gid);
            if unsafe { libc::fchown(f.as_raw_fd(), owner.0, owner.1) } != 0 {
                return Err(Error::Permissions);
            }
            Ok(())
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
            self.new_owner(&f)?;
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
        runner: Runner,
        skipped: usize,
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
            Ok(Self {
                layout,
                fs,
                runner: Runner::Native,
                skipped: 0,
            })
        }
        pub(super) fn execute(&mut self, operations: &[Operation]) -> Result<(), Error> {
            for op in operations {
                allowed(&self.layout, op)?;
            }
            for op in operations {
                self.apply(op)?;
            }
            Ok(())
        }
        fn observe(&mut self) -> Result<(Query, Artifact), Error> {
            let primary = self.fs.artifact(
                self.layout.artifact(),
                if self.layout.platform() == Platform::Linux {
                    UNIT
                } else {
                    PLIST
                },
            )?;
            if self.layout.platform() == Platform::Linux
                && self.fs.artifact(Path::new(DROPIN_PATH), DROPIN)? == Artifact::Foreign
            {
                return Err(Error::Foreign);
            }
            if primary == Artifact::Foreign {
                return Err(Error::Foreign);
            }
            let query = self.runner.query(&self.fs, self.layout.platform());
            query.gate(Gate::Always, primary)?;
            Ok((query, primary))
        }
        pub(super) fn status(&mut self) -> Result<Status, Error> {
            let (q, a) = self.observe()?;
            if self.layout.platform() == Platform::Linux {
                q.linux_status(a)
            } else {
                match (a, q) {
                    (Artifact::Absent, Query::Absent) => Ok(Status::NotInstalled),
                    (Artifact::Identical, Query::Absent) => Ok(Status::Stopped),
                    _ => Err(Error::Manager),
                }
            }
        }
        pub(super) fn provision(&self) -> Result<(), Error> {
            identity()?;
            self.fs.provision(self.layout.state())
        }
    }
    impl Fs {
        fn artifact(&self, path: &Path, expected: &[u8]) -> Result<Artifact, Error> {
            let Some((p, n)) = self.parent(path)? else {
                return Ok(Artifact::Absent);
            };
            let Some(mut f) = self.open_at(&p, &n, false)? else {
                return Ok(Artifact::Absent);
            };
            match self.bytes(&mut f, 0o644, expected.len()) {
                Ok(bytes) if bytes == expected => Ok(Artifact::Identical),
                _ => Ok(Artifact::Foreign),
            }
        }
        fn state_members(&self, p: &File, required: bool) -> Result<(), Error> {
            self.check(p, true, Some(0o700))?;
            for n in ["activity.sqlite3-wal", "activity.sqlite3-shm"] {
                if self.open_at(p, &name(n)?, false)?.is_some() {
                    return Err(Error::Permissions);
                }
            }
            for n in [
                "activity.lock",
                "activity.sqlite3",
                "activity.sqlite3-journal",
            ] {
                let Some(mut f) = self.open_at(p, &name(n)?, false)? else {
                    if required {
                        return Err(Error::Permissions);
                    }
                    continue;
                };
                self.check(&f, false, Some(0o600))?;
                if n == "activity.lock" && f.metadata().map_err(|_| Error::Permissions)?.len() != 0
                {
                    return Err(Error::Permissions);
                }
                if n == "activity.sqlite3" {
                    let mut header = Vec::new();
                    (&mut f)
                        .take(20)
                        .read_to_end(&mut header)
                        .map_err(|_| Error::Permissions)?;
                    native::rollback_database_header(&header)?;
                }
                self.check(&f, false, Some(0o600))?;
                self.same(p, &name(n)?, &f)?;
            }
            Ok(())
        }
        fn preflight(
            &self,
            layout: &Layout,
            resolver: &impl crate::service::config::Resolver,
        ) -> Result<(), Error> {
            self.verify(layout.binary(), Policy::BinaryIntegrity, true)?;
            for path in layout.private_dirs() {
                self.verify(path, Policy::PrivateDirectory, true)?;
            }
            self.verify(
                layout.endpoint().parent().ok_or(Error::Layout)?,
                Policy::EndpointDirectory,
                false,
            )?;
            let (p, n) = self.parent(layout.config())?.ok_or(Error::Layout)?;
            let mut f = self.open_at(&p, &n, false)?.ok_or(Error::Layout)?;
            let bytes = self.bytes(&mut f, 0o600, crate::service::config::MAX_CONFIG_BYTES)?;
            self.same(&p, &n, &f)?;
            ServiceConfig::parse_inventory(&bytes, layout, resolver)?;
            self.verify(layout.state(), Policy::PrivateDirectory, true)?;
            let (p, _) = self
                .parent(&layout.state().join("activity.lock"))?
                .ok_or(Error::Layout)?;
            self.state_members(&p, true)
        }
        fn provision(&self, state: &Path) -> Result<(), Error> {
            self.verify(state, Policy::PrivateDirectory, true)?;
            let (p, _) = self
                .parent(&state.join("activity.lock"))?
                .ok_or(Error::Layout)?;
            self.state_members(&p, false)?;
            let lock_name = name("activity.lock")?;
            let lock = match self.open_at(&p, &lock_name, false)? {
                Some(f) => f,
                None => self.create(&p, &lock_name, &[], 0o600)?,
            };
            if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
                return Err(Error::Permissions);
            }
            self.same(&p, &lock_name, &lock)?;
            self.state_members(&p, false)?;
            for n in ["activity.sqlite3", "activity.sqlite3-journal"] {
                let n = name(n)?;
                if self.open_at(&p, &n, false)?.is_none() {
                    self.create(&p, &n, &[], 0o600)?;
                }
            }
            self.state_members(&p, true)?;
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
            if matches!(self.runner, Runner::Native) {
                identity()?;
            }
            allowed(&self.layout, op)?;
            match op {
                Operation::StartPreflight => self.fs.preflight(&self.layout, &NativeResolver),
                Operation::ProvisionState => self.fs.provision(self.layout.state()),
                Operation::Expect(expect) => {
                    let (q, a) = self.observe()?;
                    q.expect(*expect, a, self.layout.platform())
                }
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
                Operation::Constant { path, bytes, when } => {
                    if *when == ConstantGate::UnitAbsent {
                        let (q, a) = self.observe()?;
                        q.gate(Gate::Always, a)?;
                        if a != Artifact::Absent {
                            self.skipped += 1;
                            return Ok(());
                        }
                    }
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
                Operation::Command {
                    program,
                    args,
                    when,
                } => {
                    if *when != Gate::Always {
                        let (q, a) = self.observe()?;
                        if !q.gate(*when, a)? {
                            self.skipped += 1;
                            return Ok(());
                        }
                    }
                    self.runner.command(&self.fs, program, args)
                }
                _ => Err(Error::NativeAdapterMissing),
            }
        }
    }
    enum Runner {
        Native,
        #[cfg(test)]
        Script {
            queries: std::collections::VecDeque<Query>,
            results: std::collections::VecDeque<Result<(), Error>>,
            calls: Vec<Vec<String>>,
        },
    }
    impl Runner {
        fn query(&mut self, fs: &Fs, platform: Platform) -> Query {
            #[cfg(test)]
            if let Self::Script { queries, calls, .. } = self {
                calls.push(vec!["query".into()]);
                return queries.pop_front().expect("unexpected query");
            }
            let (program, args, cap) = match platform {
                Platform::Linux => ("/usr/bin/systemctl", owner_query::SYSTEMD_QUERY, 4096),
                Platform::Macos => ("/bin/launchctl", owner_query::LAUNCHD_QUERY, 65536),
                _ => return Query::Unknown,
            };
            if fs
                .verify(Path::new(program), Policy::BinaryIntegrity, true)
                .is_err()
            {
                return Query::Unknown;
            }
            match bounded(program, args, Some(cap), Duration::from_secs(10)) {
                Ok(bytes) if platform == Platform::Linux => owner_query::systemd(true, &bytes),
                Ok(bytes) => owner_query::launchd(true, &bytes),
                Err(_) => Query::Unknown,
            }
        }
        fn command(&mut self, fs: &Fs, program: &str, args: &[&str]) -> Result<(), Error> {
            #[cfg(test)]
            if let Self::Script { results, calls, .. } = self {
                calls.push(args.iter().map(|s| (*s).to_owned()).collect());
                return results.pop_front().expect("unexpected command");
            }
            fs.verify(Path::new(program), Policy::BinaryIntegrity, true)?;
            bounded(program, args, None, Duration::from_secs(35)).map(|_| ())
        }
    }
    // Nonblocking pipe reads keep timeout/reaping independent of inherited pipe
    // handles. Retained output is capped, and no child output escapes this module.
    fn bounded(
        program: &str,
        args: &[&str],
        cap: Option<usize>,
        timeout: Duration,
    ) -> Result<Vec<u8>, Error> {
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
            .stdout(if cap.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| Error::Manager)?;
        let result = (|| {
            if let Some(pipe) = &child.stdout
                && unsafe { libc::fcntl(pipe.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK) } < 0
            {
                return Err(Error::Manager);
            }
            let deadline = Instant::now() + timeout;
            let mut output = Vec::new();
            let mut exited = None;
            loop {
                let mut eof = cap.is_none();
                if let Some(pipe) = child.stdout.as_mut() {
                    let mut buf = [0; 1024];
                    // Bound each pass too: a noisy child cannot starve the deadline.
                    for _ in 0..65 {
                        match pipe.read(&mut buf) {
                            Ok(0) => {
                                eof = true;
                                break;
                            }
                            Ok(n) => {
                                if output.len() + n > cap.unwrap_or(0) {
                                    return Err(Error::Manager);
                                }
                                output.extend_from_slice(&buf[..n]);
                            }
                            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                            Err(_) => return Err(Error::Manager),
                        }
                    }
                }
                if exited.is_none() {
                    exited = child.try_wait().map_err(|_| Error::Manager)?;
                }
                if let Some(status) = exited {
                    if !status.success() {
                        return Err(Error::Manager);
                    }
                    if eof {
                        return Ok(output);
                    }
                }
                if Instant::now() >= deadline {
                    return Err(Error::Manager);
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        })();
        if result.is_err() {
            let _ = child.kill();
            let deadline = Instant::now() + Duration::from_secs(1);
            while Instant::now() < deadline {
                if matches!(child.try_wait(), Ok(Some(_))) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        result
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
        fn another_member_group() -> u32 {
            let mut groups = [0 as libc::gid_t; 1024];
            let n = unsafe { libc::getgroups(groups.len() as i32, groups.as_mut_ptr()) };
            assert!(n >= 0, "fixture groups available");
            groups[..n as usize]
                .iter()
                .copied()
                .find(|g| *g != unsafe { libc::getegid() })
                .expect("fixture requires another own member group")
        }
        #[test]
        fn ancestor_group_role_and_new_object_ownership() {
            let (dir, fs) = fixture();
            let other = another_member_group();
            let traversal = dir.path().join("traversal");
            std::fs::create_dir(&traversal).unwrap();
            std::fs::set_permissions(&traversal, std::fs::Permissions::from_mode(0o755)).unwrap();
            let parent = File::open(&traversal).unwrap();
            assert!(unsafe { libc::fchown(parent.as_raw_fd(), fs.uid, other) } == 0);
            assert!(fs.check(&parent, true, None).is_err()); // binary leaf stays strict
            assert!(fs.check(&parent, true, Some(0o700)).is_err());
            assert!(fs.check(&parent, true, Some(0o755)).is_err());
            let accepted = fs.parent(Path::new("/traversal/child")).is_ok();
            assert_eq!(accepted, native::os::PLATFORM == Platform::Macos);
            for bits in [0o775, 0o757, 0o1755, 0o2755, 0o4755] {
                std::fs::set_permissions(&traversal, std::fs::Permissions::from_mode(bits))
                    .unwrap();
                assert!(fs.parent(Path::new("/traversal/child")).is_err());
            }
            std::fs::set_permissions(&traversal, std::fs::Permissions::from_mode(0o755)).unwrap();
            #[cfg(target_os = "macos")]
            {
                fs.directory(Path::new("/traversal/child"), 0o755).unwrap();
                let child = traversal.join("child");
                let before = std::fs::metadata(&child).unwrap();
                assert!(
                    before.uid() == fs.uid
                        && before.gid() == fs.gid
                        && before.mode() & 0o7777 == 0o755
                );
                fs.directory(Path::new("/traversal/child"), 0o755).unwrap();
                assert!(std::fs::metadata(&child).unwrap().ino() == before.ino());
                fs.create(&parent, &name("new-file").unwrap(), b"fixed", 0o600)
                    .unwrap();
                let m = std::fs::metadata(traversal.join("new-file")).unwrap();
                assert!(m.uid() == fs.uid && m.gid() == fs.gid && m.mode() & 0o7777 == 0o600);
                assert!(
                    Command::new("/bin/chmod")
                        .args(["+a", "everyone allow read"])
                        .arg(&traversal)
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .status()
                        .unwrap()
                        .success()
                );
                assert!(fs.parent(Path::new("/traversal/child")).is_err());
                assert!(
                    Command::new("/bin/chmod")
                        .arg("-N")
                        .arg(&traversal)
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .status()
                        .unwrap()
                        .success()
                );
            }
            // Existing mismatched child is refused without changing ownership or mode.
            fs.directory(Path::new("/existing"), 0o755).unwrap();
            let child = File::open(dir.path().join("existing")).unwrap();
            assert!(unsafe { libc::fchown(child.as_raw_fd(), fs.uid, other) } == 0);
            let before = child.metadata().unwrap();
            assert!(fs.directory(Path::new("/existing"), 0o755).is_err());
            let after = child.metadata().unwrap();
            assert!(
                before.ino() == after.ino()
                    && before.gid() == after.gid()
                    && before.mode() == after.mode()
            );
            symlink("traversal", dir.path().join("alias")).unwrap();
            assert!(fs.parent(Path::new("/alias/child")).is_err());
            std::fs::write(dir.path().join("regular"), b"fixed").unwrap();
            assert!(fs.parent(Path::new("/regular/child")).is_err());
        }
        fn manager(
            platform: Platform,
            queries: Vec<Query>,
            results: Vec<Result<(), Error>>,
        ) -> (tempfile::TempDir, Manager) {
            let (dir, fs) = fixture();
            let layout = Layout::for_test(platform);
            let mut paths = vec![layout.artifact().parent().unwrap().to_path_buf()];
            if platform == Platform::Linux {
                paths.push(Path::new(DROPIN_PATH).parent().unwrap().to_path_buf());
            }
            for path in paths {
                let mut current = std::path::PathBuf::from("/");
                for part in parts(&path).unwrap() {
                    current.push(part);
                    fs.directory(&current, 0o755).unwrap();
                }
            }
            (
                dir,
                Manager {
                    layout,
                    fs,
                    skipped: 0,
                    runner: Runner::Script {
                        queries: queries.into(),
                        results: results.into(),
                        calls: vec![],
                    },
                },
            )
        }
        fn cmd(platform: Platform, verb: &'static str, when: Gate) -> Operation {
            Operation::Command {
                program: if platform == Platform::Linux {
                    "/usr/bin/systemctl"
                } else {
                    "/bin/launchctl"
                },
                args: if verb == "bootstrap" {
                    vec![verb, "system", owner_query::PLIST_PATH]
                } else if platform == Platform::Linux {
                    vec![verb, "arxd.service"]
                } else {
                    vec![verb, owner_query::LAUNCHD_TARGET]
                },
                when,
            }
        }
        fn calls(m: &Manager) -> Vec<Vec<String>> {
            match &m.runner {
                Runner::Script { calls, .. } => calls.clone(),
                _ => panic!("script only"),
            }
        }
        #[test]
        fn scripted_whole_uninstall_orders_commands_and_removes_only_after_proof() {
            use super::super::super::owner_plan::{Action, OwnerContext, owner_plan};
            for platform in [Platform::Linux, Platform::Macos] {
                let q = Query::Ours(if platform == Platform::Linux {
                    owner_query::State::Stopped
                } else {
                    owner_query::State::Loaded
                });
                let queries = if platform == Platform::Linux {
                    vec![q, q, q, Query::Absent]
                } else {
                    vec![q, q, Query::Absent]
                };
                let (_dir, mut m) = manager(platform, queries, vec![Ok(()), Ok(()), Ok(())]);
                let mut current = std::path::PathBuf::from("/");
                for part in parts(m.layout.binary().parent().unwrap()).unwrap() {
                    current.push(part);
                    m.fs.directory(&current, 0o755).unwrap();
                }
                m.fs.constant(m.layout.binary(), b"fixture binary", false)
                    .unwrap();
                for path in m.layout.private_dirs() {
                    let local = _dir.path().join(path.strip_prefix("/").unwrap());
                    if local.exists() {
                        std::fs::set_permissions(local, std::fs::Permissions::from_mode(0o700))
                            .unwrap();
                    }
                }

                let bytes = if platform == Platform::Linux {
                    UNIT
                } else {
                    PLIST
                };
                m.fs.constant(m.layout.artifact(), bytes, false).unwrap();
                let plan = owner_plan(
                    &m.layout,
                    &OwnerContext {
                        admin: true,
                        running_binary: m.layout.binary(),
                    },
                    Action::Uninstall,
                )
                .unwrap();
                m.execute(plan.operations()).unwrap();
                assert_eq!(
                    m.fs.artifact(m.layout.artifact(), bytes).unwrap(),
                    Artifact::Absent
                );
                let observed = calls(&m);
                let verbs: Vec<_> = observed.iter().map(|v| v[0].as_str()).collect();
                assert_eq!(
                    verbs,
                    if platform == Platform::Linux {
                        vec![
                            "query",
                            "query",
                            "disable",
                            "query",
                            "stop",
                            "query",
                            "daemon-reload",
                        ]
                    } else {
                        vec!["query", "disable", "query", "bootout", "query"]
                    }
                );
            }
        }
        #[test]
        fn scripted_fresh_gates_skip_and_bootstrap_once() {
            let (_dir, mut m) = manager(
                Platform::Macos,
                vec![Query::Absent, Query::Ours(owner_query::State::Loaded)],
                vec![Ok(())],
            );
            let op = cmd(Platform::Macos, "bootstrap", Gate::NotLoaded);
            m.execute(&[op.clone(), op]).unwrap();
            assert_eq!(m.skipped, 1);
            assert_eq!(
                calls(&m),
                vec![
                    vec!["query"],
                    vec!["bootstrap", "system", owner_query::PLIST_PATH],
                    vec!["query"]
                ]
            );
        }
        #[test]
        fn scripted_halts_on_unknown_foreign_active_and_stop_proof() {
            for (q, error) in [
                (Query::Unknown, Error::Manager),
                (Query::Foreign, Error::Foreign),
                (Query::Ours(owner_query::State::Running), Error::Active),
            ] {
                let (_dir, mut m) = manager(Platform::Linux, vec![q], vec![]);
                assert_eq!(
                    m.execute(&[
                        Operation::Expect(owner_query::Expect::Installable),
                        cmd(Platform::Linux, "disable", Gate::Always)
                    ]),
                    Err(error)
                );
                assert_eq!(calls(&m), vec![vec!["query"]]);
            }
            for q in [Query::Unknown, Query::Ours(owner_query::State::Running)] {
                let (_dir, mut m) = manager(Platform::Linux, vec![q], vec![Ok(())]);
                m.fs.constant(m.layout.artifact(), UNIT, false).unwrap();
                let ops = [
                    cmd(Platform::Linux, "stop", Gate::Always),
                    Operation::Expect(owner_query::Expect::Unloaded),
                    Operation::RemoveConstant {
                        path: m.layout.artifact().into(),
                        expected: UNIT,
                    },
                ];
                assert_eq!(m.execute(&ops), Err(Error::Manager));
                assert_eq!(
                    m.fs.artifact(m.layout.artifact(), UNIT).unwrap(),
                    Artifact::Identical
                );
                assert_eq!(calls(&m), vec![vec!["stop", "arxd.service"], vec!["query"]]);
            }
        }
        #[test]
        fn scripted_command_failure_halts_suffix_and_allowlist_scanned_first() {
            let (_dir, mut m) = manager(Platform::Linux, vec![], vec![Err(Error::Manager)]);
            let stop = cmd(Platform::Linux, "stop", Gate::Always);
            assert_eq!(
                m.execute(&[stop.clone(), stop.clone()]),
                Err(Error::Manager)
            );
            assert_eq!(calls(&m).len(), 1);
            let (_dir, mut m) = manager(Platform::Linux, vec![], vec![]);
            let forbidden = Operation::EnsureDirectory {
                path: "/outside".into(),
                policy: Policy::PrivateDirectory,
            };
            assert_eq!(m.execute(&[stop, forbidden]), Err(Error::Layout));
            assert!(calls(&m).is_empty());
        }
        #[test]
        fn scripted_constants_check_all_artifacts_and_preserve_installed_choice() {
            let (_dir, mut m) = manager(
                Platform::Linux,
                vec![Query::Ours(owner_query::State::Stopped)],
                vec![],
            );
            m.fs.constant(m.layout.artifact(), UNIT, false).unwrap();
            m.execute(&[Operation::Constant {
                path: DROPIN_PATH.into(),
                bytes: DROPIN,
                when: ConstantGate::UnitAbsent,
            }])
            .unwrap();
            assert_eq!(
                m.fs.artifact(Path::new(DROPIN_PATH), DROPIN).unwrap(),
                Artifact::Absent
            );
            m.fs.constant(Path::new(DROPIN_PATH), b"foreign", false)
                .unwrap();
            assert_eq!(m.status(), Err(Error::Foreign));
        }
        #[test]
        fn ordinary_process_timeout_and_output_bounds() {
            assert_ne!(unsafe { libc::geteuid() }, 0);
            let start = Instant::now();
            assert_eq!(
                bounded("/bin/sleep", &["5"], None, Duration::from_millis(100)),
                Err(Error::Manager)
            );
            assert!(start.elapsed() < Duration::from_secs(2));
            assert_eq!(
                bounded("/bin/echo", &["1234"], Some(4), Duration::from_secs(1)),
                Err(Error::Manager)
            );
            assert_eq!(
                bounded("/bin/echo", &["1234"], Some(5), Duration::from_secs(1)).unwrap(),
                b"1234\n"
            );
            assert_eq!(
                bounded("/bin/false", &[], None, Duration::from_secs(1)),
                Err(Error::Manager)
            );
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
        fn header() -> Vec<u8> {
            let mut h = b"SQLite format 3\0".to_vec();
            h.extend_from_slice(&[0, 0, 1, 1]);
            h
        }
        #[test]
        fn retained_and_partial_state_preserve_existing_objects() {
            for retained in [false, true] {
                for missing in [
                    "activity.lock",
                    "activity.sqlite3",
                    "activity.sqlite3-journal",
                ] {
                    let (dir, fs) = fixture();
                    fs.directory(Path::new("/state"), 0o700).unwrap();
                    let mut before = Vec::new();
                    for n in [
                        "activity.lock",
                        "activity.sqlite3",
                        "activity.sqlite3-journal",
                    ] {
                        if n == missing {
                            continue;
                        }
                        let bytes = if retained && n == "activity.sqlite3" {
                            header()
                        } else if retained && n.ends_with("journal") {
                            b"retained recovery journal".to_vec()
                        } else {
                            vec![]
                        };
                        let (p, leaf) = fs.parent(&Path::new("/state").join(n)).unwrap().unwrap();
                        fs.create(&p, &leaf, &bytes, 0o600).unwrap();
                        let path = dir.path().join("state").join(n);
                        before.push((path.clone(), bytes, std::fs::metadata(path).unwrap()));
                    }
                    fs.provision(Path::new("/state")).unwrap();
                    for (path, bytes, old) in before {
                        assert_eq!(std::fs::read(&path).unwrap(), bytes);
                        let new = std::fs::metadata(path).unwrap();
                        assert_eq!(
                            (new.ino(), new.mode(), new.uid(), new.gid()),
                            (old.ino(), old.mode(), old.uid(), old.gid())
                        );
                    }
                    assert!(dir.path().join("state").join(missing).exists());
                }
            }
        }
        #[test]
        fn unsafe_state_refuses_before_creating_missing_members() {
            for bad in [
                "wal", "shm", "header", "mode", "symlink", "hardlink", "lock", "held",
            ] {
                let (dir, fs) = fixture();
                fs.directory(Path::new("/state"), 0o700).unwrap();
                let base = dir.path().join("state");
                let n = match bad {
                    "wal" => "activity.sqlite3-wal",
                    "shm" => "activity.sqlite3-shm",
                    "lock" | "held" => "activity.lock",
                    _ => "activity.sqlite3",
                };
                let (p, leaf) = fs.parent(&Path::new("/state").join(n)).unwrap().unwrap();
                let f = fs
                    .create(
                        &p,
                        &leaf,
                        if matches!(bad, "header" | "lock") {
                            b"bad"
                        } else {
                            b""
                        },
                        0o600,
                    )
                    .unwrap();
                if bad == "mode" {
                    std::fs::set_permissions(base.join(n), std::fs::Permissions::from_mode(0o644))
                        .unwrap();
                }
                if bad == "hardlink" {
                    std::fs::hard_link(base.join(n), base.join("alias")).unwrap();
                }
                if bad == "symlink" {
                    std::fs::remove_file(base.join(n)).unwrap();
                    symlink("absent", base.join(n)).unwrap();
                }
                if bad == "held" {
                    assert_eq!(
                        unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
                        0
                    );
                }
                assert!(fs.provision(Path::new("/state")).is_err(), "{bad}");
                assert!(!base.join("activity.sqlite3-journal").exists());
                if n != "activity.lock" {
                    assert!(!base.join("activity.lock").exists());
                }
            }
        }
        struct FakeResolver;
        impl crate::service::config::Resolver for FakeResolver {
            fn account(&self, value: &str) -> Result<crate::service::config::Account, Error> {
                if value != "1001" {
                    return Err(Error::AccountMissing);
                }
                Ok(crate::service::config::Account {
                    identity: value.into(),
                    gid: Some(1001),
                    kind: crate::service::config::AccountKind::User,
                    administrative: false,
                })
            }
            fn workspace_safe(&self, _: &str) -> Result<bool, Error> {
                panic!("preflight must not observe workspace")
            }
        }
        #[test]
        fn preflight_optional_endpoint_and_static_inventory() {
            let (dir, fs) = fixture();
            let layout = Layout::for_test(Platform::Linux);
            for path in [
                layout.binary().parent().unwrap(),
                layout.config().parent().unwrap(),
                layout.state(),
                layout.endpoint().parent().unwrap().parent().unwrap(),
            ] {
                let mut current = std::path::PathBuf::from("/");
                for part in parts(path).unwrap() {
                    current.push(part);
                    let mode = if layout.private_dirs().contains(&current) {
                        0o700
                    } else {
                        0o755
                    };
                    fs.directory(&current, mode).unwrap();
                }
            }
            let config = serde_json::json!({"schema":"arxd-service/1","client":{"identity":"1001"},"build_account":{"name":"missing-builder","identity":"1002","gid":1002},"rights":"standard","workspace":"/work","environment":{"PATH":"/usr/bin"},"limits":{"max_concurrent_runs":1,"memory_bytes":1024,"pids":32},"agents":[{"principal":"test","token_sha256":"a".repeat(64)}],"granted_batch_right":false});
            for (path, bytes, mode) in [
                (layout.config(), serde_json::to_vec(&config).unwrap(), 0o600),
                (layout.binary(), b"binary".to_vec(), 0o755),
            ] {
                let (p, n) = fs.parent(path).unwrap().unwrap();
                fs.create(&p, &n, &bytes, mode).unwrap();
            }
            fs.provision(layout.state()).unwrap();
            assert_eq!(fs.preflight(&layout, &FakeResolver), Ok(()));
            // Each protected prerequisite is required, independently of endpoint absence.
            for path in [
                layout.binary(),
                layout.config(),
                &layout.state().join("activity.lock"),
            ] {
                let actual = dir.path().join(path.strip_prefix("/").unwrap());
                let held = actual.with_extension("fixture-held");
                std::fs::rename(&actual, &held).unwrap();
                assert!(fs.preflight(&layout, &FakeResolver).is_err());
                std::fs::rename(held, actual).unwrap();
            }
            let endpoint = layout.endpoint().parent().unwrap();
            fs.directory(endpoint, 0o755).unwrap();
            assert_eq!(fs.preflight(&layout, &FakeResolver), Ok(()));
            let actual = dir.path().join(endpoint.strip_prefix("/").unwrap());
            std::fs::set_permissions(&actual, std::fs::Permissions::from_mode(0o777)).unwrap();
            assert!(fs.preflight(&layout, &FakeResolver).is_err());
            assert_eq!(std::fs::metadata(&actual).unwrap().mode() & 0o777, 0o777);
            std::fs::remove_dir(&actual).unwrap();
            symlink("missing", &actual).unwrap();
            assert!(fs.preflight(&layout, &FakeResolver).is_err());
            std::fs::remove_file(&actual).unwrap();
            std::fs::write(&actual, b"file").unwrap();
            assert!(fs.preflight(&layout, &FakeResolver).is_err());
            std::fs::remove_file(&actual).unwrap();
            std::fs::write(
                dir.path()
                    .join(layout.state().strip_prefix("/").unwrap())
                    .join("activity.sqlite3"),
                b"invalid",
            )
            .unwrap();
            assert!(fs.preflight(&layout, &FakeResolver).is_err());
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
            assert_eq!(super::super::status(), Err(Error::OwnerRequired));
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
    fn command_gates_and_foreign_targets_are_exact() {
        let gates = [
            Gate::Always,
            Gate::Loaded,
            Gate::NotLoaded,
            Gate::ArtifactPresent,
        ];
        for (os, program, args, permitted) in [
            (
                Platform::Linux,
                "/usr/bin/systemctl",
                vec!["stop", "arxd.service"],
                vec![Gate::Always, Gate::Loaded],
            ),
            (
                Platform::Linux,
                "/usr/bin/systemctl",
                vec!["disable", "arxd.service"],
                vec![Gate::Always, Gate::ArtifactPresent],
            ),
            (
                Platform::Macos,
                "/bin/launchctl",
                vec!["bootout", super::super::owner_query::LAUNCHD_TARGET],
                vec![Gate::Loaded],
            ),
        ] {
            let layout = Layout::for_test(os);
            for when in gates {
                assert_eq!(command(os, program, &args, when), permitted.contains(&when));
                assert_eq!(
                    allowed(
                        &layout,
                        &Operation::Command {
                            program,
                            args: args.clone(),
                            when
                        }
                    )
                    .is_ok(),
                    permitted.contains(&when)
                );
            }
        }
        for when in gates {
            assert!(!command(
                Platform::Linux,
                "/usr/bin/systemctl",
                &["enable", "other.service"],
                when
            ));
            assert!(!command(
                Platform::Macos,
                "/bin/launchctl",
                &["disable", "system/other"],
                when
            ));
        }
    }
    #[test]
    fn only_linux_dropin_accepts_a_conditional_constant() {
        let linux = Layout::for_test(Platform::Linux);
        let macos = Layout::for_test(Platform::Macos);
        for (layout, path, bytes, expected) in [
            (&linux, Path::new(DROPIN_PATH), DROPIN, true),
            (&linux, linux.artifact(), UNIT, false),
            (&macos, macos.artifact(), PLIST, false),
        ] {
            assert_eq!(
                allowed(
                    layout,
                    &Operation::Constant {
                        path: path.into(),
                        bytes,
                        when: ConstantGate::UnitAbsent
                    }
                )
                .is_ok(),
                expected
            );
            assert!(
                allowed(
                    layout,
                    &Operation::Constant {
                        path: path.into(),
                        bytes,
                        when: ConstantGate::Always
                    }
                )
                .is_ok()
            );
        }
    }
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
                    if matches!(
                        op,
                        Operation::Constant {
                            when: ConstantGate::UnitAbsent,
                            ..
                        }
                    ) {
                        assert!(action == Action::Install);
                    }
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
