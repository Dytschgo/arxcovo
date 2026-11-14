//! A6 nonnative failure evidence: production setup, no host cgroups or Runner.
use super::*;
use crate::exec::hierarchy::setup::*;
use std::cell::{Cell, RefCell};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Fault {
    Busy,
    Timeout,
    Verify,
    Shutdown,
}

// A closed, test-local model: handles are object labels, never filesystem paths.
struct Fs {
    fault: Fault,
    log: RefCell<Vec<String>>,
    removed: Cell<bool>,
    unit_enabled: Cell<bool>,
    runs_enabled: Cell<bool>,
    pending: Box<dyn Fn()>,
}
impl Fs {
    fn touch(&self, op: &str, dir: &str, name: &str) {
        (self.pending)();
        self.log.borrow_mut().push(format!("{op}:{dir}:{name}"));
    }
}
impl CgroupFs for Fs {
    type Dir = &'static str;
    fn proc_cgroup(&self) -> FsResult<ProcFile> {
        self.touch("proc", "", "cgroup");
        Ok(ProcFile {
            magic: PROC_MAGIC,
            bytes: b"0::/arxd.service/supervisor\n".to_vec(),
        })
    }
    fn proc_status(&self) -> FsResult<ProcFile> {
        self.touch("proc", "", "status");
        Ok(ProcFile {
            magic: PROC_MAGIC,
            bytes: b"NoNewPrivs:\t1\nCapBnd:\t00000000000000c1\n".to_vec(),
        })
    }
    fn identity(&self) -> FsResult<(u32, u32)> {
        self.touch("identity", "", "");
        Ok((0, 0))
    }
    fn pid(&self) -> FsResult<u32> {
        self.touch("pid", "", "");
        Ok(42)
    }
    fn open_root(&self) -> FsResult<Self::Dir> {
        self.touch("root", "", "");
        Ok("root")
    }
    fn open_child(&self, dir: &Self::Dir, name: &[u8]) -> FsResult<Self::Dir> {
        let child = match (*dir, name) {
            ("root", b"arxd.service") => "unit",
            ("unit", b"supervisor") => "supervisor",
            ("unit", b"runs") => "runs",
            ("runs", b"arx-1-0") if !self.removed.get() => "leaf",
            _ => return Err(FsError::Missing),
        };
        self.touch("open", dir, child);
        Ok(child)
    }
    fn stat(&self, dir: &Self::Dir) -> FsResult<Stat> {
        self.touch("stat", dir, "");
        Ok(Stat {
            uid: 0,
            mode: 0o040700,
            magic: CGROUP_MAGIC,
        })
    }
    fn read(&self, dir: &Self::Dir, file: &str, max: usize) -> FsResult<Vec<u8>> {
        self.touch("read", dir, file);
        let bytes = match file {
            "cgroup.type" => b"domain\n".as_slice(),
            "cgroup.procs" if *dir == "supervisor" => b"42\n",
            "cgroup.procs" => b"",
            "cgroup.controllers" => b"memory pids",
            "cgroup.subtree_control"
                if (*dir == "unit" && self.unit_enabled.get())
                    || (*dir == "runs" && self.runs_enabled.get()) =>
            {
                b"memory pids"
            }
            "cgroup.subtree_control" => b"",
            "cgroup.events"
                if *dir == "runs" && matches!(self.fault, Fault::Timeout | Fault::Shutdown) =>
            {
                b"populated 1\nfrozen 0\n"
            }
            "cgroup.events" => b"populated 0\nfrozen 0\n",
            _ => return Err(FsError::Missing),
        };
        assert!(bytes.len() <= max);
        Ok(bytes.to_vec())
    }
    fn write(&self, dir: &Self::Dir, file: &str, bytes: &[u8]) -> FsResult<()> {
        self.touch("write", dir, file);
        match (*dir, file, bytes) {
            ("runs", "cgroup.kill", b"1") => Ok(()),
            ("unit", "cgroup.subtree_control", b"+memory +pids") => {
                self.unit_enabled.set(true);
                Ok(())
            }
            ("runs", "cgroup.subtree_control", b"+memory +pids") => {
                self.runs_enabled.set(true);
                Ok(())
            }
            _ => Err(FsError::Invalid),
        }
    }
    fn xattr_delegate(&self, dir: &Self::Dir) -> FsResult<Option<Vec<u8>>> {
        self.touch("xattr", dir, "");
        Ok(Some(b"1".to_vec()))
    }
    fn mkdir_runs(&self, dir: &Self::Dir) -> FsResult<Created> {
        self.touch("mkdir", dir, "runs");
        Ok(Created::Exists)
    }
    fn list(&self, dir: &Self::Dir, max: usize) -> FsResult<Vec<RawEntry>> {
        self.touch("list", dir, "");
        let names = if *dir == "runs" && !self.removed.get() {
            vec![b"arx-1-0".to_vec()]
        } else if *dir == "runs" && self.fault == Fault::Verify {
            // A fresh entry only after the original leaf was removed successfully.
            vec![b"arx-2-0".to_vec()]
        } else {
            vec![]
        };
        assert!(names.len() <= max);
        Ok(names
            .into_iter()
            .map(|name| RawEntry {
                name,
                d_type: 4,
                fstatat_mode: None,
            })
            .collect())
    }
    fn rmdir(&self, dir: &Self::Dir, name: &[u8]) -> FsResult<()> {
        assert_eq!((*dir, name), ("runs", b"arx-1-0".as_slice()));
        self.touch("rmdir", dir, "arx-1-0");
        if self.fault == Fault::Busy {
            return Err(FsError::Busy);
        }
        self.removed.set(true);
        Ok(())
    }
    fn open_ro(&self, dir: &Self::Dir, file: &str) -> FsResult<()> {
        self.touch("ro", dir, file);
        Ok(())
    }
}
struct Manual {
    elapsed: Cell<Duration>,
    stop: Option<Cancellation>,
}
impl Clock for Manual {
    fn now(&self) -> Duration {
        self.elapsed.get()
    }
    fn sleep(&self, duration: Duration) {
        assert_eq!(duration, Duration::from_millis(5));
        assert!(self.elapsed.get() < Duration::from_secs(5));
        self.elapsed.set(self.elapsed.get() + duration);
        if let Some(stop) = &self.stop {
            stop.cancel();
        }
    }
}

