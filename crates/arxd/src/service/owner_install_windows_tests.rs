use super::super::{layout::Layout, owner_scm::*};
use super::*;
use std::{cell::RefCell, io::Write, rc::Rc, time::Duration};
type Log = Rc<RefCell<Vec<String>>>;
thread_local! { static HOOK: RefCell<(Log, Option<usize>, usize)> = RefCell::new((Rc::default(), None, 0)); }
type Boundary = Box<dyn FnMut(&str)>;
thread_local! { static BOUNDARY: RefCell<Option<Boundary>> = RefCell::new(None); }
fn boundary(name: &str) {
    BOUNDARY.with(|b| {
        if let Some(check) = b.borrow_mut().as_mut() {
            check(name);
        }
    });
}
pub(super) fn created(path: &Path) -> Result<(), Failure> {
    boundary(path.file_name().unwrap().to_str().unwrap());
    HOOK.with(|h| {
        let mut h = h.borrow_mut();
        h.0.borrow_mut().push(format!(
            "create:{}",
            path.file_name().unwrap().to_string_lossy()
        ));
        h.2 += 1;
        if h.1 == Some(h.2) {
            Err(Failure::Io)
        } else {
            Ok(())
        }
    })
}
struct Fake {
    log: Log,
    exists: bool,
    foreign: bool,
    active: bool,
    fail: Option<&'static str>,
    base: BaseConfig,
    values: Vec<Config2>,
}
impl Fake {
    fn new(log: Log) -> Self {
        Self {
            log,
            exists: false,
            foreign: false,
            active: false,
            fail: None,
            base: BaseConfig {
                service_type: TYPE,
                start_type: START_DEMAND,
                image: vec![],
                account: "LocalSystem".encode_utf16().collect(),
            },
            values: vec![],
        }
    }
}
impl ScmCalls for Fake {
    fn open(&mut self, a: Access) -> Result<bool, Win32> {
        if a == Access::Install {
            boundary("register");
        }
        self.log.borrow_mut().push(
            if a == Access::Status {
                "observe"
            } else {
                "register"
            }
            .into(),
        );
        if self.fail == Some("open") || (a == Access::Install && self.fail == Some("register")) {
            return Err(Win32(5));
        }
        Ok(self.exists)
    }
    fn create(&mut self, image: &[u16]) -> Result<(), Win32> {
        self.exists = true;
        self.base.image = image.to_vec();
        Ok(())
    }
    fn config(&mut self) -> Result<BaseConfig, Fault> {
        if self.fail == Some("config") {
            return Err(Fault::Malformed);
        }
        let mut b = self.base.clone();
        if self.foreign {
            b.service_type = 99;
        }
        Ok(b)
    }
    fn config2(&mut self, l: Level) -> Result<Config2, Fault> {
        if self.fail == Some("config2") {
            return Err(Fault::Malformed);
        }
        self.values
            .iter()
            .find(|v| v.level() == l)
            .cloned()
            .ok_or(Fault::Malformed)
    }
    fn state(&mut self) -> Result<Snapshot, Fault> {
        if self.fail == Some("state") {
            return Err(Fault::Malformed);
        }
        Ok(Snapshot {
            state: if self.active { RUNNING } else { STOPPED },
            win32_exit: 0,
            specific_exit: 0,
        })
    }
    fn set_start(&mut self, b: Boot) -> Result<(), Win32> {
        assert_eq!(b, Boot::Demand);
        self.base.start_type = START_DEMAND;
        Ok(())
    }
    fn set_config2(&mut self, v: &Config2) -> Result<(), Win32> {
        self.values.retain(|old| old.level() != v.level());
        self.values.push(v.clone());
        Ok(())
    }
    fn start(&mut self) -> Result<(), Win32> {
        panic!("activation forbidden")
    }
    fn control_stop(&mut self) -> Result<(), Win32> {
        panic!("stop forbidden")
    }
    fn delete(&mut self) -> Result<(), Win32> {
        panic!("delete forbidden")
    }
}
struct FakeClock;
impl Clock for FakeClock {
    fn now(&self) -> Duration {
        Duration::ZERO
    }
    fn sleep(&mut self, _: Duration) {
        panic!("poll forbidden")
    }
}
struct Fixture {
    temp: tempfile::TempDir,
    policy: ProtectionPolicy,
    log: Log,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let policy = ProtectionPolicy {
            fixture_owner: Some(crate::local::current_identity().unwrap()),
        };
        // Every fixture path is beneath this owned temporary directory.
        directory(&temp.path().join("bin"), &policy, FILE_SHARE_READ).unwrap();
        let log = Log::default();
        HOOK.with(|h| *h.borrow_mut() = (log.clone(), None, 0));
        Self { temp, policy, log }
    }
    fn ns(&self) -> Namespace {
        Namespace::Fixture {
            parent: self.temp.path().to_owned(),
            binary_directory: existing(&self.temp.path().join("bin"), READ, FILE_SHARE_READ)
                .unwrap()
                .unwrap(),
        }
    }
    fn engine(&self) -> WindowsScm<Fake, FakeClock> {
        let mut f = Fake::new(self.log.clone());
        let layout = Layout::for_test(Platform::Windows);
        // Capture the exact adapter registration values without duplicating them.
        let r = super::super::owner_plan::registration(&layout);
        f.base.image = format!("\"{}\" service run", r.image.display())
            .encode_utf16()
            .collect();
        f.values = vec![
            Config2::Delayed(1),
            Config2::Sid(1),
            Config2::Privileges(
                r.privileges
                    .iter()
                    .map(|s| s.encode_utf16().collect())
                    .collect(),
            ),
            Config2::Preshutdown(30000),
            Config2::Failure {
                reset: 86400,
                reboot: vec![],
                command: vec![],
                actions: vec![(1, 5000), (1, 30000), (0, 0)],
            },
            Config2::NonCrash(0),
        ];
        WindowsScm::new(&layout, f, FakeClock).unwrap()
    }
    fn run(&self) -> Result<(), OpenError> {
        self.run_engine(&mut self.engine())
    }
    fn run_engine(&self, scm: &mut WindowsScm<Fake, FakeClock>) -> Result<(), OpenError> {
        install_with(scm, self.ns(), &self.policy, || {
            self.log.borrow_mut().push("restore".into());
            Ok(())
        })
    }
    fn state(&self) -> PathBuf {
        self.temp.path().join("Arxcovo/state")
    }
    fn file(&self, path: &Path, bytes: &[u8]) {
        let mut f = create(path, &self.policy, FILE_SHARE_READ).unwrap();
        f.write_all(bytes).unwrap();
        f.sync_all().unwrap();
    }
    fn partial(&self) {
        directory(
            &self.temp.path().join("Arxcovo"),
            &self.policy,
            FILE_SHARE_READ,
        )
        .unwrap();
        directory(
            &self.temp.path().join("Arxcovo/config"),
            &self.policy,
            FILE_SHARE_READ,
        )
        .unwrap();
        directory(&self.state(), &self.policy, FILE_SHARE_READ).unwrap();
        self.log.borrow_mut().clear();
    }
}
fn snapshot(path: &Path) -> (Vec<u8>, (u64, [u8; 16]), std::time::SystemTime) {
    let f = existing(path, READ, FILE_SHARE_READ).unwrap().unwrap();
    (
        std::fs::read(path).unwrap(),
        super::super::owner_windows::identity(&f).unwrap(),
        f.metadata().unwrap().modified().unwrap(),
    )
}
#[test]
fn i1_fresh_and_order() {
    let f = Fixture::new();
    f.run().unwrap();
    for relative in ["Arxcovo", "Arxcovo/config", "Arxcovo/state"] {
        let dir = existing(&f.temp.path().join(relative), READ, FILE_SHARE_READ)
            .unwrap()
            .unwrap();
        f.policy.check(&dir, true).unwrap();
    }
    let lock = f.temp.path().join("Arxcovo/config/owner.lock");
    let handle = existing(&lock, READ, FILE_SHARE_READ).unwrap().unwrap();
    f.policy.check(&handle, false).unwrap();
    assert_eq!(handle.metadata().unwrap().len(), 0);
    assert_eq!(std::fs::read_dir(f.state()).unwrap().count(), 3);
    assert_eq!(
        std::fs::read_dir(f.temp.path().join("Arxcovo/config"))
            .unwrap()
            .count(),
        1
    );
    for name in MEMBERS {
        let file = existing(&f.state().join(name), READ, FILE_SHARE_READ)
            .unwrap()
            .unwrap();
        f.policy.check(&file, false).unwrap();
        assert_eq!(file.metadata().unwrap().len(), 0);
    }
    let log = f.log.borrow();
    assert_eq!(&log[..2], ["observe", "restore"]);
    assert_eq!(
        &log[2..9],
        [
            "create:Arxcovo",
            "create:config",
            "create:owner.lock",
            "create:state",
            "create:activity.lock",
            "create:activity.sqlite3",
            "create:activity.sqlite3-journal"
        ]
    );
    assert_eq!(log[9], "register");
    assert_eq!(log.len(), 10);
    assert_eq!(
        std::fs::read_dir(f.temp.path().join("Arxcovo"))
            .unwrap()
            .count(),
        2
    );
}
#[test]
fn i2_observe_and_binary_refuse_before_mutation() {
    for case in [
        "foreign", "active", "open", "config", "state", "config2", "binary",
    ] {
        let f = Fixture::new();
        let mut scm = f.engine();
        scm.test_calls().exists = true;
        if case == "foreign" {
            scm.test_calls().foreign = true;
        } else if case == "active" {
            scm.test_calls().active = true;
        } else {
            scm.test_calls().fail = Some(case);
        }
        let ns = if case == "binary" {
            Namespace::Fixture {
                parent: f.temp.path().to_owned(),
                binary_directory: existing(f.temp.path(), READ, FILE_SHARE_READ)
                    .unwrap()
                    .unwrap(),
            }
        } else {
            f.ns()
        };
        let result = install_with(&mut scm, ns, &f.policy, || {
            panic!("restore before admission")
        });
        let expected = match case {
            "foreign" => Error::Foreign,
            "active" => Error::Active,
            "binary" => Error::Permissions,
            _ => Error::Manager,
        };
        assert!(
            matches!(result, Err(OpenError::Layout(e)) if e == expected),
            "{case}"
        );
        assert!(!f.temp.path().join("Arxcovo").exists());
        assert!(!f.log.borrow().iter().any(|v| v == "register"));
    }
}
#[test]
fn i3_crash_prefixes_and_register_retry() {
    for n in 1..=7 {
        let f = Fixture::new();
        HOOK.with(|h| h.borrow_mut().1 = Some(n));
        assert!(matches!(f.run(), Err(OpenError::Store(Failure::Io))));
        let dirs: Vec<_> = ["Arxcovo", "Arxcovo/config", "Arxcovo/state"]
            .into_iter()
            .map(|p| f.temp.path().join(p))
            .filter(|p| p.exists())
            .map(|p| {
                let h = existing(&p, READ, FILE_SHARE_READ).unwrap().unwrap();
                (p, super::super::owner_windows::identity(&h).unwrap())
            })
            .collect();
        let owner = f.temp.path().join("Arxcovo/config/owner.lock");
        let owner_before = owner.exists().then(|| snapshot(&owner));
        let old: Vec<_> = MEMBERS
            .iter()
            .filter_map(|name| {
                let p = f.state().join(name);
                p.is_file().then(|| (p.clone(), snapshot(&p)))
            })
            .collect();
        HOOK.with(|h| h.borrow_mut().1 = None);
        f.run().unwrap();
        for (p, id) in dirs {
            let h = existing(&p, READ, FILE_SHARE_READ).unwrap().unwrap();
            assert_eq!(super::super::owner_windows::identity(&h).unwrap(), id);
        }
        if let Some(before) = owner_before {
            assert_eq!(snapshot(&owner), before);
        }
        for (p, before) in old {
            assert_eq!(snapshot(&p), before);
        }
    }
    let f = Fixture::new();
    let mut scm = f.engine();
    scm.test_calls().fail = Some("register");
    assert!(matches!(
        f.run_engine(&mut scm),
        Err(OpenError::Layout(Error::Manager))
    ));
    let files: Vec<_> = [
        "Arxcovo/config/owner.lock",
        "Arxcovo/state/activity.lock",
        "Arxcovo/state/activity.sqlite3",
        "Arxcovo/state/activity.sqlite3-journal",
    ]
    .into_iter()
    .map(|p| f.temp.path().join(p))
    .map(|p| {
        let before = snapshot(&p);
        (p, before)
    })
    .collect();
    let dirs: Vec<_> = ["Arxcovo", "Arxcovo/config", "Arxcovo/state"]
        .into_iter()
        .map(|p| f.temp.path().join(p))
        .map(|p| {
            let h = existing(&p, READ, FILE_SHARE_READ).unwrap().unwrap();
            (p, super::super::owner_windows::identity(&h).unwrap())
        })
        .collect();
    scm.test_calls().fail = None;
    f.run_engine(&mut scm).unwrap();
    for (p, before) in files {
        assert_eq!(snapshot(&p), before);
    }
    for (p, before) in dirs {
        let h = existing(&p, READ, FILE_SHARE_READ).unwrap().unwrap();
        assert_eq!(super::super::owner_windows::identity(&h).unwrap(), before);
    }
}
#[test]
fn i4_retains_database_and_journal() {
    let f = Fixture::new();
    f.partial();
    let mut header = b"SQLite format 3\0\x10\x00\x01\x01".to_vec();
    header.extend_from_slice(b"retained tail");
    f.file(&f.state().join(MEMBERS[1]), &header);
    f.file(&f.state().join(MEMBERS[2]), b"journal");
    let before = [
        snapshot(&f.state().join(MEMBERS[1])),
        snapshot(&f.state().join(MEMBERS[2])),
    ];
    f.run().unwrap();
    assert_eq!(snapshot(&f.state().join(MEMBERS[1])), before[0]);
    assert_eq!(snapshot(&f.state().join(MEMBERS[2])), before[1]);
    assert!(f.state().join(MEMBERS[0]).is_file());
}
#[test]
fn i5_unsafe_state_is_classified_before_creates() {
    for case in 0..27 {
        let f = Fixture::new();
        f.partial();
        match case {
            0 => {
                f.file(&f.state().join("activity.sqlite3-wal"), b"");
            }
            1 => {
                f.file(&f.state().join("activity.sqlite3-shm"), b"");
            }
            2 => {
                std::fs::create_dir(f.state().join("activity.sqlite3-shm")).unwrap();
            }
            3 => {
                f.file(&f.state().join(MEMBERS[0]), b"x");
            }
            4 => {
                f.file(&f.temp.path().join("Arxcovo/config/owner.lock"), b"x");
            }
            5 => {
                f.file(
                    &f.state().join(MEMBERS[1]),
                    b"SQLite format 3\0\x10\x00\x02\x02",
                );
            }
            6 => {
                f.file(&f.state().join(MEMBERS[1]), &[0; 20]);
            }
            7 => {
                f.file(&f.state().join(MEMBERS[1]), b"");
                std::fs::hard_link(f.state().join(MEMBERS[1]), f.state().join("alias")).unwrap();
            }
            n => {
                f.file(&f.state().join(MEMBERS[1]), &vec![0; n - 7]);
            }
        }
        f.log.borrow_mut().clear();
        let result = f.run();
        if case == 4 {
            assert!(
                matches!(result, Err(OpenError::Store(Failure::RecoveryRequired))),
                "case {case}"
            );
        } else {
            assert!(
                matches!(result, Err(OpenError::Layout(Error::Permissions))),
                "case {case}"
            );
        }
        assert!(!f.state().join(MEMBERS[2]).exists());
        assert!(
            !f.log
                .borrow()
                .iter()
                .any(|v| v == "register" || v.starts_with("create:activity"))
        );
    }
}
#[test]
fn i6_busy_locks_and_writer() {
    for name in [
        "Arxcovo/config/owner.lock",
        "Arxcovo/state/activity.lock",
        "Arxcovo/state/activity.sqlite3",
        "Arxcovo/state/activity.sqlite3-journal",
    ] {
        let f = Fixture::new();
        f.run().unwrap();
        let path = f.temp.path().join(name);
        let handle = if name.ends_with("sqlite3") || name.ends_with("journal") {
            std::fs::OpenOptions::new().write(true).open(path).unwrap()
        } else {
            existing(&path, READ, 0).unwrap().unwrap()
        };
        assert!(
            matches!(f.run(), Err(OpenError::Store(Failure::Busy))),
            "{name}"
        );
        drop(handle);
        f.run().unwrap();
    }
}
#[test]
fn i7_read_only_mask_fence_and_create_conflict() {
    let f = Fixture::new();
    let p = f.temp.path().join("absent");
    for bit in 0..32 {
        let bit = 1u32 << bit;
        if bit & READ_ONLY == 0 {
            assert!(matches!(
                existing(&p, READ | bit, 0),
                Err(Failure::RecoveryRequired)
            ));
        }
    }
    assert!(existing(&p, READ_ONLY, 0).unwrap().is_none());
    let p = f.temp.path().join("file");
    drop(create(&p, &f.policy, 0).unwrap());
    assert!(matches!(create(&p, &f.policy, 0), Err(Failure::Conflict)));
}
#[test]
fn i8_parent_pin_sharing_table() {
    for share in [FILE_SHARE_READ, FILE_SHARE_READ | FILE_SHARE_WRITE] {
        let f = Fixture::new();
        let parent = f.temp.path().join("parent");
        let pin = directory(&parent, &f.policy, share).unwrap();
        let child = directory(&parent.join("child"), &f.policy, share).unwrap();
        let file = parent.join("child/file");
        drop(create(&file, &f.policy, FILE_SHARE_READ).unwrap());
        let result = std::fs::rename(&file, parent.join("child/renamed"));
        if share == FILE_SHARE_READ {
            assert_eq!(result.unwrap_err().raw_os_error(), Some(32));
        } else {
            result.unwrap();
        }
        assert_eq!(
            std::fs::rename(parent.join("child"), parent.join("other"))
                .unwrap_err()
                .raw_os_error(),
            Some(32)
        );
        drop(child);
        drop(pin);
        std::fs::rename(&parent, f.temp.path().join("renamed-parent")).unwrap();
    }
}
#[test]
fn i9_descriptor_directory_and_binary_creator_owner() {
    let sd = descriptor(&ProtectionPolicy::production().creation_descriptor()).unwrap();
    let p = native::windows_descriptor_protection(sd.as_ptr()).unwrap();
    layout::check_protection(Platform::Windows, Policy::PrivateDirectory, true, &p).unwrap();
    if let layout::Protection::Windows {
        owner,
        protected,
        dacl_present,
        aces,
    } = p
    {
        assert_eq!(owner, "S-1-5-18");
        assert!(protected && dacl_present);
        assert_eq!(aces.len(), 2);
    } else {
        panic!("wrong platform");
    }
    let text = "O:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FRFX;;;BU)";
    for (suffix, good) in [("", true), ("(A;OICIIO;GA;;;CO)", false)] {
        let sd = descriptor(&format!("{text}{suffix}")).unwrap();
        let p = native::windows_descriptor_protection(sd.as_ptr()).unwrap();
        assert_eq!(
            layout::check_protection(Platform::Windows, Policy::BinaryIntegrity, true, &p).is_ok(),
            good
        );
    }
}
fn unsafe_directory(path: &Path, text: &str) {
    let sd = descriptor(text).unwrap();
    let name: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let sa = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd.as_ptr(),
        bInheritHandle: 0,
    };
    assert_ne!(unsafe { CreateDirectoryW(name.as_ptr(), &sa) }, 0);
}
#[test]
fn i5_acl_and_directory_types_refuse() {
    let who = crate::local::current_identity().unwrap();
    for case in [
        "foreign-dir",
        "unprotected-dir",
        "foreign-member",
        "member-directory",
    ] {
        let f = Fixture::new();
        match case {
            "foreign-dir" => unsafe_directory(
                &f.temp.path().join("Arxcovo"),
                &format!("O:{who}D:P(A;;FA;;;{who})(A;;FR;;;WD)"),
            ),
            "unprotected-dir" => {
                unsafe_directory(
                    &f.temp.path().join("Arxcovo"),
                    &format!("O:{who}D:(A;;FA;;;{who})"),
                );
                super::super::owner_windows::unprotect_install_fixture(&f.temp);
            }
            "foreign-member" => {
                f.partial();
                let sd = descriptor(&format!("O:{who}D:P(A;;FA;;;{who})(A;;FR;;;WD)")).unwrap();
                drop(
                    open(
                        &f.state().join(MEMBERS[1]),
                        GENERIC_READ | GENERIC_WRITE | READ_CONTROL,
                        FILE_SHARE_READ,
                        CREATE_NEW,
                        Some(&sd),
                    )
                    .unwrap(),
                );
            }
            _ => {
                f.partial();
                directory(&f.state().join(MEMBERS[1]), &f.policy, FILE_SHARE_READ).unwrap();
            }
        }
        f.log.borrow_mut().clear();
        let result = f.run();
        assert!(
            matches!(result, Err(OpenError::Layout(Error::Permissions))),
            "case {case}: {result:?}"
        );
        assert!(
            !f.log
                .borrow()
                .iter()
                .any(|v| v == "register" || v.starts_with("create:activity"))
        );
    }
}
#[test]
fn i5_junction_state_and_member_refuse() {
    use std::os::windows::process::CommandExt;
    for member in [false, true] {
        let f = Fixture::new();
        f.partial();
        let link = if member {
            f.state().join(MEMBERS[1])
        } else {
            std::fs::remove_dir(f.state()).unwrap();
            f.state()
        };
        let target = f.temp.path().join("target");
        directory(&target, &f.policy, FILE_SHARE_READ).unwrap();
        // A `/` in cmd.exe's own image path is parsed as `/c` (running `md`).
        let cmd = PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32")
            .join("cmd.exe");
        let output = std::process::Command::new(cmd)
            .current_dir(link.parent().unwrap())
            .raw_arg(format!(
                "/d /c mklink /J {} \"{}\"",
                link.file_name().unwrap().to_string_lossy(),
                target.to_string_lossy().replace('/', "\\")
            ))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "owned-temp junction setup failed: exit={:?}, syntax={}, denied={}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).contains("syntax"),
            String::from_utf8_lossy(&output.stderr).contains("denied")
        );
        f.log.borrow_mut().clear();
        assert!(matches!(
            f.run(),
            Err(OpenError::Layout(Error::Permissions))
        ));
        assert!(
            !f.log
                .borrow()
                .iter()
                .any(|v| v == "register" || v.starts_with("create:activity"))
        );
        std::fs::remove_dir(link).unwrap();
    }
}
#[test]
fn restore_failure_and_none_leaf_ancestor_rule() {
    let f = Fixture::new();
    assert!(matches!(
        install_with(&mut f.engine(), f.ns(), &f.policy, || Err(
            Error::OwnerRequired
        )),
        Err(OpenError::Layout(Error::OwnerRequired))
    ));
    assert!(!f.temp.path().join("Arxcovo").exists());
    // The shared walker applies its ancestor predicate to the None leaf too.
    let count = std::cell::Cell::new(0);
    let pins = super::super::owner_windows::walk_chain(
        f.temp.path(),
        None,
        &|_| panic!("no final policy"),
        &|_| {
            count.set(count.get() + 1);
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(count.get(), pins.len());
}

#[test]
fn i3_lock_and_pins_cover_state_creation_through_registration() {
    let f = Fixture::new();
    let root = f.temp.path().join("Arxcovo");
    let calls = Rc::new(RefCell::new(Vec::new()));
    let seen = calls.clone();
    BOUNDARY.with(|b| {
        *b.borrow_mut() = Some(Box::new(move |name| {
            if ![
                "state",
                "activity.lock",
                "activity.sqlite3",
                "activity.sqlite3-journal",
                "register",
            ]
            .contains(&name)
            {
                return;
            }
            seen.borrow_mut().push(name.to_owned());
            assert!(matches!(
                existing(&root.join("config/owner.lock"), READ, 0),
                Err(Failure::Busy)
            ));
            assert!(matches!(
                existing(&root.join("config/owner.lock"), READ, FILE_SHARE_READ),
                Err(Failure::Busy)
            ));
            for part in ["", "config", "state"] {
                assert_eq!(
                    std::fs::rename(root.join(part), root.with_file_name("moved"))
                        .unwrap_err()
                        .raw_os_error(),
                    Some(32)
                );
            }
        }))
    });
    let result = f.run().and_then(|()| f.run());
    BOUNDARY.with(|b| *b.borrow_mut() = None);
    result.unwrap();
    assert_eq!(
        *calls.borrow(),
        [
            "state",
            "activity.lock",
            "activity.sqlite3",
            "activity.sqlite3-journal",
            "register",
            "register"
        ]
    );
    // Success releases both pins and owner lock; a second install converges.
    f.run().unwrap();
}

#[test]
fn i7_real_open_error_boundaries_and_member_create_race() {
    let f = Fixture::new();
    let missing = f.temp.path().join("missing/child");
    assert!(matches!(existing(&missing, READ, 0), Err(Failure::Io)));
    assert!(matches!(create(&missing, &f.policy, 0), Err(Failure::Io)));
    let p = f.temp.path().join("held");
    let held = create(&p, &f.policy, 0).unwrap();
    assert!(matches!(existing(&p, READ, 0), Err(Failure::Busy)));
    assert!(matches!(
        open(&p, FILE_WRITE_DATA, 0, OPEN_EXISTING, None),
        Err(Failure::RecoveryRequired)
    ));
    drop(held);
    // This callback runs after classification, when the preceding missing
    // member was created: the next CREATE_NEW must expose 80 as Conflict.
    let db = f.state().join(MEMBERS[1]);
    BOUNDARY.with(|b| {
        *b.borrow_mut() = Some(Box::new(move |name| {
            if name == "activity.lock" {
                std::fs::write(&db, []).unwrap();
            }
        }))
    });
    let result = f.run();
    BOUNDARY.with(|b| *b.borrow_mut() = None);
    assert!(matches!(result, Err(OpenError::Store(Failure::Conflict))));
    assert!(!f.state().join(MEMBERS[2]).exists());
    assert!(!f.log.borrow().iter().any(|s| s == "register"));
}

#[test]
fn i5_sidefile_recheck_after_creation_refuses_registration() {
    let f = Fixture::new();
    let wal = f.state().join("activity.sqlite3-wal");
    BOUNDARY.with(|b| {
        *b.borrow_mut() = Some(Box::new(move |name| {
            if name == "activity.sqlite3-journal" {
                std::fs::write(&wal, []).unwrap();
            }
        }))
    });
    let result = f.run();
    BOUNDARY.with(|b| *b.borrow_mut() = None);
    assert!(matches!(result, Err(OpenError::Layout(Error::Permissions))));
    assert!(!f.log.borrow().iter().any(|s| s == "register"));
}

#[test]
fn i5_unsafe_owner_lock_and_wal_directory_tokens() {
    for case in ["hardlink", "directory", "wal-directory"] {
        let f = Fixture::new();
        f.partial();
        let lock = f.temp.path().join("Arxcovo/config/owner.lock");
        match case {
            "hardlink" => {
                f.file(&lock, &[]);
                std::fs::hard_link(&lock, f.temp.path().join("alias")).unwrap();
            }
            "directory" => {
                directory(&lock, &f.policy, FILE_SHARE_READ).unwrap();
            }
            _ => {
                directory(
                    &f.state().join("activity.sqlite3-wal"),
                    &f.policy,
                    FILE_SHARE_READ,
                )
                .unwrap();
            }
        }
        f.log.borrow_mut().clear();
        let result = f.run();
        if case == "wal-directory" {
            assert!(matches!(result, Err(OpenError::Layout(Error::Permissions))));
        } else {
            assert!(matches!(
                result,
                Err(OpenError::Store(Failure::RecoveryRequired))
            ));
        }
        assert!(
            !f.log
                .borrow()
                .iter()
                .any(|s| s == "register" || s.starts_with("create:activity"))
        );
        assert!(MEMBERS.iter().all(|n| !f.state().join(n).exists()));
    }
}
