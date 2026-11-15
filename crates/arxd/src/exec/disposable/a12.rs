//! Dormant A12 producer fixture. Portable tests use only injected observations.
use super::*;
use std::{panic::AssertUnwindSafe, time::Instant};

// A source gate, never an environment variable or delivery-file permission.
const DELIVERY_ENABLED: bool = false;
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Pin {
    dev: u64,
    ino: u64,
    mode: u32,
    uid: u32,
    gid: u32,
    nlink: u64,
    size: u64,
    sha256: String,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Delivery {
    version: u32,
    run: String,
    source_sha: String,
    workflow_sha: String,
    uid: u32,
    gid: u32,
    root: Pin,
    workspace: Pin,
    harness: Pin,
}
fn hex(s: &str, n: usize) -> bool {
    s.len() == n
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn layout(exe: &Path) -> Option<(PathBuf, String, String)> {
    let name = exe.file_name()?.to_str()?.strip_prefix("arxd-")?;
    if !hex(name, name.len()) || name.is_empty() {
        return None;
    }
    let stage = exe.parent()?.parent()?.parent()?.parent()?.parent()?;
    if exe.parent()? != stage.join("source/target/debug/deps") {
        return None;
    }
    let run = stage.file_name()?.to_str()?.strip_prefix("arx-ci-")?;
    let (id, attempt) = run.split_once('-')?;
    let decimal = |s: &str, max| {
        !s.is_empty()
            && s.len() <= max
            && !s.starts_with('0')
            && s.bytes().all(|b| b.is_ascii_digit())
    };
    if stage.parent()? != Path::new("/var/lib") || !decimal(id, 20) || !decimal(attempt, 6) {
        return None;
    }
    let account = format!("arxa12{id}x{attempt}");
    (account.len() <= 32).then(|| (stage.to_owned(), run.to_owned(), account))
}
fn decode(bytes: &[u8], run: &str, source: &str) -> Option<Delivery> {
    if bytes.len() > 4096 {
        return None;
    }
    let d: Delivery = serde_json::from_slice(bytes).ok()?;
    (d.version == 1
        && d.run == run
        && hex(source, 40)
        && d.source_sha == source
        && d.workflow_sha == source
        && d.uid >= 1000
        && d.gid >= 1000
        && d.root.uid == d.uid
        && d.root.gid == d.gid
        && d.root.mode == 0o40700
        && d.workspace.uid == d.uid
        && d.workspace.gid == d.gid
        && d.workspace.mode == 0o40700
        && d.harness.uid == 0
        && d.harness.gid == 0
        && d.harness.mode == 0o100700
        && d.harness.nlink == 1
        && (4..=128 * 1024 * 1024).contains(&d.harness.size)
        && hex(&d.harness.sha256, 64)
        && d.root.ino > 0
        && d.workspace.ino > 0
        && d.harness.ino > 0)
        .then_some(d)
}
fn closed_read<T>(reader: impl FnOnce() -> Option<T>) -> Option<T> {
    if !DELIVERY_ENABLED {
        return None;
    }
    reader()
}
fn owned_pins(d: &Delivery, root: &Pin, workspace: &Pin, harness: &Pin, target: &Pin) -> bool {
    root == &d.root
        && workspace == &d.workspace
        && harness == &d.harness
        && target.uid == 0
        && target.mode & 0o170000 == 0o100000
        && target.mode & 0o6022 == 0
        && target.mode & 0o111 != 0
        && target.nlink == 1
}
fn runs_identity(expected: (u64, u64), observed: (u64, u64), mode: u32, uid: u32) -> bool {
    expected.1 > 0
        && expected == observed
        && mode & 0o170000 == 0o040000
        && mode & 0o022 == 0
        && uid == 0
}
fn record_bytes(
    reader: impl std::io::Read,
    mode: u32,
    uid: u32,
    nlink: u64,
    size: u64,
) -> Option<Vec<u8>> {
    use std::io::Read;
    if mode != 0o100600 || uid != 0 || nlink != 1 || size > 4096 {
        return None;
    }
    let mut bytes = vec![];
    reader.take(4097).read_to_end(&mut bytes).ok()?;
    (bytes.len() as u64 == size && bytes.len() <= 4096).then_some(bytes)
}
fn checked_call(
    recheck: impl FnOnce() -> Option<()>,
    call: impl FnOnce() -> Result<RunResult, RunError>,
) -> Result<RunResult, RunError> {
    recheck().ok_or(RunError::NotStarted(NotStarted::Invalid))?;
    call()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Case {
    Normal,
    Cancel,
}
impl Case {
    fn spec(self) -> RunSpec {
        RunSpec {
            executable: match self {
                Self::Normal => "/usr/bin/true",
                Self::Cancel => "/usr/bin/sleep",
            }
            .into(),
            arguments: match self {
                Self::Normal => vec![],
                Self::Cancel => vec!["15".into()],
            },
            cwd: PathBuf::new(),
            timeout: Duration::from_secs(15),
            output_bytes: 1024,
        }
    }
}

#[derive(Clone, Copy)]
struct Population {
    before: (u64, u64),
    after: (u64, u64),
    pid: u32,
    populated: bool,
    owned: bool,
}
impl Population {
    fn proven(self) -> bool {
        self.owned
            && self.populated
            && self.pid > 0
            && self.before.1 > 0
            && self.before == self.after
    }
}

#[allow(clippy::too_many_arguments)] // Explicit independent counterfactual facts, test-only.
fn accepts(
    case: Case,
    run: &Result<RunResult, RunError>,
    expected: (u32, u32),
    post_spawn: bool,
    joined: bool,
    residue: bool,
    absent: bool,
    elapsed: Duration,
) -> bool {
    let Ok(run) = run else { return false };
    joined
        && absent
        && !residue
        && elapsed <= Duration::from_secs(25)
        && (case == Case::Normal || post_spawn)
        && run.outcome
            == match case {
                Case::Normal => Outcome::Exited,
                Case::Cancel => Outcome::Cancelled,
            }
        && run.exit_code == if case == Case::Normal { Some(0) } else { None }
        && run.identity
            == (ObservedIdentity {
                uid: expected.0,
                gid: expected.1,
                daemon_was_root: true,
            })
        && run.tree_boundary == TreeBoundary::LinuxCgroupV2
        && run.tree_cleanup_confirmed
}

// One owned call, with cancellation on observation failure/panic before joining.
// Scoped joins deliberately retain ownership even if synchronous OS calls stall.
// Elapsed checks reject late results; they are not hard OS interruption claims.
fn exercise(
    case: Case,
    expected: (u32, u32),
    work: impl FnOnce(&Cancellation) -> Result<RunResult, RunError> + Send,
    mut population: impl FnMut() -> Result<Option<Population>, ()>,
    after_join: impl FnOnce() -> (bool, bool), // residue, literal absence
) -> bool {
    let start = Instant::now();
    let token = Cancellation::default();
    std::thread::scope(|scope| {
        let call_token = &token;
        let worker = scope.spawn(move || work(call_token));
        let proof = std::panic::catch_unwind(AssertUnwindSafe(|| {
            if case == Case::Normal {
                return true;
            }
            while start.elapsed() < Duration::from_secs(5) && !worker.is_finished() {
                match population() {
                    Ok(Some(p)) if p.proven() => return true,
                    Ok(None) => std::thread::sleep(Duration::from_millis(5)),
                    _ => return false,
                }
            }
            false
        }))
        .unwrap_or(false);
        if case == Case::Cancel || !proof {
            token.cancel();
        }
        // No assertions or fallible postcondition reads before this explicit join.
        let result = worker.join();
        let joined = result.is_ok();
        let result = result.unwrap_or(Err(RunError::AfterSpawn));
        let (residue, absent) = after_join();
        accepts(
            case,
            &result,
            expected,
            proof,
            joined,
            residue,
            absent,
            start.elapsed(),
        )
    })
}

#[cfg(target_os = "linux")]
mod native {
    use super::*;
    use crate::exec::hierarchy::setup::CgroupFs;
    use std::{cell::RefCell, fs::File, os::fd::AsRawFd, os::unix::fs::MetadataExt};

    struct Prepared {
        config: OwnerConfig,
        exe: PathBuf,
        delivery: Delivery,
    }
    fn prepared_delivery() -> Option<Prepared> {
        closed_read(read_delivery)
    }
    fn root_open(path: &Path) -> Option<File> {
        // Component-wise O_NOFOLLOW with root-owned, nonwritable ancestry.
        use std::{ffi::CString, os::fd::FromRawFd};
        if !path.is_absolute() {
            return None;
        }
        let mut file = File::open("/").ok()?;
        for part in path.components().skip(1) {
            let m = file.metadata().ok()?;
            if !m.is_dir() || m.uid() != 0 || m.mode() & 0o022 != 0 {
                return None;
            }
            let std::path::Component::Normal(part) = part else {
                return None;
            };
            let name = CString::new(part.as_encoded_bytes()).ok()?;
            let fd = unsafe {
                libc::openat(
                    file.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
                )
            };
            if fd < 0 {
                return None;
            }
            file = unsafe { File::from_raw_fd(fd) };
        }
        Some(file)
    }
    fn pin(path: &Path, binary: bool) -> Option<Pin> {
        use sha2::{Digest, Sha256};
        use std::io::Read;
        let mut file = root_open(path)?;
        let m = file.metadata().ok()?;
        let sha256 = if binary {
            if !m.is_file() || m.len() > 128 * 1024 * 1024 {
                return None;
            }
            let mut h = Sha256::new();
            let count = std::io::copy(&mut (&mut file).take(128 * 1024 * 1024 + 1), &mut h).ok()?;
            if count != m.len() {
                return None;
            }
            format!("{:x}", h.finalize())
        } else {
            String::new()
        };
        let after = file.metadata().ok()?;
        if (
            m.dev(),
            m.ino(),
            m.mode(),
            m.uid(),
            m.gid(),
            m.len(),
            m.mtime(),
            m.ctime(),
        ) != (
            after.dev(),
            after.ino(),
            after.mode(),
            after.uid(),
            after.gid(),
            after.len(),
            after.mtime(),
            after.ctime(),
        ) {
            return None;
        }
        Some(Pin {
            dev: m.dev(),
            ino: m.ino(),
            mode: m.mode(),
            uid: m.uid(),
            gid: m.gid(),
            nlink: m.nlink(),
            size: if binary { m.len() } else { 0 },
            sha256,
        })
    }
    fn workspace_pin(path: &Path, expected_root: &Pin) -> Option<Pin> {
        use std::os::fd::FromRawFd;
        let root = root_open(path.parent()?)?;
        let m = root.metadata().ok()?;
        if (m.dev(), m.ino(), m.mode(), m.uid(), m.gid())
            != (
                expected_root.dev,
                expected_root.ino,
                expected_root.mode,
                expected_root.uid,
                expected_root.gid,
            )
        {
            return None;
        }
        let fd = unsafe {
            libc::openat(
                root.as_raw_fd(),
                c"workspace".as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return None;
        }
        let file = unsafe { File::from_raw_fd(fd) };
        let m = file.metadata().ok()?;
        Some(Pin {
            dev: m.dev(),
            ino: m.ino(),
            mode: m.mode(),
            uid: m.uid(),
            gid: m.gid(),
            nlink: m.nlink(),
            size: 0,
            sha256: String::new(),
        })
    }
    fn private(path: &Path) -> Option<Vec<u8>> {
        let file = root_open(path)?;
        let m = file.metadata().ok()?;
        record_bytes(file, m.mode(), m.uid(), m.nlink(), m.len())
    }
    fn account(name: &str, uid: u32, gid: u32, workspace: &Path) -> Option<()> {
        use std::ffi::{CStr, CString};
        let name = CString::new(name).ok()?;
        let mut pw = std::mem::MaybeUninit::<libc::passwd>::uninit();
        let mut result = std::ptr::null_mut();
        let mut buf = vec![0u8; 16384];
        if unsafe {
            libc::getpwnam_r(
                name.as_ptr(),
                pw.as_mut_ptr(),
                buf.as_mut_ptr().cast(),
                buf.len(),
                &mut result,
            )
        } != 0
            || result.is_null()
        {
            return None;
        }
        let pw = unsafe { pw.assume_init() };
        if pw.pw_uid != uid
            || pw.pw_gid != gid
            || unsafe { CStr::from_ptr(pw.pw_shell) }.to_bytes() != b"/usr/sbin/nologin"
            || unsafe { CStr::from_ptr(pw.pw_dir) }.to_bytes()
                != workspace.as_os_str().as_encoded_bytes()
        {
            return None;
        }
        let mut groups = [0u32; 2];
        let mut n = 2;
        if unsafe { libc::getgrouplist(name.as_ptr(), gid, groups.as_mut_ptr(), &mut n) } < 0
            || n != 1
            || groups[0] != gid
        {
            return None;
        }
        let mut gr = std::mem::MaybeUninit::<libc::group>::uninit();
        let mut result = std::ptr::null_mut();
        if unsafe {
            libc::getgrnam_r(
                name.as_ptr(),
                gr.as_mut_ptr(),
                buf.as_mut_ptr().cast(),
                buf.len(),
                &mut result,
            )
        } != 0
            || result.is_null()
        {
            return None;
        }
        let gr = unsafe { gr.assume_init() };
        if gr.gr_gid != gid || gr.gr_mem.is_null() || !unsafe { *gr.gr_mem }.is_null() {
            return None;
        }
        use std::io::Read;
        for (path, group) in [("/etc/passwd", false), ("/etc/group", true)] {
            let file = root_open(Path::new(path))?;
            let m = file.metadata().ok()?;
            if !m.is_file() || m.uid() != 0 || m.mode() & 0o022 != 0 || m.len() > 1024 * 1024 {
                return None;
            }
            let mut bytes = vec![];
            file.take(1024 * 1024 + 1).read_to_end(&mut bytes).ok()?;
            if bytes.len() > 1024 * 1024 {
                return None;
            }
            let text = std::str::from_utf8(&bytes).ok()?;
            let mut matches = 0;
            for line in text.lines() {
                let fields: Vec<_> = line.split(':').collect();
                if fields.len() != if group { 4 } else { 7 } {
                    return None;
                }
                let id = fields[2].parse::<u32>().ok()?;
                if id == if group { gid } else { uid } {
                    matches += 1;
                    if fields[0].as_bytes() != name.as_bytes() {
                        return None;
                    }
                }
                if !group
                    && fields[3].parse::<u32>().ok()? == gid
                    && fields[0].as_bytes() != name.as_bytes()
                {
                    return None;
                }
                if group
                    && fields[3]
                        .split(',')
                        .any(|member| member.as_bytes() == name.as_bytes())
                {
                    return None;
                }
            }
            if matches != 1 {
                return None;
            }
        }
        Some(())
    }
    fn read_delivery() -> Option<Prepared> {
        let exe = std::env::current_exe().ok()?;
        let (stage, run, name) = layout(&exe)?;
        let stage_info = root_open(&stage)?.metadata().ok()?;
        if stage_info.uid() != 0 || stage_info.gid() != 0 || stage_info.mode() != 0o40700 {
            return None;
        }
        let source = private(&stage.join("a12-source-sha"))?;
        let d = decode(
            &private(&stage.join("a12-delivery.json"))?,
            &run,
            std::str::from_utf8(&source).ok()?,
        )?;
        let config = OwnerConfig {
            account: name.clone(),
            uid: d.uid,
            gid: d.gid,
            workspace: PathBuf::from("/var/lib").join(&name).join("workspace"),
            protected_directories: vec![stage.clone()],
            environment: BTreeMap::from([("PATH".into(), "/usr/bin:/bin".into())]),
            admin: false,
            cgroup_parent: Some(PathBuf::from(
                "/sys/fs/cgroup/system.slice/arxd.service/runs",
            )),
            memory_bytes: 64 * 1024 * 1024,
            pids: 32,
        };
        let prepared = Prepared {
            config,
            exe,
            delivery: d,
        };
        prepared.recheck(false, Case::Normal)?;
        Some(prepared)
    }
    impl Prepared {
        fn recheck(&self, runs: bool, case: Case) -> Option<()> {
            let d = &self.delivery;
            account(&self.config.account, d.uid, d.gid, &self.config.workspace)?;
            if !owned_pins(
                d,
                &pin(self.config.workspace.parent()?, false)?,
                &workspace_pin(&self.config.workspace, &d.root)?,
                &pin(&self.exe, true)?,
                &pin(&case.spec().executable, true)?,
            ) {
                return None;
            }
            if runs {
                let m = root_open(self.config.cgroup_parent.as_ref()?)?
                    .metadata()
                    .ok()?;
                if !m.is_dir() || m.uid() != 0 || m.mode() & 0o022 != 0 {
                    return None;
                }
            }
            Some(())
        }
    }

    struct Observation {
        fs: hierarchy::linux::LinuxFs,
        runs: hierarchy::linux::Dir,
        path: PathBuf,
        parent: File,
        held: Option<(Vec<u8>, File)>,
    }
    impl Observation {
        fn sample(&mut self) -> Result<Option<Population>, ()> {
            use hierarchy::parse::{Entry, classify_entry};
            let entries = self.fs.list(&self.runs, 512).map_err(|_| ())?;
            let mut names = vec![];
            for entry in entries {
                match classify_entry(&entry.name, entry.d_type, entry.fstatat_mode) {
                    Entry::Dot | Entry::Control => (),
                    Entry::Child => names.push(entry.name),
                    _ => return Err(()),
                }
            }
            if names.is_empty() {
                return Ok(None);
            }
            if names.len() != 1 {
                return Err(());
            }
            let name = names.pop().unwrap();
            let text = std::str::from_utf8(&name).map_err(|_| ())?;
            let prefix = format!("arx-{}-", std::process::id());
            if !text
                .strip_prefix(&prefix)
                .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
            {
                return Err(());
            }
            let path = self.path.join(text);
            let held = File::from(super::super::unix::trusted_root(&path).map_err(|_| ())?);
            let before = held.metadata().map_err(|_| ())?;
            let dir = self.fs.open_child(&self.runs, &name).map_err(|_| ())?;
            let procs = self.fs.read(&dir, "cgroup.procs", 4096).map_err(|_| ())?;
            let events = self.fs.read(&dir, "cgroup.events", 4096).map_err(|_| ())?;
            let pids =
                hierarchy::parse::pids(std::str::from_utf8(&procs).map_err(|_| ())?).ok_or(())?;
            let reopened = File::from(super::super::unix::trusted_root(&path).map_err(|_| ())?);
            let after = reopened.metadata().map_err(|_| ())?;
            let identity = (before.dev(), before.ino());
            if let Some((old_name, old)) = &self.held {
                let old = old.metadata().map_err(|_| ())?;
                if *old_name != name || (old.dev(), old.ino()) != identity {
                    return Err(());
                }
            }
            self.held = Some((name, held));
            if pids.is_empty() {
                return Ok(None);
            }
            if pids.len() != 1 {
                return Err(());
            }
            Ok(Some(Population {
                before: identity,
                after: (after.dev(), after.ino()),
                pid: pids[0],
                populated: hierarchy::parse::events(std::str::from_utf8(&events).map_err(|_| ())?)
                    == Some(true),
                owned: true, // exclusive empty runs proven before the sole owned call
            }))
        }
        fn absent(&self) -> bool {
            let empty = hierarchy::fake::a5::empty(&self.fs, &self.runs).is_ok();
            let removed = self.held.as_ref().is_none_or(|(name, held)| {
                let name = std::ffi::CString::new(name.as_slice()).unwrap();
                let mut st = std::mem::MaybeUninit::<libc::stat>::uninit();
                // Keep the inode descriptor until literal no-follow absence is observed.
                let _ = held.as_raw_fd();
                (unsafe {
                    libc::fstatat(
                        self.runs_fd(),
                        name.as_ptr(),
                        st.as_mut_ptr(),
                        libc::AT_SYMLINK_NOFOLLOW,
                    )
                }) < 0
                    && std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT)
            });
            empty && removed
        }
        fn runs_fd(&self) -> i32 {
            self.parent.as_raw_fd()
        }
    }

    #[test]
    #[ignore = "A12 source preparation only; delivery unattested; unselected; NOT granted"]
    fn disposable_linux_a12() {
        let startup = Instant::now();
        let prepared = prepared_delivery().expect("A12 delivery unattested");
        for (key, value) in [
            ("CI", "true"),
            ("GITHUB_ACTIONS", "true"),
            ("RUNNER_ENVIRONMENT", "github-hosted"),
            ("ARX_DISPOSABLE_EXEC_TEST", "1"),
        ] {
            assert_eq!(std::env::var(key).as_deref(), Ok(value));
        }
        assert_eq!(unsafe { libc::getuid() }, 0);
        assert_eq!(unsafe { libc::geteuid() }, 0);
        assert_eq!(unsafe { libc::umask(0o077) }, 0o077);
        assert!(!Path::new("identity-ack").exists());
        let config = prepared.config.clone();
        assert!(config.uid >= 1000 && config.gid >= 1000 && !config.admin);
        assert_eq!((config.memory_bytes, config.pids), (64 * 1024 * 1024, 32));
        let fs = hierarchy::linux::LinuxFs::system();
        struct Clock(Instant);
        impl hierarchy::setup::Clock for Clock {
            fn now(&self) -> Duration {
                self.0.elapsed()
            }
            fn sleep(&self, d: Duration) {
                std::thread::sleep(d);
            }
        }
        let proc = fs.proc_cgroup().unwrap();
        assert_eq!(proc.magic, hierarchy::setup::PROC_MAGIC);
        let location = hierarchy::locate(&proc.bytes).unwrap();
        let unit = hierarchy::fake::a5::preflight(&fs, &location).unwrap();
        // Refuse collisions before the janitor could touch a pre-existing leaf.
        assert!(matches!(
            fs.open_child(&unit, b"runs"),
            Err(hierarchy::setup::FsError::Missing)
        ));
        assert!(hierarchy::fake::prepare_a1a2(
            &fs,
            &Clock(Instant::now()),
            &Cancellation::default()
        ));
        let runs = fs.open_child(&unit, b"runs").unwrap();
        let path = config.cgroup_parent.clone().unwrap();
        assert_eq!(
            path,
            PathBuf::from("/sys/fs/cgroup")
                .join(location.unit_components().join("/"))
                .join("runs")
        );
        let parent = File::from(super::super::unix::trusted_root(&path).unwrap());
        let observation = RefCell::new(Observation {
            fs,
            runs,
            path,
            parent,
            held: None,
        });
        assert!(observation.borrow().absent());
        let runner = Runner::new(config.clone()).unwrap();
        assert!(startup.elapsed() <= Duration::from_secs(15));
        crate::service::unix::Notify::send_to(
            &std::env::var_os("NOTIFY_SOCKET").unwrap(),
            b"READY=1",
        )
        .unwrap();
        identity_ack();
        for case in [Case::Normal, Case::Cancel] {
            // Reopen every owned fact at the actual call boundary, in its worker.
            let held_runs = observation.borrow().parent.metadata().unwrap();
            assert!(exercise(
                case,
                (config.uid, config.gid),
                |token| checked_call(
                    || {
                        prepared.recheck(true, case)?;
                        let reopened =
                            root_open(config.cgroup_parent.as_ref()?)?.metadata().ok()?;
                        (runs_identity(
                            (held_runs.dev(), held_runs.ino()),
                            (reopened.dev(), reopened.ino()),
                            reopened.mode(),
                            reopened.uid(),
                        ) && case.spec().cwd.as_os_str().is_empty())
                        .then_some(())
                    },
                    || runner.run_phased(&case.spec(), token, &Cancellation::default())
                ),
                || observation.borrow_mut().sample(),
                || (runner.cleanup_residue(), observation.borrow().absent())
            ));
            marker(match case {
                Case::Normal => b"ARX_A12_NORMAL_OK\n",
                Case::Cancel => b"ARX_A12_CANCEL_OK\n",
            });
        }
        marker(b"ARX_A12_COMPLETE_OK\n");
        assert!(startup.elapsed() <= Duration::from_secs(90));
    }

    #[test]
    fn a12_delivery_closed() {
        assert!(prepared_delivery().is_none());
    }

    fn marker(bytes: &[u8]) {
        assert_eq!(
            unsafe { libc::write(1, bytes.as_ptr().cast(), bytes.len()) },
            bytes.len() as isize
        );
    }

    fn identity_ack() {
        use std::{io::Read, os::unix::fs::OpenOptionsExt};
        let start = Instant::now();
        loop {
            match std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
                .open("identity-ack")
            {
                Ok(file) => {
                    let m = file.metadata().unwrap();
                    assert!(
                        m.is_file() && m.uid() == 0 && m.mode() & 0o7777 == 0o600 && m.nlink() == 1
                    );
                    let mut bytes = vec![];
                    file.take(2).read_to_end(&mut bytes).unwrap();
                    assert_eq!(bytes, b"1");
                    assert!(start.elapsed() <= Duration::from_secs(5));
                    return;
                }
                Err(e) => assert_eq!(e.kind(), std::io::ErrorKind::NotFound),
            }
            assert!(start.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

#[test]
fn a12_acceptance_counterfactuals() {
    let make = mock_result;
    for case in [Case::Normal, Case::Cancel] {
        let check = |run: &Result<RunResult, RunError>, proof, joined, residue, absent, elapsed| {
            accepts(
                case,
                run,
                (1001, 1002),
                proof,
                joined,
                residue,
                absent,
                elapsed,
            )
        };
        assert!(check(&make(case), true, true, false, true, Duration::ZERO));
        for (proof, joined, residue, absent, elapsed) in [
            (true, false, false, true, 0),
            (true, true, true, true, 0),
            (true, true, false, false, 0),
            (true, true, false, true, 26),
        ] {
            assert!(!check(
                &make(case),
                proof,
                joined,
                residue,
                absent,
                Duration::from_secs(elapsed)
            ));
        }
        if case == Case::Cancel {
            assert!(!check(
                &make(case),
                false,
                true,
                false,
                true,
                Duration::ZERO
            ));
        }
        for error in [
            RunError::NotStarted(NotStarted::Cancelled),
            RunError::SpawnFailed,
            RunError::AfterSpawn,
        ] {
            assert!(!check(&Err(error), true, true, false, true, Duration::ZERO));
        }
        for mutation in 0..7 {
            let mut run = make(case).unwrap();
            match mutation {
                0 => run.tree_cleanup_confirmed = false,
                1 => run.identity.uid += 1,
                2 => run.outcome = Outcome::TimedOut,
                3 => run.exit_code = Some(7),
                4 => run.identity.gid += 1,
                5 => run.identity.daemon_was_root = false,
                _ => run.tree_boundary = TreeBoundary::MacProcessGroupBestEffort,
            }
            assert!(!check(&Ok(run), true, true, false, true, Duration::ZERO));
        }
        assert_eq!(case.spec().timeout, Duration::from_secs(15));
    }
}

fn mock_result(case: Case) -> Result<RunResult, RunError> {
    Ok(RunResult {
        outcome: if case == Case::Normal {
            Outcome::Exited
        } else {
            Outcome::Cancelled
        },
        exit_code: if case == Case::Normal { Some(0) } else { None },
        identity: ObservedIdentity {
            uid: 1001,
            gid: 1002,
            daemon_was_root: true,
        },
        tree_boundary: TreeBoundary::LinuxCgroupV2,
        tree_cleanup_confirmed: true,
        stdout: CapturedOutput {
            head: vec![],
            tail: vec![],
            dropped_bytes: 0,
        },
        stderr: CapturedOutput {
            head: vec![],
            tail: vec![],
            dropped_bytes: 0,
        },
    })
}

#[test]
fn a12_owned_call_mock_counterfactuals() {
    use std::sync::atomic::AtomicUsize;
    let good = Population {
        before: (7, 9),
        after: (7, 9),
        pid: 42,
        populated: true,
        owned: true,
    };
    for fault in 0..7 {
        let phase = AtomicUsize::new(0);
        let observed = AtomicBool::new(false);
        let accepted = exercise(
            Case::Cancel,
            (1001, 1002),
            |token| {
                phase.store(1, Ordering::Release);
                let start = Instant::now();
                while !token.is_cancelled() && start.elapsed() < Duration::from_secs(1) {
                    std::thread::yield_now();
                }
                assert!(token.is_cancelled());
                phase.store(2, Ordering::Release);
                mock_result(Case::Cancel)
            },
            || {
                if phase.load(Ordering::Acquire) == 0 {
                    return Ok(None);
                }
                observed.store(true, Ordering::Release);
                let mut p = good;
                match fault {
                    1 => p.after.1 += 1,
                    2 => p.populated = false,
                    3 => p.owned = false,
                    4 => p.pid = 0,
                    5 => return Err(()),
                    6 => panic!("injected observation failure"),
                    _ => (),
                }
                Ok(Some(p))
            },
            || {
                assert_eq!(
                    phase.load(Ordering::Acquire),
                    2,
                    "postconditions only after join"
                );
                (false, true)
            },
        );
        assert!(observed.load(Ordering::Acquire));
        assert_eq!(accepted, fault == 0);
    }
    assert!(exercise(
        Case::Normal,
        (1001, 1002),
        |_| mock_result(Case::Normal),
        || panic!("normal does not need population sampling"),
        || (false, true)
    ));
    assert!(!exercise(
        Case::Cancel,
        (1001, 1002),
        |_| Err(RunError::NotStarted(NotStarted::Cancelled)),
        || Ok(None),
        || (false, true)
    ));
    assert!(!exercise(
        Case::Normal,
        (1001, 1002),
        |_| panic!("injected worker failure"),
        || Ok(None),
        || (false, true)
    ));
}

#[test]
fn a12_delivery_workspace_relative_cwd_contract() {
    // Pure call-boundary validation only: never construct or call a Runner.
    // Empty cwd binds the owned root. Absolute cwd must remain rejected.
    let workspace = if cfg!(windows) {
        PathBuf::from("C:/arxcovo-a12-contract/workspace")
    } else {
        PathBuf::from("/var/lib/arxcovo-a12-contract/workspace")
    };
    assert!(workspace.is_absolute());
    for case in [Case::Normal, Case::Cancel] {
        let mut spec = case.spec();
        // Use a platform-native absolute spelling for this portable validator
        // test; Linux's fixed targets are separately pinned below.
        if cfg!(windows) {
            spec.executable = PathBuf::from("C:/arxcovo-a12-contract/target.exe");
        }
        assert!(validate_spec(&spec).is_ok());
        assert!(validate_phased_spec(&spec).is_ok());
        spec.cwd = workspace.clone();
        for result in [validate_spec(&spec), validate_phased_spec(&spec)] {
            assert!(matches!(
                result,
                Err(ExecError::Invalid(
                    "absolute executable and workspace-relative cwd required"
                ))
            ));
        }
        spec.cwd = PathBuf::new();
        assert!(validate_phased_spec(&spec).is_ok());
    }
}

#[test]
fn a12_fixed_targets_and_closed_source_gate() {
    assert_eq!(
        Case::Normal.spec().executable,
        PathBuf::from("/usr/bin/true")
    );
    assert!(Case::Normal.spec().arguments.is_empty());
    assert_eq!(
        Case::Cancel.spec().executable,
        PathBuf::from("/usr/bin/sleep")
    );
    assert_eq!(Case::Cancel.spec().arguments, ["15"]);
    let source = include_str!("a12.rs");
    const { assert!(!DELIVERY_ENABLED) };
    assert!(closed_read::<()>(|| panic!("closed gate must not read")).is_none());
    // Exclude portable tests, so neither the expected string nor the mutation
    // controls below can satisfy the production wiring check by matching itself.
    let wired = |text: &str| {
        let native = text
            .split("\n#[test]\nfn a12_acceptance_counterfactuals")
            .next()
            .unwrap();
        let body = native
            .split("fn prepared_delivery() -> Option<Prepared> {")
            .nth(1)
            .and_then(|rest| rest.split_once('}'))
            .map(|(body, _)| body.trim());
        body == Some("closed_read(read_delivery)") && native.matches("read_delivery").count() == 2
    };
    assert!(wired(source));
    let bypass = source.replacen("closed_read(read_delivery)\n", "read_delivery()\n", 1);
    assert_ne!(bypass, source);
    assert!(!wired(&bypass));
    let extra = source.replacen(
        "    fn read_delivery()",
        "    fn hidden_bypass() { let _ = read_delivery(); }\n    fn read_delivery()",
        1,
    );
    assert_ne!(extra, source);
    assert!(!wired(&extra));
    let fixture = source
        .split("fn disposable_linux_a12() {")
        .nth(1)
        .unwrap()
        .split("#[test]")
        .next()
        .unwrap();
    for host in [
        "std::env::var(key)",
        "libc::getuid()",
        "LinuxFs::system()",
        "prepare_a1a2(",
        "Runner::new(",
    ] {
        assert!(fixture.find("prepared_delivery()").unwrap() < fixture.find(host).unwrap());
    }
    assert!(fixture.find("identity_ack();").unwrap() < fixture.find("runner.run_phased(").unwrap());
    assert!(
        fixture
            .find("Err(hierarchy::setup::FsError::Missing)")
            .unwrap()
            < fixture.find("prepare_a1a2(").unwrap()
    );
}

#[test]
fn a12_delivery_schema_and_layout_counterfactuals() {
    let exe = Path::new("/var/lib/arx-ci-123-1/source/target/debug/deps/arxd-abc123");
    let (stage, run, name) = layout(exe).unwrap();
    assert_eq!(stage, Path::new("/var/lib/arx-ci-123-1"));
    assert_eq!(name, "arxa12123x1");
    for wrong in [
        "/tmp/arx-ci-123-1/source/target/debug/deps/arxd-ab",
        "/var/lib/arx-ci-0123-1/source/target/debug/deps/arxd-ab",
        "/var/lib/arx-ci-123-1/source/target/debug/arxd-ab",
        "/var/lib/arx-ci-123-1/source/target/debug/deps/arxd-xyz",
    ] {
        assert!(layout(Path::new(wrong)).is_none());
    }
    let pin = |uid, mode| serde_json::json!({"dev":1,"ino":9,"uid":uid,"gid":uid,"mode":mode,"nlink":1,"size":0,"sha256":""});
    let mut value = serde_json::json!({"version":1,"run":run,"source_sha":"a".repeat(40),"workflow_sha":"a".repeat(40),"uid":1001,"gid":1001,"root":pin(1001,0o40700),"workspace":pin(1001,0o40700),"harness":pin(0,0o100700)});
    value["harness"]["size"] = 4.into();
    value["harness"]["sha256"] = "b".repeat(64).into();
    let bytes = serde_json::to_vec(&value).unwrap();
    let d = decode(&bytes, "123-1", &"a".repeat(40)).unwrap();
    let target = Pin {
        mode: 0o100755,
        ..d.harness.clone()
    };
    assert!(owned_pins(&d, &d.root, &d.workspace, &d.harness, &target));
    for role in 0..4 {
        for field in 0..8 {
            let mut pins = [
                d.root.clone(),
                d.workspace.clone(),
                d.harness.clone(),
                target.clone(),
            ];
            let p = &mut pins[role];
            match field {
                0 => p.dev += 1,
                1 => p.ino += 1,
                2 => p.mode = 0o120777,
                3 => p.uid += 1,
                4 => p.gid += 1,
                5 => p.nlink += 1,
                6 => p.size += 1,
                _ => p.sha256 = "c".repeat(64),
            }
            // Fixed system target pins trust/regular/executable/link predicates,
            // rather than authorizing a caller-provided system-image hash.
            if role == 3 && [0, 1, 4, 6, 7].contains(&field) {
                continue;
            }
            assert!(
                !owned_pins(&d, &pins[0], &pins[1], &pins[2], &pins[3]),
                "role {role} field {field}"
            );
        }
    }

    assert!(decode(&bytes, "123-1", &"a".repeat(40)).is_some());
    assert!(decode(&bytes, "124-1", &"a".repeat(40)).is_none());
    assert!(decode(&bytes, "123-1", &"c".repeat(40)).is_none());
    for pointer in [
        "/uid",
        "/gid",
        "/root/uid",
        "/root/mode",
        "/workspace/uid",
        "/workspace/mode",
        "/harness/nlink",
        "/harness/ino",
        "/harness/mode",
        "/harness/size",
    ] {
        let mut bad = value.clone();
        *bad.pointer_mut(pointer).unwrap() = 0.into();
        if pointer == "/root/uid" {
            bad["root"]["uid"] = 1.into();
        }
        assert!(
            decode(&serde_json::to_vec(&bad).unwrap(), "123-1", &"a".repeat(40)).is_none(),
            "{pointer}"
        );
    }
    for suffix in [",\"unknown\":1}", ",\"uid\":1001}"] {
        let mut bad = bytes[..bytes.len() - 1].to_vec();
        bad.extend_from_slice(suffix.as_bytes());
        assert!(decode(&bad, "123-1", &"a".repeat(40)).is_none());
    }
    assert!(decode(&vec![b' '; 4097], "123-1", &"a".repeat(40)).is_none());
    let mut bad = value;
    bad["workspace"]["extra"] = true.into();
    assert!(decode(&serde_json::to_vec(&bad).unwrap(), "123-1", &"a".repeat(40)).is_none());
}

#[test]
fn a12_actual_reader_and_per_call_refusal() {
    let bytes = b"{}";
    assert_eq!(
        record_bytes(&bytes[..], 0o100600, 0, 1, 2),
        Some(bytes.to_vec())
    );
    for (mode, uid, nlink, size) in [
        (0o100644, 0, 1, 2),
        (0o120600, 0, 1, 2),
        (0o100600, 1, 1, 2),
        (0o100600, 0, 2, 2),
        (0o100600, 0, 1, 4097),
        (0o100600, 0, 1, 1),
        (0o100600, 0, 1, 3),
    ] {
        assert!(record_bytes(&bytes[..], mode, uid, nlink, size).is_none());
    }
    let called = std::cell::Cell::new(0);
    for valid in [true, false, true, false] {
        let result = checked_call(
            || valid.then_some(()),
            || {
                called.set(called.get() + 1);
                mock_result(Case::Normal)
            },
        );
        assert_eq!(result.is_ok(), valid);
    }
    assert_eq!(called.get(), 2, "failed revalidation cannot call Runner");
    struct Broken;
    impl std::io::Read for Broken {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::Other.into())
        }
    }
    assert!(record_bytes(Broken, 0o100600, 0, 1, 2).is_none());
}

#[test]
fn a12_runs_reopen_identity_counterfactuals() {
    assert!(runs_identity((7, 9), (7, 9), 0o40755, 0));
    for (observed, mode, uid) in [
        ((8, 9), 0o40755, 0),
        ((7, 10), 0o40755, 0),
        ((7, 9), 0o40775, 0),
        ((7, 9), 0o40755, 1001),
        ((7, 9), 0o100755, 0),
    ] {
        assert!(!runs_identity((7, 9), observed, mode, uid));
    }
}
