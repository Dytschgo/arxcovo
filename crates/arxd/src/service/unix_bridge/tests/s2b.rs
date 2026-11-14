use super::e6::{activity, call, refused};
use super::*;
use std::sync::{Mutex, atomic::AtomicBool};

struct Observed {
    bridge: UnixBridge,
    entries: Arc<AtomicUsize>,
}
impl Executor for Observed {
    fn preflight(&self) -> Result<(), Unavailable> {
        self.bridge.preflight()
    }
    fn run(&self, r: Option<&Request>, c: &Cancellation, h: &Cancellation) -> Report {
        self.entries.fetch_add(1, Ordering::AcqRel);
        self.bridge.run(r, c, h)
    }
}
fn port_bridge(port: Box<dyn RunnerPort>, state: Readiness) -> UnixBridge {
    UnixBridge::new(
        &Layout::for_test(Platform::Linux),
        &config(),
        Some("/synthetic/runs".into()),
        |_| Ok(port),
        Box::new(|| Ok(())),
        state,
    )
}

#[cfg(target_os = "linux")]
#[test]
fn s2b_real_group_unwinds_before_bridge_returns() {
    if unsafe { libc::getuid() } == 0 || unsafe { libc::geteuid() } == 0 {
        eprintln!("SKIP S2b ordinary-owned constructor requires nonroot");
        return;
    }
    struct Holding {
        group: Mutex<Option<exec::TestGroup>>,
        residue: Arc<AtomicBool>,
    }
    impl RunnerPort for Holding {
        fn cleanup_residue(&self) -> bool {
            self.residue.load(Ordering::Acquire)
        }
        fn run(
            &self,
            _: &exec::RunSpec,
            _: &Cancellation,
            _: &Cancellation,
        ) -> Result<exec::RunResult, exec::RunError> {
            let _group = self.group.lock().unwrap().take().unwrap();
            panic!("ARX-S2B-FIXED-PANIC");
        }
    }
    let base = tempfile::tempdir().unwrap();
    let residue = Arc::new(AtomicBool::new(false));
    let group = exec::TestGroup::owned(&base, residue.clone());
    let state = ready();
    let b = port_bridge(
        Box::new(Holding {
            group: Mutex::new(Some(group)),
            residue: residue.clone(),
        }),
        state.clone(),
    );
    assert!(
        matches!(
            Executor::run(
                &b,
                Some(&request()),
                &Cancellation::default(),
                &Cancellation::default()
            ),
            Report::Failed {
                creation: Creation::Unknown
            }
        ),
        "S2b Failed Unknown"
    );
    assert!(
        residue.load(Ordering::Acquire),
        "S2b real Drop published before return"
    );
    assert_eq!(
        state.check(),
        Err(Unavailable::MechanismUnavailable),
        "S2b latch before return"
    );
    assert!(
        base.path().join("leaf/cgroup.events").exists(),
        "owned leaf remains nonempty"
    );
}

