use super::*;
use crate::exec::hierarchy::setup::SetupFailure;
use std::sync::atomic::AtomicBool;
struct Port {
    step: Box<Step>,
    residue: Arc<AtomicBool>,
    calls: Arc<AtomicUsize>,
}
impl RunnerPort for Port {
    fn cleanup_residue(&self) -> bool {
        self.residue.load(Ordering::Acquire)
    }
    fn run(
        &self,
        s: &exec::RunSpec,
        c: &Cancellation,
        h: &Cancellation,
    ) -> Result<exec::RunResult, exec::RunError> {
        self.calls.fetch_add(1, Ordering::AcqRel);
        (self.step)(s, c, h)
    }
}
fn domain(
    r: Readiness,
    residue: Arc<AtomicBool>,
    calls: Arc<AtomicUsize>,
    limit: u64,
    step: impl Fn(
        &exec::RunSpec,
        &Cancellation,
        &Cancellation,
    ) -> Result<exec::RunResult, exec::RunError>
    + Send
    + Sync
    + 'static,
) -> AdmissionDomain {
    let mut v = fixture();
    v["limits"]["memory_bytes"] = json!(16777216);
    v["limits"]["max_concurrent_runs"] = json!(limit);
    let c = parse(v, &FakeResolver::default()).unwrap();
    AdmissionDomain::new(
        &Layout::for_test(Platform::Linux),
        &c,
        "/owned-synthetic/runs".into(),
        |_| {
            Ok(Box::new(Port {
                step: Box::new(step),
                residue,
                calls,
            }))
        },
        Box::new(|| Ok(())),
        r,
    )
}
fn activity(dir: &tempfile::TempDir) -> Arc<Activity> {
    Arc::new(Activity::open_service_fixture(dir.path()).unwrap())
}
async fn call(
    e: &Arc<Execution>,
    a: &Arc<Activity>,
    discovery: bool,
) -> Result<Value, crate::service::executor::Failure> {
    let id = a
        .start_dedicated(
            "alpha",
            if discovery {
                "discover_tools"
            } else {
                "run_command"
            },
        )
        .unwrap();
    e.dispatch(
        if discovery { None } else { Some(request()) },
        a.clone(),
        id,
    )
    .await
}
fn refused(out: Result<Value, crate::service::executor::Failure>, token: Unavailable) {
    assert!(
        out.err().is_some_and(|f| f.execution_reason == Some(token)),
        "fixed refusal"
    );
}
fn row(a: &Activity, outcome: &str, reason: Option<&str>, stop: Option<&str>) {
    let p = serde_json::to_value(list(a)).unwrap();
    let r = &p["activities"][0];
    assert!(r["outcome"] == outcome, "fixed row outcome");
    if let Some(reason) = reason {
        assert!(r["reason"] == reason, "fixed row reason");
    }
    if let Some(stop) = stop {
        assert!(r["stop_reason"] == stop, "fixed row stop");
    }
}
#[tokio::test]
async fn t11_t12_t13_t13b_reports_and_latches() {
    use exec::{NotStarted as N, RunError::NotStarted};
    for error in [N::Containment, N::Resource, N::Cancelled] {
        for residue_on in [false, true] {
            let residue = Arc::new(AtomicBool::new(false));
            let flag = residue.clone();
            let calls = Arc::new(AtomicUsize::new(0));
            let readiness = ready();
            let observed = readiness.clone();
            let d = domain(readiness, residue, calls.clone(), 1, move |_, _, _| {
                flag.store(residue_on, Ordering::Release);
                Err(NotStarted(error))
            });
            let e = d.handle();
            let dir = private_temp();
            let a = activity(&dir);
            let out = call(&e, &a, false).await;
            match error {
                N::Containment => {
                    refused(out, Unavailable::MechanismUnavailable);
                    row(&a, "refused", Some("preflight_failed"), None);
                }
                N::Resource => {
                    assert!(out.is_ok(), "row F response");
                    row(&a, "failed", Some("execution_failed"), None);
                }
                N::Cancelled => {
                    assert!(out.is_ok(), "row E response");
                    row(&a, "failed", None, Some("cancelled"));
                }
                _ => unreachable!(),
            }
            if residue_on {
                assert!(
                    observed.check() == Err(Unavailable::MechanismUnavailable),
                    "post call latch before next dispatch"
                );
            }
            if error == N::Cancelled && !residue_on {
                for _ in 0..5 {
                    assert!(call(&e, &a, false).await.is_ok(), "ordinary cancellation");
                }
                assert!(calls.load(Ordering::Acquire) == 6, "six port calls");
            } else {
                refused(call(&e, &a, false).await, Unavailable::MechanismUnavailable);
                assert!(calls.load(Ordering::Acquire) == 1, "no later port call");
                row(&a, "refused", Some("preflight_failed"), None);
            }
            e.drain().await.unwrap();
        }
    }
}
fn probe(sudo: &str) -> exec::RunResult {
    let mut r = result();
    let text = String::from_utf8(table()).unwrap().replace("absent", sudo);
    r.stdout = exec::CapturedOutput {
        head: text.into_bytes(),
        tail: vec![],
        dropped_bytes: 0,
    };
    r
}
#[tokio::test]
async fn t14_t17_rights_tools_and_private_bytes() {
    for sudo in [
        "exit_zero",
        "untrusted",
        "unknown",
        "absent",
        "exit_nonzero",
    ] {
        let calls = Arc::new(AtomicUsize::new(0));
        let d = domain(
            ready(),
            Arc::new(AtomicBool::new(false)),
            calls.clone(),
            1,
            move |_, _, _| Ok(probe(sudo)),
        );
        let e = d.handle();
        let dir = private_temp();
        let a = activity(&dir);
        let out = call(&e, &a, true).await.ok().unwrap();
        assert!(
            out["tools"]
                .as_array()
                .is_some_and(|t| t.len() == TOOLS.len()),
            "row N tools"
        );
        row(&a, "succeeded", None, None);
        if ["exit_zero", "untrusted", "unknown"].contains(&sudo) {
            refused(call(&e, &a, false).await, Unavailable::RightsMismatch);
            row(&a, "refused", Some("preflight_failed"), None);
            assert!(calls.load(Ordering::Acquire) == 1, "rights latch");
        } else {
            assert!(call(&e, &a, false).await.is_ok(), "rights allowed");
            assert!(calls.load(Ordering::Acquire) == 2, "second call");
        }
        let rows = serde_json::to_string(&list(&a)).unwrap();
        for text in [out.to_string(), rows] {
            assert!(
                !text.contains("no_new_privs")
                    && !text.contains("sudo")
                    && !text.contains("{\"rights\":{"),
                "private rights withheld"
            );
        }
        e.drain().await.unwrap();
        drop(a);
        drop(e);
        drop(d);
        for entry in std::fs::read_dir(dir.path()).unwrap() {
            let p = entry.unwrap().path();
            if p.is_file() {
                let b = std::fs::read(p).unwrap();
                for marker in [b"no_new_privs".as_slice(), b"sudo", b"{\"rights\""] {
                    assert!(
                        !b.windows(marker.len()).any(|w| w == marker),
                        "private database bytes"
                    );
                }
            }
        }
    }
}
#[tokio::test]
async fn t15_exhaustive_setup_failure_dispatch() {
    use SetupFailure::*;
    for f in [
        Hardening,
        Identity,
        Locate,
        Root,
        Unit,
        Delegation,
        UnitState,
        Supervisor,
        EnableUnit,
        Runs,
        KillInterface,
        KillTimeout,
        LeafStructure,
        LeafRemoval,
        VerifyFailed,
        EnableRuns,
        Postcondition,
        Stopped,
        Panicked,
    ] {
        let (token, stored) = match f {
            Hardening | Identity | Locate | Root | Unit | Delegation | UnitState | Supervisor
            | EnableUnit | Runs | KillInterface | LeafStructure | EnableRuns | Postcondition => {
                (Unavailable::MechanismUnavailable, "preflight_failed")
            }
            KillTimeout | LeafRemoval | VerifyFailed | Panicked => {
                (Unavailable::Quarantined, "quarantined")
            }
            Stopped => (Unavailable::Stopping, "shutdown"),
        };
        let r = Readiness::pending();
        r.fail(f.into());
        let calls = Arc::new(AtomicUsize::new(0));
        let d = domain(
            r,
            Arc::new(AtomicBool::new(false)),
            calls.clone(),
            1,
            |_, _, _| Ok(result()),
        );
        let e = d.handle();
        let dir = private_temp();
        let a = activity(&dir);
        refused(call(&e, &a, false).await, token);
        row(&a, "refused", Some(stored), None);
        assert!(calls.load(Ordering::Acquire) == 0, "setup zero calls");
        e.drain().await.unwrap();
    }
}
#[tokio::test]
async fn t16_monotone_first_cause() {
    let r = Readiness::pending();
    assert!(
        r.check() == Err(Unavailable::MechanismUnavailable),
        "pending"
    );
    r.set_ready();
    r.fail(Cause::Janitor);
    assert!(r.check().is_ok(), "late fail ignored");
    let barrier = Arc::new(std::sync::Barrier::new(9));
    let mut workers = Vec::new();
    for i in 0..8 {
        let r = r.clone();
        let b = barrier.clone();
        workers.push(std::thread::spawn(move || {
            b.wait();
            r.latch(if i % 2 == 0 {
                Cause::Rights
            } else {
                Cause::Containment
            });
            r.check()
        }));
    }
    r.latch(Cause::Rights);
    barrier.wait();
    let values: Vec<_> = workers.into_iter().map(|w| w.join().unwrap()).collect();
    assert!(
        values.iter().all(|v| *v == values[0]) && values[0].is_err(),
        "first cause retained"
    );
    assert!(
        r.check() == Err(Unavailable::RightsMismatch),
        "initial writer wins"
    );
    r.set_ready();
    r.fail(Cause::Stopped);
    assert!(r.check() == values[0], "no reset");
    let d = domain(
        r,
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicUsize::new(0)),
        1,
        |_, _, _| Ok(result()),
    );
    let e = d.handle();
    let dir = private_temp();
    let a = activity(&dir);
    refused(call(&e, &a, false).await, values[0].unwrap_err());
    e.drain().await.unwrap();
}
// Bounded, cancellation-aware rendezvous: a broken admission gate fails instead
// of leaving a blocking worker and drain stuck forever.
fn wait_until(predicate: impl Fn() -> bool) {
    let end = std::time::Instant::now() + Duration::from_secs(5);
    while !predicate() && std::time::Instant::now() < end {
        std::thread::yield_now();
    }
    assert!(predicate(), "bounded rendezvous");
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn t18_stale_preflight_gate_limit_one() {
    for cancellation in [false, true] {
        let residue = Arc::new(AtomicBool::new(false));
        let flag = residue.clone();
        let calls = Arc::new(AtomicUsize::new(0));
        let d = domain(ready(), residue, calls.clone(), 1, move |_, _, _| {
            flag.store(cancellation, Ordering::Release);
            Err(exec::RunError::NotStarted(if cancellation {
                exec::NotStarted::Cancelled
            } else {
                exec::NotStarted::Containment
            }))
        });
        let e = d.handle();
        let arrived = Arc::new(AtomicBool::new(false));
        let release = Arc::new(AtomicBool::new(false));
        let sequence = Arc::new(AtomicUsize::new(0));
        let (seen, go, seq) = (arrived.clone(), release.clone(), sequence.clone());
        e.admission_barrier_for_test(Arc::new(move || {
            if seq.fetch_add(1, Ordering::AcqRel) == 0 {
                seen.store(true, Ordering::Release);
                wait_until(|| go.load(Ordering::Acquire));
            }
        }));
        let dir = private_temp();
        let a = activity(&dir);
        let id = a.start_dedicated("alpha", "run_command").unwrap();
        let w1 = e.dispatch(Some(request()), a.clone(), id);
        tokio::time::timeout(Duration::from_secs(5), async {
            while !arrived.load(Ordering::Acquire) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let w2 = call(&e, &a, false).await;
        if cancellation {
            assert!(w2.is_ok(), "unchanged E");
        } else {
            refused(w2, Unavailable::MechanismUnavailable);
        }
        release.store(true, Ordering::Release);
        refused(w1.await, Unavailable::MechanismUnavailable);
        assert!(calls.load(Ordering::Acquire) == 1, "binding slot gate");
        e.drain().await.unwrap();
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn t19_four_residue_calls_bound() {
    let residue = Arc::new(AtomicBool::new(false));
    let flag = residue.clone();
    let calls = Arc::new(AtomicUsize::new(0));
    let arrived = calls.clone();
    let release = Arc::new(AtomicBool::new(false));
    let go = release.clone();
    let d = domain(ready(), residue, calls.clone(), 4, move |_, c, h| {
        wait_until(|| go.load(Ordering::Acquire) || c.is_cancelled() || h.is_cancelled());
        flag.store(true, Ordering::Release);
        Err(exec::RunError::NotStarted(exec::NotStarted::Cancelled))
    });
    let e = d.handle();
    let dir = private_temp();
    let a = activity(&dir);
    let mut futures = Vec::new();
    for _ in 0..4 {
        let id = a.start_dedicated("alpha", "run_command").unwrap();
        futures.push(e.dispatch(Some(request()), a.clone(), id));
    }
    let reached = tokio::time::timeout(Duration::from_secs(5), async {
        while arrived.load(Ordering::Acquire) != 4 {
            tokio::task::yield_now().await;
        }
    })
    .await;
    release.store(true, Ordering::Release);
    assert!(reached.is_ok(), "four admitted workers");
    for f in futures {
        assert!(f.await.is_ok(), "row E response");
    }
    for _ in 0..6 {
        refused(call(&e, &a, false).await, Unavailable::MechanismUnavailable);
    }
    assert!(calls.load(Ordering::Acquire) == 4, "residue bound");
    let p = serde_json::to_value(list(&a)).unwrap();
    let rows = p["activities"].as_array().unwrap();
    assert!(
        rows.iter()
            .filter(|r| r["stop_reason"] == "cancelled" && r["outcome"] == "failed")
            .count()
            == 4,
        "four E rows"
    );
    assert!(
        rows.iter()
            .filter(|r| r["outcome"] == "refused" && r["reason"] == "preflight_failed")
            .count()
            == 6,
        "six refusals"
    );
    e.drain().await.unwrap();
}
#[tokio::test]
async fn t20_shared_domain_handles_never_replace_runner() {
    let dir = private_temp();
    let parent = dir.path().join("runs");
    std::fs::create_dir(&parent).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let residue = Arc::new(AtomicBool::new(false));
    let flag = residue.clone();
    let allocations = AtomicUsize::new(0);
    let port_calls = calls.clone();
    let d = AdmissionDomain::new(
        &Layout::for_test(Platform::Linux),
        &config(),
        parent.clone(),
        |owner| {
            allocations.fetch_add(1, Ordering::AcqRel);
            assert!(
                owner.cgroup_parent.as_ref() == Some(&parent),
                "owned subtree bound once"
            );
            Ok(Box::new(Port {
                calls: port_calls,
                residue,
                step: Box::new(move |_, _, _| {
                    flag.store(true, Ordering::Release);
                    Err(exec::RunError::NotStarted(exec::NotStarted::Cancelled))
                }),
            }))
        },
        Box::new(|| Ok(())),
        ready(),
    );
    let first = d.handle();
    let second = d.handle();
    assert!(Arc::ptr_eq(&first, &second), "shared admission state");
    let a = activity(&dir);
    assert!(call(&first, &a, false).await.is_ok(), "initial E");
    drop(first);
    refused(
        call(&second, &a, false).await,
        Unavailable::MechanismUnavailable,
    );
    drop(second);
    let recreated = d.handle();
    refused(
        call(&recreated, &a, false).await,
        Unavailable::MechanismUnavailable,
    );
    assert!(
        calls.load(Ordering::Acquire) == 1 && allocations.load(Ordering::Acquire) == 1,
        "sole runner retained"
    );
    recreated.drain().await.unwrap();
}

#[test]
fn e6_source_ownership_and_slot_latch_pins() {
    let bridge = include_str!("../../unix_bridge.rs");
    let body = &bridge[bridge.find("    fn body(").unwrap()
        ..bridge.find("impl Executor for UnixBridge").unwrap()];
    let post = &body[body.find("let outcome = runner.run").unwrap()..];
    assert!(
        post.find("self.readiness.latch(Cause::Containment)")
            .unwrap()
            < post.find("match outcome").unwrap(),
        "post call latch before return"
    );
    let domain = &bridge[bridge.find("struct AdmissionDomain").unwrap()..];
    assert!(
        domain.matches("unix_bridge_for_test").count() == 1,
        "one execution allocation"
    );
    let handle = &domain[domain.find("fn handle(").unwrap()..];
    assert!(
        !handle.contains("new(") && !handle.contains("Readiness") && !handle.contains("Runner"),
        "handle only clones retained domain"
    );
    assert!(!domain.contains("pub"), "private wiring owner");
}

#[tokio::test]
async fn t13c_existing_residue_gate() {
    let calls = Arc::new(AtomicUsize::new(0));
    let d = domain(
        ready(),
        Arc::new(AtomicBool::new(true)),
        calls.clone(),
        1,
        |_, _, _| Ok(result()),
    );
    let e = d.handle();
    let dir = private_temp();
    let a = activity(&dir);
    refused(call(&e, &a, false).await, Unavailable::MechanismUnavailable);
    assert!(calls.load(Ordering::Acquire) == 0, "existing residue gate");
    e.drain().await.unwrap();
}

// Invoked only inside the existing normal T10 parent's isolated child.
pub(super) async fn pa6_panic_residue_report_and_gate() {
    let residue = Arc::new(AtomicBool::new(false));
    let flag = residue.clone();
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let b = Arc::new(UnixBridge::new(
        &Layout::for_test(Platform::Linux),
        &config(),
        "/owned-synthetic/runs".into(),
        |_| {
            Ok(Box::new(Port {
                calls: count,
                residue: residue.clone(),
                step: Box::new(move |_, _, _| {
                    flag.store(true, Ordering::Release);
                    panic!("ARX-PA6-FIXED-PANIC");
                }),
            }))
        },
        Box::new(|| Ok(())),
        ready(),
    ));
    let e = Execution::unix_bridge_for_test(b.clone(), &config());
    let dir = private_temp();
    let a = activity(&dir);
    let out = call(&e, &a, false).await;
    assert!(
        out.as_ref()
            .is_ok_and(|v| v["outcome"] == "failed" && v["cleanup_confirmed"] == false),
        "unknown creation is an honest failed dispatch"
    );
    row(&a, "failed", Some("cleanup_unconfirmed"), Some("failed"));
    assert!(
        residue.load(Ordering::Acquire),
        "residue remains visible after the failed response"
    );
    // Exercise the bridge's next in-slot gate, independently of Execution's G latch.
    assert!(matches!(
        b.run(
            Some(&request()),
            &Cancellation::default(),
            &Cancellation::default()
        ),
        Report::Unavailable(Unavailable::MechanismUnavailable)
    ));
    assert_eq!(b.readiness.check(), Err(Unavailable::MechanismUnavailable));
    residue.store(false, Ordering::Release);
    assert_eq!(
        b.preflight(),
        Err(Unavailable::MechanismUnavailable),
        "terminal latch survives cleared residue"
    );
    for _ in 0..4 {
        refused(call(&e, &a, false).await, Unavailable::Quarantined);
    }
    assert_eq!(
        calls.load(Ordering::Acquire),
        1,
        "unwind cannot readmit the port"
    );
    let text = serde_json::to_string(&list(&a)).unwrap();
    assert!(
        !text.contains("ARX-PA6-FIXED-PANIC"),
        "panic payload withheld from activity"
    );
    e.drain().await.unwrap();
}
