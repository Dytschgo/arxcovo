use super::*;
use std::collections::VecDeque;
#[derive(Default)]
struct FakeClock(Duration);
impl Clock for FakeClock {
    fn now(&self) -> Duration {
        self.0
    }
    fn sleep(&mut self, d: Duration) {
        self.0 += d;
        assert!(
            self.0 <= Duration::from_secs(36),
            "poll loop must terminate"
        );
    }
}
struct FakeScm {
    exists: bool,
    marked: bool,
    base: BaseConfig,
    values: Vec<Config2>,
    snapshot: Snapshot,
    calls: Vec<String>,
    fail: Option<(usize, u32)>,
    malformed: Option<usize>,
    states: VecDeque<u32>,
    sticky: bool,
    race: bool,
}
impl FakeScm {
    fn new() -> Self {
        let r = registration(&Layout::for_test(Platform::Windows));
        Self {
            exists: false,
            marked: false,
            base: BaseConfig {
                service_type: TYPE,
                start_type: START_DEMAND,
                image: command_line(&r).unwrap(),
                account: r.account.encode_utf16().collect(),
            },
            values: expected(&r),
            snapshot: Snapshot {
                state: STOPPED,
                win32_exit: 0,
                specific_exit: 0,
            },
            calls: vec![],
            fail: None,
            malformed: None,
            states: VecDeque::new(),
            sticky: false,
            race: false,
        }
    }
    fn call(&mut self, s: impl Into<String>) -> Result<(), Win32> {
        self.calls.push(s.into());
        if let Some((i, e)) = self.fail
            && self.calls.len() == i
        {
            return Err(Win32(e));
        }
        Ok(())
    }
    fn read(&mut self, s: impl Into<String>) -> Result<(), Fault> {
        self.call(s).map_err(|e| Fault::Win32(e.0))?;
        if self.malformed == Some(self.calls.len()) {
            return Err(Fault::Malformed);
        }
        Ok(())
    }
    // Boot persistence MODEL, not Windows evidence.
    fn reboot_model(&mut self) {
        self.snapshot.state = if self.exists && !self.marked && self.base.start_type == START_AUTO {
            RUNNING
        } else {
            STOPPED
        };
    }
}
impl ScmCalls for FakeScm {
    fn open(&mut self, a: Access) -> Result<bool, Win32> {
        match self.call(format!("open:{a:?}")) {
            Err(Win32(ABSENT)) => return Ok(false),
            other => other?,
        }
        Ok(self.exists)
    }
    fn create(&mut self, image: &[u16]) -> Result<(), Win32> {
        if self.race {
            self.call("create:demand")?;
            self.exists = true;
            return Err(Win32(EXISTS));
        }
        self.call("create:demand")?;
        self.exists = true;
        self.base.image = image.to_vec();
        self.base.start_type = START_DEMAND;
        Ok(())
    }
    fn config(&mut self) -> Result<BaseConfig, Fault> {
        self.read("config")?;
        Ok(self.base.clone())
    }
    fn config2(&mut self, l: Level) -> Result<Config2, Fault> {
        self.read(format!("read:{l:?}"))?;
        Ok(self.values[LEVELS.iter().position(|x| *x == l).unwrap()].clone())
    }
    fn state(&mut self) -> Result<Snapshot, Fault> {
        self.read("state")?;
        if let Some(s) = self.states.pop_front() {
            self.snapshot.state = s;
        }
        Ok(self.snapshot)
    }
    fn set_start(&mut self, b: Boot) -> Result<(), Win32> {
        self.call(format!("boot:{b:?}"))?;
        if self.marked {
            return Err(Win32(MARKED));
        }
        self.base.start_type = match b {
            Boot::Auto => START_AUTO,
            Boot::Demand => START_DEMAND,
        };
        Ok(())
    }
    fn set_config2(&mut self, v: &Config2) -> Result<(), Win32> {
        self.call(format!("write:{:?}", v.level()))?;
        self.values[LEVELS.iter().position(|x| *x == v.level()).unwrap()] = v.clone();
        Ok(())
    }
    fn start(&mut self) -> Result<(), Win32> {
        self.call("start")?;
        if !self.sticky {
            self.snapshot.state = RUNNING;
        }
        Ok(())
    }
    fn control_stop(&mut self) -> Result<(), Win32> {
        self.call("control")?;
        if !self.sticky {
            self.snapshot.state = STOPPED;
        }
        Ok(())
    }
    fn delete(&mut self) -> Result<(), Win32> {
        self.call("delete")?;
        if self.marked {
            return Err(Win32(MARKED));
        }
        self.marked = true;
        Ok(())
    }
}
fn engine(present: bool, state: u32) -> WindowsScm<FakeScm, FakeClock> {
    let mut f = FakeScm::new();
    f.exists = present;
    f.snapshot.state = state;
    WindowsScm::for_test(
        &Layout::for_test(Platform::Windows),
        f,
        FakeClock::default(),
    )
}
fn reads() -> Vec<String> {
    LEVELS.iter().map(|l| format!("read:{l:?}")).collect()
}
fn writes() -> Vec<String> {
    LEVELS.iter().map(|l| format!("write:{l:?}")).collect()
}
fn seq(prefix: &[&str], middle: Vec<String>, suffix: &[&str]) -> Vec<String> {
    prefix
        .iter()
        .map(|s| s.to_string())
        .chain(middle)
        .chain(suffix.iter().map(|s| s.to_string()))
        .collect()
}
#[test]
fn transition_matrix_exact_calls() {
    for present in [false, true] {
        let mut e = engine(present, STOPPED);
        e.calls.base.start_type = START_AUTO;
        assert_eq!(e.register(), Ok(()));
        let mut want = if present {
            seq(
                &["open:Install", "config", "state", "boot:Demand"],
                writes(),
                &["config"],
            )
        } else {
            seq(&["open:Install", "create:demand"], writes(), &["config"])
        };
        want.extend(reads());
        want.push("state".into());
        assert_eq!(e.calls.calls, want);
        assert_eq!(e.calls.base.start_type, START_DEMAND);
    }
    for state in [STOPPED, RUNNING, START_PENDING] {
        let mut e = engine(true, state);
        if state == START_PENDING {
            e.calls.states = VecDeque::from([START_PENDING, RUNNING]);
        }
        assert_eq!(e.start(), Ok(()));
        let mut tail = vec!["state", "boot:Auto", "write:Delayed"];
        if state == STOPPED {
            tail.push("start");
        }
        tail.push("state");
        assert_eq!(
            e.calls.calls,
            seq(&["open:Start", "config"], reads(), &tail)
        );
        assert_eq!(e.calls.base.start_type, START_AUTO);
    }
    for state in [STOPPED, RUNNING, PAUSED, PAUSE_PENDING, CONTINUE_PENDING] {
        let mut e = engine(true, state);
        assert_eq!(e.stop(), Ok(()));
        let want = if state == STOPPED {
            vec!["open:Stop", "config", "state", "boot:Demand"]
        } else {
            vec![
                "open:Stop",
                "config",
                "state",
                "control",
                "state",
                "boot:Demand",
            ]
        };
        assert_eq!(e.calls.calls, want);
    }
    for state in [RUNNING, START_PENDING, STOP_PENDING, PAUSED, 99] {
        let mut e = engine(true, state);
        assert_eq!(e.register(), Err(Error::Active));
        assert_eq!(e.calls.calls, ["open:Install", "config", "state"]);
    }
    let mut e = engine(false, STOPPED);
    assert!(e.deregister().is_ok());
    assert_eq!(e.calls.calls, ["open:Uninstall"]);
    for marked in [false, true] {
        let mut e = engine(true, if marked { STOPPED } else { RUNNING });
        e.calls.marked = marked;
        assert!(e.deregister().is_ok());
        let want = if marked {
            vec![
                "open:Uninstall",
                "config",
                "state",
                "boot:Demand",
                "delete",
                "state",
            ]
        } else {
            vec![
                "open:Uninstall",
                "config",
                "state",
                "control",
                "state",
                "boot:Demand",
                "delete",
                "state",
            ]
        };
        assert_eq!(e.calls.calls, want);
    }
    for start in [true, false] {
        let mut e = engine(false, STOPPED);
        assert_eq!(
            if start { e.start() } else { e.stop() },
            Err(Error::NotInstalled)
        );
        assert_eq!(e.calls.calls.len(), 1);
    }
    for idx in 0..6 {
        let mut e = engine(true, STOPPED);
        e.calls.values[idx] = Config2::Delayed(0);
        assert_eq!(e.start(), Err(Error::NotInstalled));
        assert!(!e.calls.calls.iter().any(|s| s.starts_with("boot:")));
        assert_eq!(e.status(), Err(Error::NotInstalled));
        assert_eq!(e.register(), Ok(()));
        assert_eq!(e.start(), Ok(()));
    }
}
#[test]
fn classification_matrix() {
    let mut variants = vec![];
    for t in [0x110, 0x20, 0x50] {
        let mut b = FakeScm::new().base;
        b.service_type = t;
        variants.push(b);
    }
    for t in [0, 1, 4] {
        let mut b = FakeScm::new().base;
        b.start_type = t;
        variants.push(b);
    }
    let original = String::from_utf16(&FakeScm::new().base.image).unwrap();
    for image in [
        original.to_lowercase(),
        original.replace('"', ""),
        format!("{original} "),
        original.replace("run", "stop"),
        original.replacen('"', "", 1),
    ] {
        let mut b = FakeScm::new().base;
        b.image = image.encode_utf16().collect();
        variants.push(b);
    }
    for a in ["localsystem", r".\LocalSystem", r"NT AUTHORITY\SYSTEM"] {
        let mut b = FakeScm::new().base;
        b.account = a.encode_utf16().collect();
        variants.push(b);
    }
    for b in variants {
        for action in 0..6 {
            let mut e = engine(true, STOPPED);
            e.calls.base = b.clone();
            let result = match action {
                0 => e.observe().map(|o| {
                    assert_eq!(o, Observation::Foreign);
                }),
                1 => e.register(),
                2 => e.start(),
                3 => e.stop(),
                4 => e.status().map(|_| ()),
                _ => e.deregister().map(|_| ()),
            };
            if action != 0 {
                assert_eq!(result, Err(Error::Foreign));
            }
            assert_eq!(e.calls.calls.len(), 2);
        }
    }
    let mut e = engine(true, STOPPED);
    e.calls.values[2] = Config2::Privileges(vec![]);
    assert_eq!(e.stop(), Ok(()));
    assert!(e.deregister().is_ok());
}
#[test]
fn installed_exact_set_and_failure_policy() {
    let mut e = engine(true, STOPPED);
    if let Config2::Privileges(p) = &mut e.calls.values[2] {
        p.reverse();
    }
    assert_eq!(e.status(), Ok(Status::Stopped));
    let mut bad = vec![
        Config2::Delayed(0),
        Config2::Sid(0),
        Config2::Preshutdown(1),
        Config2::NonCrash(1),
    ];
    let r = registration(&Layout::for_test(Platform::Windows));
    let mut p = r
        .privileges
        .map(|s| s.encode_utf16().collect::<Vec<_>>())
        .to_vec();
    p[3] = p[0].clone();
    bad.push(Config2::Privileges(p));
    for (actions, reboot, command, reset) in [
        (vec![(1, 5000), (1, 30000)], vec![], vec![], 86400),
        (vec![(1, 5000), (1, 30000), (0, 0)], vec![65], vec![], 86400),
        (vec![(1, 5000), (1, 30000), (0, 0)], vec![], vec![65], 86400),
        (vec![(1, 5000), (1, 30000), (0, 0)], vec![], vec![], 1),
    ] {
        bad.push(Config2::Failure {
            actions,
            reboot,
            command,
            reset,
        });
    }
    for v in bad {
        let mut e = engine(true, STOPPED);
        let i = LEVELS.iter().position(|l| *l == v.level()).unwrap();
        e.calls.values[i] = v;
        assert_eq!(e.status(), Err(Error::NotInstalled));
        assert_eq!(e.start(), Err(Error::NotInstalled));
    }
}
#[test]
fn status_all_states_and_exit_pairs() {
    for (s, want) in [
        (STOPPED, Status::Stopped),
        (START_PENDING, Status::Starting),
        (RUNNING, Status::Running),
        (STOP_PENDING, Status::Stopping),
        (CONTINUE_PENDING, Status::Failed),
        (PAUSE_PENDING, Status::Failed),
        (PAUSED, Status::Failed),
    ] {
        let mut e = engine(true, s);
        assert_eq!(e.status(), Ok(want));
    }
    for (w, s, want) in [
        (0, 0, Status::Stopped),
        (1077, 0, Status::Stopped),
        (1066, 7, Status::Failed),
        (2, 0, Status::Failed),
        (1077, 5, Status::Failed),
    ] {
        let mut e = engine(true, STOPPED);
        e.calls.snapshot.win32_exit = w;
        e.calls.snapshot.specific_exit = s;
        assert_eq!(e.status(), Ok(want));
    }
    assert_eq!(engine(true, 99).status(), Err(Error::Manager));
    assert_eq!(engine(false, STOPPED).status(), Ok(Status::NotInstalled));
}
#[test]
fn deadlines_and_one_successful_control() {
    let mut e = engine(true, RUNNING);
    e.calls.sticky = true;
    assert_eq!(e.stop(), Err(Error::Manager));
    assert_eq!(e.clock.now(), STOP_DEADLINE);
    assert_eq!(e.calls.calls.iter().filter(|s| *s == "control").count(), 1);
    assert!(e.calls.calls.iter().filter(|s| *s == "state").count() <= 122);
    assert!(
        !e.calls
            .calls
            .iter()
            .any(|s| s.starts_with("boot:") || s == "delete")
    );
    let mut e = engine(true, START_PENDING);
    e.calls.sticky = true;
    assert_eq!(e.start(), Err(Error::Manager));
    assert_eq!(e.clock.now(), START_DEADLINE);
    for code in [1061, 1062] {
        let mut e = engine(true, RUNNING);
        e.calls.fail = Some((4, code));
        e.calls.states = VecDeque::from([RUNNING, START_PENDING, RUNNING, STOPPED]);
        assert_eq!(e.stop(), Ok(()));
        assert_eq!(e.calls.calls.iter().filter(|s| *s == "control").count(), 2);
    }
}
#[test]
fn per_call_faults_halt_without_proof() {
    let codes = [5, 1056, 1060, 1061, 1062, 1066, 1072, 1073, 1077, 1078, 122];
    for op in 0..6 {
        let mut baseline = engine(true, STOPPED);
        let invoke = |e: &mut WindowsScm<FakeScm, FakeClock>| match op {
            0 => e.observe().map(|_| ()),
            1 => e.register(),
            2 => e.start(),
            3 => e.stop(),
            4 => e.status().map(|_| ()),
            _ => e.deregister().map(|_| ()),
        };
        assert_eq!(invoke(&mut baseline), Ok(()));
        for i in 1..=baseline.calls.calls.len() {
            for code in codes {
                let mut e = engine(true, STOPPED);
                e.calls.fail = Some((i, code));
                let result = invoke(&mut e);
                let call = &baseline.calls.calls[i - 1];
                if i == 1 && code == ABSENT {
                    assert_eq!(
                        result,
                        if op == 2 || op == 3 {
                            Err(Error::NotInstalled)
                        } else {
                            Ok(())
                        }
                    );
                    if op != 1 {
                        assert_eq!(e.calls.calls.len(), 1);
                    }
                    continue;
                }
                let accepted = op == 5
                    && ((code == MARKED && (call == "boot:Demand" || call == "delete"))
                        || (code == ABSENT && i == baseline.calls.calls.len()));
                if accepted {
                    assert_eq!(result, Ok(()));
                } else {
                    assert_eq!(result, Err(Error::Manager));
                    assert_eq!(
                        e.calls.calls.len(),
                        if op == 2 && call == "start" && code == ALREADY_RUNNING {
                            i + 1
                        } else {
                            i
                        }
                    );
                }
            }
            if baseline.calls.calls[i - 1] == "config"
                || baseline.calls.calls[i - 1] == "state"
                || baseline.calls.calls[i - 1].starts_with("read:")
            {
                let mut e = engine(true, STOPPED);
                e.calls.malformed = Some(i);
                assert_eq!(invoke(&mut e), Err(Error::Manager));
                assert_eq!(e.calls.calls.len(), i);
            }
        }
    }
    let mut e = engine(true, STOPPED);
    e.calls.states = VecDeque::from([STOPPED, RUNNING]);
    assert!(matches!(e.deregister(), Err(Error::Manager)));
}
#[test]
fn boot_and_crash_prefix_model() {
    let mut e = engine(false, STOPPED);
    assert_eq!(e.register(), Ok(()));
    e.calls.reboot_model();
    assert_eq!(e.calls.snapshot.state, STOPPED);
    assert_eq!(e.start(), Ok(()));
    e.calls.reboot_model();
    assert_eq!(e.calls.snapshot.state, RUNNING);
    assert_eq!(e.register(), Err(Error::Active));
    e.calls.reboot_model();
    assert_eq!(e.calls.snapshot.state, RUNNING);
    assert_eq!(e.stop(), Ok(()));
    e.calls.reboot_model();
    assert_eq!(e.calls.snapshot.state, STOPPED);
    let mut e = engine(true, STOPPED);
    e.calls.fail = Some((12, 5));
    assert_eq!(e.start(), Err(Error::Manager));
    e.calls.reboot_model();
    assert_eq!(e.calls.snapshot.state, RUNNING);
    e.calls.snapshot.state = STOPPED;
    e.calls.fail = None;
    assert_eq!(e.register(), Ok(()));
    e.calls.reboot_model();
    assert_eq!(e.calls.snapshot.state, STOPPED);
    for present in [false, true] {
        let mut baseline = engine(present, STOPPED);
        assert_eq!(baseline.register(), Ok(()));
        for k in 1..=baseline.calls.calls.len() {
            let mut e = engine(present, STOPPED);
            e.calls.fail = Some((k, 5));
            assert_eq!(e.register(), Err(Error::Manager));
            e.calls.fail = None;
            assert_eq!(e.register(), Ok(()));
            e.calls.reboot_model();
            assert_eq!(e.calls.snapshot.state, STOPPED);
        }
    }
    let mut e = engine(true, RUNNING);
    e.calls.base.start_type = START_AUTO;
    e.calls.sticky = true;
    assert_eq!(e.stop(), Err(Error::Manager));
    e.calls.reboot_model();
    assert_eq!(e.calls.snapshot.state, RUNNING);
}
#[test]
fn command_line_exact_and_rejections() {
    let mut r = registration(&Layout::for_test(Platform::Windows));
    assert_eq!(
        command_line(&r).unwrap(),
        format!("\"{}\" service run", r.image.display())
            .encode_utf16()
            .collect::<Vec<_>>()
    );
    for p in ["relative.exe", "C:\\bad\".exe", "C:\\bad\0.exe"] {
        r.image = p.into();
        assert!(matches!(command_line(&r), Err(Error::Layout)));
    }
    r.image = r"C:\Program Files\Arxcovo\arxd.exe".into();
    assert_eq!(
        command_line(&r).unwrap(),
        r#""C:\Program Files\Arxcovo\arxd.exe" service run"#
            .encode_utf16()
            .collect::<Vec<_>>()
    );
}