async fn stale_admission(panic_first: bool) {
    let residue = Arc::new(AtomicBool::new(false));
    let flag = residue.clone();
    let calls = Arc::new(AtomicUsize::new(0));
    let state = ready();
    let b = port_bridge(
        Box::new(e6::Port {
            residue: residue.clone(),
            calls: calls.clone(),
            step: Box::new(move |_, _, _| {
                if panic_first {
                    flag.store(true, Ordering::Release);
                    panic!("ARX-S2B-GATE-PANIC");
                }
                Ok(result())
            }),
        }),
        state.clone(),
    );
    let entries = Arc::new(AtomicUsize::new(0));
    let observed = Arc::new(Observed {
        bridge: b,
        entries: entries.clone(),
    });
    let mut v = fixture();
    v["limits"]["max_concurrent_runs"] = json!(1);
    let cfg = parse(v, &FakeResolver::default()).unwrap();
    let e = Execution::unix_bridge_for_test(observed.clone(), &cfg);
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let (release, held) = std::sync::mpsc::sync_channel(1);
    let held = Mutex::new(held);
    let seq = AtomicUsize::new(0);
    e.admission_barrier_for_test(Arc::new(move || {
        if seq.fetch_add(1, Ordering::AcqRel) == 0 {
            tx.send(()).unwrap();
            held.lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
        }
    }));
    let dir = private_temp();
    let a = activity(&dir);
    let id = a.start_dedicated("alpha", "run_command").unwrap();
    let pending = e.dispatch(Some(request()), a.clone(), id);
    tokio::task::spawn_blocking(move || rx.recv_timeout(Duration::from_secs(5)).unwrap())
        .await
        .unwrap();
    assert_eq!(state.check(), Ok(()), "pre-admitted while Ready");
    if panic_first {
        assert!(call(&e, &a, false).await.is_ok(), "panic dispatch returned");
        assert_eq!(calls.load(Ordering::Acquire), 1);
        assert_eq!(
            observed.preflight(),
            Err(Unavailable::MechanismUnavailable),
            "ordinary postpanic preflight"
        );
        let entered = entries.load(Ordering::Acquire);
        assert!(
            call(&e, &a, false).await.is_err(),
            "ordinary postpanic early refusal"
        );
        assert_eq!(
            entries.load(Ordering::Acquire),
            entered,
            "early refusal never enters bridge"
        );
    } else {
        residue.store(true, Ordering::Release);
        assert_eq!(
            observed.preflight(),
            Ok(()),
            "independent Ready residue case"
        );
    }
    assert_eq!(
        e.active_for_test(),
        0,
        "actual slot observed free before release"
    );
    let before_entries = entries.load(Ordering::Acquire);
    let before_calls = calls.load(Ordering::Acquire);
    release.send(()).unwrap();
    let out = pending.await;
    assert_eq!(
        entries.load(Ordering::Acquire),
        before_entries + 1,
        "actual bridge gate entered after admission"
    );
    assert_eq!(
        calls.load(Ordering::Acquire),
        before_calls,
        "S2b gate refuses zero additional Runner calls"
    );
    assert_eq!(
        state.check(),
        Err(Unavailable::MechanismUnavailable),
        "S2b containment latched"
    );
    refused(out, Unavailable::MechanismUnavailable);
    e.drain().await.unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s2b_latched_readiness_gate() {
    stale_admission(true).await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s2b_independent_residue_gate() {
    stale_admission(false).await;
}

// Deliberately partial lexical guard. It does not parse Rust or prove ownership.
fn ownership_guard(unix: &str, executor: &str) -> bool {
    let forbidden = [
        "forget(",
        "ManuallyDrop",
        "Box::leak",
        "Arc<Group>",
        "Arc<super::cgroup::Group>",
        "thread::spawn",
        "scope.spawn",
    ];
    let code: String = unix
        .lines()
        .map(|l| l.split("//").next().unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    if forbidden.iter().any(|x| code.contains(x)) {
        return false;
    }
    if code.matches("super::cgroup::Group").count() != 2
        || !code.contains("let group = super::cgroup::Group::create(c, residue)")
        || !code.contains("group: super::cgroup::Group,")
    {
        return false;
    }
    let Some(body) = executor.split("workers.spawn_blocking(move || {").nth(1) else {
        return false;
    };
    let mut depth = 1;
    let mut end = None;
    for (i, ch) in body.char_indices() {
        if ch == '{' {
            depth += 1;
        }
        if ch == '}' {
            depth -= 1;
        }
        if depth == 0 {
            end = Some(i);
            break;
        }
    }
    let Some(end) = end else {
        return false;
    };
    let closure = &body[..end];
    let Some(run) = closure.find("this.backend.run(request.as_ref(), &cancel, &this.shutdown)")
    else {
        return false;
    };
    let Some(slot) = closure.find("let r = if cancel.is_cancelled()") else {
        return false;
    };
    let Some(release) = closure[slot..].find("this.active.fetch_sub(1, Ordering::AcqRel)") else {
        return false;
    };
    slot + release > run && closure[run..slot + release].contains("};")
}
#[test]
fn s2b_partial_lexical_inventory_and_controls() {
    let unix = include_str!("../../../exec/unix.rs");
    let executor = include_str!("../../executor.rs");
    assert!(
        ownership_guard(unix, executor),
        "S2b bounded lexical ownership/order guard"
    );
    let forgotten = unix.replacen(
        "let procs_fd = group.procs.as_raw_fd();",
        "let procs_fd = group.procs.as_raw_fd(); std::mem::forget(group);",
        1,
    );
    assert!(
        !ownership_guard(&forgotten, executor),
        "meaningful group-forgotten control"
    );
    let early = executor.replacen("this.backend.run(request.as_ref(), &cancel, &this.shutdown)",
        "this.active.fetch_sub(1, Ordering::AcqRel); this.backend.run(request.as_ref(), &cancel, &this.shutdown)", 1);
    assert!(
        !ownership_guard(unix, &early),
        "meaningful early-slot-release control"
    );
}
