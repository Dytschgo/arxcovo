//! Dormant A12 producer fixture. Portable tests use only injected observations.
use super::*;
use std::{panic::AssertUnwindSafe, time::Instant};

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
    }
    fn prepared_delivery() -> Option<Prepared> {
        // No A12 delivery attestation or run grant exists. Never consume A7's.
        // Future delivery must pin exclusive owned stage/account/workspace/runs,
        // artifacts and fresh-host gates, with collision quarantine and cleanup.
        None
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
        let config = prepared.config;
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
            assert!(exercise(
                case,
                (config.uid, config.gid),
                |token| runner.run_phased(&case.spec(), token, &Cancellation::default()),
                || observation.borrow_mut().sample(),
                || (runner.cleanup_residue(), observation.borrow().absent())
            ));
        }
        assert!(startup.elapsed() <= Duration::from_secs(90));
    }

    #[test]
    fn a12_delivery_closed() {
        assert!(prepared_delivery().is_none());
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
fn a12_delivery_cwd_contract_conflict() {
    // Pure call-boundary validation only: never construct or call a Runner.
    // The delivery brief requests cwd = config.workspace, an absolute path.
    // Both existing entry validators reject that before the native call.
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
    let body = source
        .split("fn prepared_delivery() -> Option<Prepared> {")
        .nth(1)
        .unwrap()
        .split('}')
        .next()
        .unwrap();
    let code: String = body
        .lines()
        .map(|line| line.split("//").next().unwrap())
        .collect();
    assert_eq!(code.trim(), "None");
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