#[test]
fn source_boundaries() {
    let files = [
        include_str!("owner_scm.rs"),
        include_str!("owner_scm_windows.rs"),
    ];
    let forbidden = [
        ["owner_", "transactions"],
        ["Owner", "Store"],
        ["Batch", "Right"],
        ["uninstall_", "right"],
        ["parts", "("],
    ]
    .map(|p| p.concat());
    for source in files {
        for word in &forbidden {
            assert!(!source.contains(word.as_str()));
        }
    }
    let source = files[0];
    let body = source.split("pub fn deregister(").nth(1).unwrap();
    let prefix = source.split("pub fn deregister(").next().unwrap();
    assert_eq!(
        prefix
            .replace("pub struct Deregistered(());", "")
            .matches("Deregistered(())")
            .count(),
        0
    );
    assert_eq!(body.matches("Deregistered(())").count(), 2);
    let test_sources = [
        include_str!("owner_scm_tests.rs"),
        include_str!("owner_scm_windows_tests.rs"),
    ];
    // Join fragments so this source guard does not match its own forbidden literals.
    for name in [
        ["Native", "Scm"],
        ["WindowsScm::", "system"],
        ["OpenSC", "ManagerW"],
        ["Create", "ServiceW"],
    ] {
        for source in test_sources {
            assert!(!source.contains(&name.concat()));
        }
    }
}
#[test]
fn failure_after_stop_keeps_auto_until_rerun() {
    let mut e = engine(true, RUNNING);
    e.calls.base.start_type = START_AUTO;
    e.calls.fail = Some((6, 5));
    assert_eq!(e.stop(), Err(Error::Manager));
    assert_eq!(e.calls.snapshot.state, STOPPED);
    assert_eq!(e.calls.base.start_type, START_AUTO);
    e.calls.fail = None;
    assert_eq!(e.stop(), Ok(()));
    assert_eq!(e.calls.base.start_type, START_DEMAND);
}