async fn joined(fault: Fault, failure: SetupFailure) {
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let state = Arc::new(std::sync::Mutex::new(None::<Readiness>));
    let during = state.clone();
    let during_calls = calls.clone();
    let d = StartupDomain::new(
        &Layout::for_test(Platform::Linux),
        &config(),
        locate(b"0::/arxd.service/supervisor"),
        || Ok(()),
        move |_| {
            Ok(Box::new(e6::Port {
                calls: count,
                residue: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                step: Box::new(|_, _, _| Ok(result())),
            }))
        },
        move |location, token| {
            let fs = Fs {
                fault,
                log: RefCell::new(vec![]),
                removed: Cell::new(false),
                unit_enabled: Cell::new(false),
                runs_enabled: Cell::new(false),
                pending: Box::new(move || {
                    assert_eq!(
                        during.lock().unwrap().as_ref().unwrap().check(),
                        Err(Unavailable::MechanismUnavailable),
                        "A6 Pending throughout actual setup"
                    );
                    assert_eq!(during_calls.load(Ordering::Acquire), 0);
                }),
            };
            let clock = Manual {
                elapsed: Cell::new(Duration::ZERO),
                stop: (fault == Fault::Shutdown).then(|| token.clone()),
            };
            let outcome = setup(&fs, location, &clock, token);
            assert_eq!(
                outcome,
                SetupOutcome::Unavailable(failure),
                "actual setup failure"
            );
            let log = fs.log.borrow();
            assert_eq!(
                log.iter()
                    .filter(|op| *op == "write:runs:cgroup.kill")
                    .count(),
                1
            );
            assert!(!fs.runs_enabled.get(), "failure cannot enable runs");
            if matches!(fault, Fault::Busy | Fault::Verify) {
                let remove = log
                    .iter()
                    .position(|op| op == "rmdir:runs:arx-1-0")
                    .unwrap();
                for validation in ["stat:leaf:", "list:leaf:", "read:leaf:cgroup.events"] {
                    assert!(
                        log.iter().position(|op| op == validation).unwrap() < remove,
                        "validated leaf before removal"
                    );
                }
                assert_eq!(fs.removed.get(), fault == Fault::Verify);
                if fault == Fault::Verify {
                    assert_eq!(
                        log.last().unwrap(),
                        "list:runs:",
                        "fresh verification after successful rmdir"
                    );
                }
            } else {
                assert!(!log.iter().any(|op| op.starts_with("rmdir:")));
                assert_eq!(
                    clock.now(),
                    if fault == Fault::Timeout {
                        Duration::from_secs(5)
                    } else {
                        Duration::from_millis(5)
                    }
                );
            }
            outcome
        },
    );
    *state.lock().unwrap() = Some(d.readiness.clone());
    let dir = private_temp();
    let a = e6::activity(&dir);
    e6::refused(
        e6::call(&d.domain.handle(), &a, false).await,
        Unavailable::MechanismUnavailable,
    );
    assert!(!d.setup.lock().unwrap().take().unwrap()(
        Cancellation::default()
    ));
    let expected = if fault == Fault::Shutdown {
        Unavailable::Stopping
    } else {
        Unavailable::Quarantined
    };
    assert_eq!(d.readiness.check(), Err(expected));
    d.readiness.set_ready();
    assert_eq!(
        d.readiness.check(),
        Err(expected),
        "set_ready cannot clear failure"
    );
    for discovery in [false, true, false] {
        let e = d.domain.handle();
        e6::refused(e6::call(&e, &a, discovery).await, expected);
        assert_eq!(e.preflight_for_test(), Err(expected));
    }
    assert_eq!(
        calls.load(Ordering::Acquire),
        0,
        "no runner calls in this domain"
    );
    d.domain.handle().drain().await.unwrap();
    drop(a);
    let reopened = e6::activity(&dir);
    let rows = serde_json::to_value(list(&reopened)).unwrap();
    let rows = rows["activities"].as_array().unwrap();
    assert_eq!(
        rows.len(),
        4,
        "all dispatch rows committed across database reopen"
    );
    for row in &rows[..3] {
        assert_eq!(row["outcome"], "refused");
        assert!(row["finished_at_ms"].is_number(), "committed terminal row");
        assert_eq!(
            row["reason"],
            if fault == Fault::Shutdown {
                "shutdown"
            } else {
                "quarantined"
            }
        );
    }
    assert_eq!(
        rows[3]["reason"], "preflight_failed",
        "initial Pending refusal"
    );
}

#[tokio::test]
async fn a6_nonnative_validated_leaf_busy_join() {
    joined(Fault::Busy, SetupFailure::LeafRemoval).await;
}
#[tokio::test]
async fn a6_nonnative_populated_deadline_join() {
    joined(Fault::Timeout, SetupFailure::KillTimeout).await;
}
#[tokio::test]
async fn a6_nonnative_fresh_verification_residue_join() {
    joined(Fault::Verify, SetupFailure::VerifyFailed).await;
}
#[tokio::test]
async fn a6_nonnative_shutdown_during_janitor_stays_stopping() {
    joined(Fault::Shutdown, SetupFailure::Stopped).await;
}