#[test]
fn create_races_and_start_already_running() {
    let mut e = engine(false, STOPPED);
    e.calls.race = true;
    assert_eq!(e.register(), Ok(()));
    assert_eq!(
        &e.calls.calls[..6],
        [
            "open:Install",
            "create:demand",
            "open:Install",
            "config",
            "state",
            "boot:Demand"
        ]
    );
    for code in [EXISTS, DUPLICATE, MARKED, 5] {
        let mut e = engine(false, STOPPED);
        e.calls.fail = Some((2, code));
        assert_eq!(
            e.register(),
            Err(if code == DUPLICATE {
                Error::Foreign
            } else {
                Error::Manager
            })
        );
        assert_eq!(e.calls.calls.len(), if code == EXISTS { 3 } else { 2 });
    }
    let mut e = engine(true, STOPPED);
    e.calls.fail = Some((12, ALREADY_RUNNING));
    e.calls.states = VecDeque::from([STOPPED, RUNNING]);
    assert_eq!(e.start(), Ok(()));
    assert_eq!(e.calls.base.start_type, START_AUTO);
}
#[test]
fn running_deregister_fault_table() {
    let codes = [5, 1056, 1060, 1061, 1062, 1066, 1072, 1073, 1077, 1078, 122];
    for i in 1..=8 {
        for code in codes {
            let mut e = engine(true, RUNNING);
            e.calls.fail = Some((i, code));
            let result = e.deregister();
            let accepted = (i == 4 && matches!(code, CANNOT_ACCEPT | NOT_ACTIVE))
                || ((i == 6 || i == 7) && code == MARKED)
                || ((i == 1 || i == 8) && code == ABSENT);
            assert_eq!(result.is_ok(), accepted, "call {i}, code {code}");
            if !accepted {
                assert_eq!(e.calls.calls.len(), i);
            }
        }
    }
    for i in [2, 3, 5, 8] {
        let mut e = engine(true, RUNNING);
        e.calls.malformed = Some(i);
        assert!(matches!(e.deregister(), Err(Error::Manager)));
        assert_eq!(e.calls.calls.len(), i);
    }
}
#[test]
fn observe_is_read_only_for_absent_partial_and_active() {
    assert_eq!(engine(false, STOPPED).observe(), Ok(Observation::Absent));
    for state in [STOPPED, RUNNING, START_PENDING, 99] {
        for installed in [false, true] {
            let mut e = engine(true, state);
            if !installed {
                e.calls.values[0] = Config2::Delayed(0);
            }
            assert_eq!(
                e.observe(),
                Ok(Observation::Ours {
                    boot: Boot::Demand,
                    installed,
                    active: state != STOPPED
                })
            );
            assert_eq!(
                e.calls.calls,
                seq(&["open:Status", "config"], reads(), &["state"])
            );
        }
    }
}
