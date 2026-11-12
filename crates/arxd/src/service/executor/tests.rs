use super::*;
use crate::service::application::tests::private_temp;
use std::sync::atomic::AtomicUsize;
use std::time::{Duration, Instant};
struct Fake {
    calls: Arc<AtomicUsize>,
    wait: bool,
    cleanup: bool,
}
impl Executor for Fake {
    fn preflight(&self) -> Result<(), Unavailable> {
        Ok(())
    }
    fn run(&self, _: Option<&Request>, cancel: &Cancellation, shutdown: &Cancellation) -> Report {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let until = Instant::now() + Duration::from_secs(3);
        while self.wait
            && !cancel.is_cancelled()
            && !shutdown.is_cancelled()
            && Instant::now() < until
        {
            std::thread::sleep(Duration::from_millis(2));
        }
        Report::FakeForTest {
            stop: if shutdown.is_cancelled() {
                StopReason::Shutdown
            } else if cancel.is_cancelled() {
                StopReason::Cancelled
            } else {
                StopReason::Exited
            },
            cleanup: self.cleanup,
        }
    }
}
pub(crate) fn fake(wait: bool, cleanup: bool) -> (Arc<Execution>, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let e = Execution {
        backend: Arc::new(Fake {
            calls: calls.clone(),
            wait,
            cleanup,
        }),
        reason: Arc::new(Mutex::new(Unavailable::MechanismUnavailable)),
        shutdown: Cancellation::default(),
        quarantined: AtomicBool::new(false),
        active: AtomicUsize::new(0),
        audit_failed: AtomicBool::new(false),
        limit: 1,
        workers: Mutex::new(tokio::task::JoinSet::new()),
        rights: None,
    };
    (Arc::new(e), calls)
}
fn request() -> Request {
    serde_json::from_value(json!({"tool":"cargo","args":["private-argv"],"cwd":"private-cwd"}))
        .unwrap()
}
async fn entered(calls: &AtomicUsize) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while calls.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}
#[test]
#[cfg(windows)]
fn malformed_unicode_drive_prefix_refuses_without_panicking() {
    for path in ["Cé:\\tool.exe", "C中:\\tool.exe", "C🦀:\\tool.exe"] {
        let request: Request = serde_json::from_value(json!({"executable":path})).unwrap();
        assert!(!request.validate());
    }
    let request: Request =
        serde_json::from_value(json!({"executable":"C:\\synthetic\\café.exe"})).unwrap();
    assert!(request.validate());
}
#[test]
fn strict_service_arguments_and_development_boundary() {
    assert!(request().validate());
    #[cfg(windows)]
    let path = r"C:\synthetic\cargo.exe";
    #[cfg(unix)]
    let path = "/synthetic/cargo";
    let good = json!({"executable":path});
    assert!(
        serde_json::from_value::<Request>(good.clone())
            .unwrap()
            .validate()
    );
    assert!(serde_json::from_value::<crate::development::Request>(good).is_err());
    for v in [
        json!({}),
        json!({"tool":"cargo","executable":path}),
        json!({"executable":"relative"}),
        json!({"tool":"cargo","cwd":"../escape"}),
        json!({"tool":"cargo","args":["\u{0}"]}),
        json!({"tool":"cargo","timeout_s":0}),
        json!({"tool":"cargo","output_bytes":8193}),
    ] {
        assert!(!serde_json::from_value::<Request>(v).unwrap().validate());
    }
    for key in [
        "mode",
        "principal",
        "identity",
        "config",
        "executor",
        "environment",
    ] {
        let mut v = json!({"tool":"cargo"});
        v[key] = json!("forbidden");
        assert!(serde_json::from_value::<Request>(v).is_err());
    }
}
#[tokio::test]
async fn unpolled_drop_finishes_once_and_shutdown_joins() {
    let dir = private_temp();
    let activity = Arc::new(Activity::open_service_fixture(dir.path()).unwrap());
    let (e, calls) = fake(true, true);
    let id = activity.start("alpha", "run_command").unwrap();
    let work = e.dispatch(Some(request()), activity.clone(), id);
    entered(&calls).await;
    drop(work);
    let observer = rusqlite::Connection::open(dir.path().join("activity.sqlite3")).unwrap();
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let state: String = observer
                .query_row("SELECT outcome FROM activity WHERE id=?", [id], |r| {
                    r.get(0)
                })
                .unwrap();
            if state != "started" {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        observer
            .query_row::<String, _, _>("SELECT stop_reason FROM activity WHERE id=?", [id], |r| r
                .get(0))
            .unwrap(),
        "cancelled"
    );
    drop(observer);
    e.drain().await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let db = rusqlite::Connection::open(dir.path().join("activity.sqlite3")).unwrap();
    let row: (String, String, i64) = db
        .query_row(
            "SELECT outcome,stop_reason,count(*) FROM activity WHERE id=?",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(row.0, "failed");
    assert!(["cancelled", "shutdown"].contains(&row.1.as_str()));
    assert_eq!(row.2, 1);
    assert!(activity.finish(id, "succeeded").is_err());
    assert_eq!(db.query_row::<i64,_,_>("SELECT count(*) FROM activity WHERE observed_kind IS NOT NULL OR observed_id IS NOT NULL OR rights IS NOT NULL",[],|r|r.get(0)).unwrap(),0);
}
#[tokio::test]
async fn busy_cleanup_precedence_quarantine_and_completion_withholding() {
    let dir = private_temp();
    let activity = Arc::new(Activity::open_service_fixture(dir.path()).unwrap());
    let (e, calls) = fake(true, false);
    let first = activity.start("alpha", "run_command").unwrap();
    let work = e.dispatch(Some(request()), activity.clone(), first);
    entered(&calls).await;
    let second = activity.start("beta", "run_command").unwrap();
    assert_eq!(
        e.dispatch(Some(request()), activity.clone(), second)
            .await
            .err()
            .unwrap()
            .execution_reason,
        Some(Unavailable::Busy)
    );
    drop(work);
    e.drain().await.unwrap();
    assert_eq!(e.preflight(), Err(Unavailable::Quarantined));
    let db = rusqlite::Connection::open(dir.path().join("activity.sqlite3")).unwrap();
    assert_eq!(
        db.query_row::<String, _, _>("SELECT reason FROM activity WHERE id=?", [first], |r| r
            .get(0))
            .unwrap(),
        "cleanup_unconfirmed"
    );
    let (e, _) = fake(false, true);
    let id = activity.start("alpha", "run_command").unwrap();
    db.execute_batch("PRAGMA journal_mode=TRUNCATE; CREATE TRIGGER fail_finish BEFORE UPDATE ON activity BEGIN SELECT RAISE(FAIL,'synthetic'); END;").unwrap();
    assert_eq!(
        e.dispatch(Some(request()), activity.clone(), id)
            .await
            .err()
            .unwrap()
            .reason,
        Reason::StorageFailed
    );
    assert_eq!(e.drain().await, Err(Reason::ExecutionFailed));
    db.execute_batch("DROP TRIGGER fail_finish").unwrap();
    activity.shutdown_pending().unwrap();
    assert_eq!(
        db.query_row::<String, _, _>("SELECT outcome FROM activity WHERE id=?", [id], |r| r
            .get(0))
            .unwrap(),
        "started"
    );
}
#[test]
fn synthetic_observation_mapping_and_terminal_table() {
    use crate::store::{ObservationPoint, ObservedKind, Rights};
    let w = Observation::Windows {
        sid: "S-1-5-21-100-200-300-1001".into(),
        integrity_rid: 8192,
        elevated: false,
        matches: true,
    }
    .metadata(Rights::Standard);
    assert_eq!(w.observed_kind, Some(ObservedKind::WindowsSid));
    assert_eq!(w.observation_point, Some(ObservationPoint::PostSwitch));
    assert_eq!(w.elevation, Some(false));
    assert_eq!(w.observed_gid, None);
    let u = Observation::Unix {
        uid: 1002,
        gid: 1003,
        matches: false,
    }
    .metadata(Rights::Standard);
    assert_eq!(u.observation_point, Some(ObservationPoint::PreExec));
    assert_eq!(u.reason, Some(Reason::IdentityMismatch));
    assert_eq!(u.integrity_rid, None);
    assert_eq!(u.observed_gid, Some(1003));
    for stop in [
        StopReason::Timeout,
        StopReason::Cancelled,
        StopReason::OutputLimit,
        StopReason::Shutdown,
        StopReason::Failed,
    ] {
        assert_eq!(
            terminal(&Metadata {
                stop_reason: Some(stop),
                cleanup_confirmed: Some(true),
                ..Default::default()
            }),
            Outcome::Failed
        );
    }
    for (code, outcome) in [(0, Outcome::Succeeded), (1, Outcome::Failed)] {
        assert_eq!(
            terminal(&Metadata {
                stop_reason: Some(StopReason::Exited),
                exit_code: Some(code),
                cleanup_confirmed: Some(true),
                ..Default::default()
            }),
            outcome
        );
    }
}
#[test]
fn output_fits_sdk_double_json_worst_cases() {
    for byte in [b'"', b'\\', b'\n', 0, 255] {
        let o = || {
            output_value(
                CapturedOutput {
                    head: vec![byte; 4096],
                    tail: vec![byte; 4096],
                    dropped_bytes: 0,
                },
                8192,
            )
        };
        let mut metadata = Observation::Windows {
            sid: format!("S-1-5-{}", vec!["4294967295"; 15].join("-")),
            integrity_rid: u32::MAX,
            elevated: true,
            matches: true,
        }
        .metadata(crate::store::Rights::Admin);
        metadata.tree_boundary = Some(crate::store::TreeBoundary::WindowsJob);
        metadata.stop_reason = Some(StopReason::Exited);
        metadata.exit_code = Some(i32::MIN);
        metadata.cleanup_confirmed = Some(true);
        metadata.argc = Some(128);
        let inner = json!({"activity_id":i64::MAX,"outcome":"failed","reason":null,"exit_code":i32::MIN,"rights":"admin","cleanup_confirmed":true,"tree_boundary":"windows_job","observed_identity":serde_json::to_value(&metadata).unwrap(),"stdout":o(),"stderr":o(),"metadata":serde_json::to_value(&metadata).unwrap()});
        let outer=json!({"jsonrpc":"2.0","id":i64::MAX,"result":{"content":[{"type":"text","text":inner.to_string()}]}}).to_string();
        assert!(outer.len() + 1 < 65536, "{}", outer.len());
    }
}

#[test]
fn all_postpreflight_reason_mappings_and_cleanup_precedence() {
    for stop in [
        StopReason::Exited,
        StopReason::Timeout,
        StopReason::Cancelled,
        StopReason::OutputLimit,
        StopReason::Shutdown,
        StopReason::Failed,
    ] {
        let mut m = Metadata {
            stop_reason: Some(stop),
            cleanup_confirmed: Some(true),
            ..Default::default()
        };
        normalize(&mut m);
        assert_eq!(
            m.reason,
            if stop == StopReason::Failed {
                Some(Reason::ExecutionFailed)
            } else {
                None
            }
        );
        m.identity_matches_pin = Some(false);
        normalize(&mut m);
        assert_eq!(m.reason, Some(Reason::IdentityMismatch));
        m.cleanup_confirmed = Some(false);
        normalize(&mut m);
        assert_eq!(m.reason, Some(Reason::CleanupUnconfirmed));
    }
}
#[tokio::test]
async fn explicit_shutdown_is_terminal_and_workers_are_joined() {
    let dir = private_temp();
    let a = Arc::new(Activity::open_service_fixture(dir.path()).unwrap());
    let (e, calls) = fake(true, true);
    let id = a.start("alpha", "run_command").unwrap();
    let work = e.dispatch(Some(request()), a.clone(), id);
    entered(&calls).await;
    e.shutdown();
    e.drain().await.unwrap();
    let value = work.await.ok().unwrap();
    assert_eq!(value["metadata"]["stop_reason"], "shutdown");
    assert!(e.workers.lock().unwrap().is_empty());
    assert_eq!(e.preflight(), Err(Unavailable::Stopping));
}

#[test]
fn discovery_fixed_names_printable_bounded_and_no_paths() {
    let mut tools: Vec<_> = crate::development::TOOLS
        .iter()
        .map(|n| ToolVersion {
            name: (*n).into(),
            found: true,
            version: Some("synthetic 1.0".into()),
        })
        .collect();
    assert!(valid_tools(&tools));
    for version in [
        "x".repeat(129),
        "bad\nversion".into(),
        "C:\\private\\tool.exe".into(),
        "/private/tool".into(),
    ] {
        tools[0].version = Some(version);
        assert!(!valid_tools(&tools));
    }
    tools[0].version = None;
    tools[0].name = "other".into();
    assert!(!valid_tools(&tools));
}

#[cfg(windows)]
#[test]
fn unsafe_service_executable_paths_refuse_lexically() {
    for path in [
        r"\\server\share\tool.exe",
        r"\\?\C:\tool.exe",
        r"C:tool.exe",
        r"C:\CON.exe",
        r"C:\aux.txt\tool.exe",
        r"C:\safe\..\tool.exe",
        r"C:\tool.exe:stream",
        "C:\\bad\nname.exe",
    ] {
        assert!(!absolute_executable(path));
    }
}

pub(crate) fn backend_fixture(backend: Arc<dyn Executor>) -> Arc<Execution> {
    let (e, _) = fake(false, true);
    let mut e = Arc::try_unwrap(e).ok().unwrap();
    e.backend = backend;
    Arc::new(e)
}
struct OnceReport(Mutex<Option<Report>>);
impl Executor for OnceReport {
    fn preflight(&self) -> Result<(), Unavailable> {
        Ok(())
    }
    fn run(&self, _: Option<&Request>, _: &Cancellation, _: &Cancellation) -> Report {
        self.0.lock().unwrap().take().unwrap()
    }
}
fn tools() -> Vec<ToolVersion> {
    crate::development::TOOLS
        .iter()
        .map(|n| ToolVersion {
            name: (*n).into(),
            found: false,
            version: None,
        })
        .collect()
}
async fn report_boundary_case(case: usize) {
    let dir = private_temp();
    let a = Arc::new(Activity::open_service_fixture(dir.path()).unwrap());
    let mut m = Metadata {
        stop_reason: Some(StopReason::Exited),
        exit_code: Some(0),
        cleanup_confirmed: Some(true),
        identity_mode: Some(crate::store::IdentityMode::DevelopmentCurrentUser),
        ..Default::default()
    };
    let (report, reason, quarantine) = match case {
        0 => {
            m.cleanup_confirmed = Some(false);
            (
                Report::Discovered {
                    metadata: m,
                    tools: vec![],
                },
                Some(Reason::CleanupUnconfirmed),
                true,
            )
        }
        1 => {
            m.stop_reason = Some(StopReason::Failed);
            (
                Report::Discovered {
                    metadata: m,
                    tools: tools(),
                },
                Some(Reason::ExecutionFailed),
                true,
            )
        }
        2 => (
            Report::Discovered {
                metadata: m,
                tools: vec![],
            },
            Some(Reason::ExecutionFailed),
            true,
        ),
        3 => {
            m.reason = Some(Reason::Busy);
            (
                Report::Discovered {
                    metadata: m,
                    tools: tools(),
                },
                None,
                false,
            )
        }
        4 => (
            Report::Failed { after_spawn: false },
            Some(Reason::ExecutionFailed),
            false,
        ),
        5 => (
            Report::Failed { after_spawn: true },
            Some(Reason::CleanupUnconfirmed),
            true,
        ),
        6 => {
            m.identity_matches_pin = Some(false);
            (
                Report::Discovered {
                    metadata: m,
                    tools: vec![],
                },
                Some(Reason::IdentityMismatch),
                true,
            )
        }
        _ => (
            Report::Completed {
                metadata: m,
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
            },
            None,
            false,
        ),
    };
    let e = backend_fixture(Arc::new(OnceReport(Mutex::new(Some(report)))));
    let id = a.start("alpha", "discover_tools").unwrap();
    let v = e.dispatch(None, a.clone(), id).await.ok().unwrap();
    assert_eq!(v["reason"], json!(reason), "case {case}");
    if case == 7 {
        assert_eq!(v["metadata"]["identity_mode"], "dedicated_account");
    }
    if case == 0 {
        assert_eq!(v["metadata"]["cleanup_confirmed"], false);
        assert_eq!(v["metadata"]["stop_reason"], "exited");
    }
    assert_eq!(
        e.quarantined.load(Ordering::Acquire),
        quarantine,
        "case {case}"
    );
    assert!(a.finish(id, "failed").is_err());
}

pub(crate) struct Gate {
    pub entered: tokio::sync::Notify,
    open: Mutex<bool>,
    cv: std::sync::Condvar,
}
impl Gate {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            entered: tokio::sync::Notify::new(),
            open: Mutex::new(false),
            cv: std::sync::Condvar::new(),
        })
    }
    pub fn wait(&self) {
        self.entered.notify_one();
        let mut open = self.open.lock().unwrap();
        while !*open {
            open = self.cv.wait(open).unwrap();
        }
    }
    pub fn release(&self) {
        *self.open.lock().unwrap() = true;
        self.cv.notify_all();
    }
}
pub(crate) struct GatedBackend {
    pub gate: Arc<Gate>,
    pub preflight_gate: bool,
    pub calls: Arc<AtomicUsize>,
}
impl Executor for GatedBackend {
    fn preflight(&self) -> Result<(), Unavailable> {
        if self.preflight_gate {
            self.gate.wait();
        }
        Ok(())
    }
    fn run(&self, _: Option<&Request>, _: &Cancellation, shutdown: &Cancellation) -> Report {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if !self.preflight_gate {
            self.gate.wait();
        }
        Report::FakeForTest {
            stop: if shutdown.is_cancelled() {
                StopReason::Shutdown
            } else {
                StopReason::Exited
            },
            cleanup: true,
        }
    }
}
#[tokio::test]
async fn cancellation_after_gated_preflight_never_enters_backend() {
    let dir = private_temp();
    let a = Arc::new(Activity::open_service_fixture(dir.path()).unwrap());
    let gate = Gate::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let e = backend_fixture(Arc::new(GatedBackend {
        gate: gate.clone(),
        preflight_gate: true,
        calls: calls.clone(),
    }));
    let id = a.start("alpha", "run_command").unwrap();
    let work = e.dispatch(Some(request()), a.clone(), id);
    gate.entered.notified().await;
    drop(work);
    gate.release();
    let mut workers = std::mem::take(&mut *e.workers.lock().unwrap());
    while let Some(result) = workers.join_next().await {
        result.unwrap();
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    e.drain().await.unwrap();
    let db = rusqlite::Connection::open(dir.path().join("activity.sqlite3")).unwrap();
    let row: (String, Option<String>, i64) = db
        .query_row(
            "SELECT outcome,stop_reason,count(*) FROM activity WHERE id=?",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(row, ("failed".into(), Some("cancelled".into()), 1));
    assert!(a.finish(id, "failed").is_err());
}

#[tokio::test]
async fn report_boundary_case_0() {
    report_boundary_case(0).await;
}
#[tokio::test]
async fn report_boundary_case_1() {
    report_boundary_case(1).await;
}
#[tokio::test]
async fn report_boundary_case_2() {
    report_boundary_case(2).await;
}
#[tokio::test]
async fn report_boundary_case_3() {
    report_boundary_case(3).await;
}
#[tokio::test]
async fn report_boundary_case_4() {
    report_boundary_case(4).await;
}
#[tokio::test]
async fn report_boundary_case_5() {
    report_boundary_case(5).await;
}
#[tokio::test]
async fn report_boundary_case_6() {
    report_boundary_case(6).await;
}
#[tokio::test]
async fn report_boundary_case_7() {
    report_boundary_case(7).await;
}

#[test]
#[cfg(windows)]
fn windows_console_device_names_refuse_lexically() {
    for name in ["CONIN$", "conout$", "CONIN$.other"] {
        assert!(
            !absolute_executable(&format!(r"C:\synthetic\{name}.exe")),
            "{name}"
        );
        assert!(
            !absolute_executable(&format!(r"C:\{name}\tool.exe")),
            "{name}"
        );
    }
}
#[test]
#[cfg(windows)]
fn windows_superscript_port_names_refuse_lexically() {
    for prefix in ["COM", "lpt"] {
        for digit in ['¹', '²', '³'] {
            let name = format!("{prefix}{digit}");
            assert!(
                !absolute_executable(&format!(r"C:\synthetic\{name}.exe")),
                "{name}"
            );
            assert!(
                !absolute_executable(&format!(r"C:\{name}\tool.exe")),
                "{name}"
            );
        }
    }
}
#[test]
#[cfg(windows)]
fn windows_spaced_reserved_bases_refuse_lexically() {
    for name in ["CON ", "AUX  ", "NUL ", "COM1 ", "LPT³ ", "CONOUT$ "] {
        assert!(
            !absolute_executable(&format!(r"C:\synthetic\{name}.exe")),
            "{name}"
        );
    }
}
#[test]
#[cfg(windows)]
fn ordinary_device_like_filenames_remain_lexically_valid() {
    for name in ["CONSOLE", "CONIN$tool", "COM10", "LPT0", "COM¹tool", "café"] {
        assert!(
            absolute_executable(&format!(r"C:\synthetic\{name}.exe")),
            "{name}"
        );
    }
}
#[tokio::test(flavor = "current_thread")]
async fn late_dispatch_after_drain_finishes_without_worker_or_poll() {
    for discovery in [false, true] {
        for storage_failure in [false, true] {
            let dir = private_temp();
            let a = Arc::new(Activity::open_service_fixture(dir.path()).unwrap());
            let (e, calls) = fake(false, true);
            e.drain().await.unwrap();
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
            let db = rusqlite::Connection::open(dir.path().join("activity.sqlite3")).unwrap();
            if storage_failure {
                db.execute_batch("PRAGMA journal_mode=TRUNCATE; CREATE TRIGGER fail_late_finish BEFORE UPDATE ON activity BEGIN SELECT RAISE(FAIL,'synthetic'); END;").unwrap();
            }
            let work = e.dispatch((!discovery).then(request), a.clone(), id);
            // Snapshot before polling or dropping the caller future. A registered
            // worker is a bug even if the blocking pool has already run it.
            let registered = e.workers.lock().unwrap().len();
            let row: (String, Option<String>, String, Option<String>, Option<i64>) = db
                .query_row(
                    "SELECT outcome,reason,identity_mode,program,argc FROM activity WHERE id=?",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
                )
                .unwrap();
            let schema: u32 = db
                .pragma_query_value(None, "user_version", |r| r.get(0))
                .unwrap();
            let result = work.await;
            let drained = e.drain().await;
            // Always join old-behavior workers before any regression assertion.
            assert_eq!(schema, 3);
            assert_eq!(registered, 0, "late dispatch registered an unjoined worker");
            assert_eq!(calls.load(Ordering::SeqCst), 0);
            let failure = result.err().unwrap();
            if storage_failure {
                assert_eq!(failure.reason, Reason::StorageFailed);
                assert_eq!(failure.execution_reason, None);
                assert_eq!(drained, Err(Reason::ExecutionFailed));
                assert_eq!(row.0, "started");
            } else {
                assert_eq!(failure.reason, Reason::Shutdown);
                assert_eq!(failure.execution_reason, Some(Unavailable::Stopping));
                assert!(drained.is_ok());
                assert_eq!(
                    row,
                    (
                        "refused".into(),
                        Some("shutdown".into()),
                        "dedicated_account".into(),
                        (!discovery).then(|| "cargo".into()),
                        (!discovery).then_some(1)
                    )
                );
                assert!(a.finish(id, "succeeded").is_err());
            }
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn late_dispatch_during_drain_preserves_gated_accepted_worker() {
    let dir = private_temp();
    let a = Arc::new(Activity::open_service_fixture(dir.path()).unwrap());
    let gate = Gate::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let e = backend_fixture(Arc::new(GatedBackend {
        gate: gate.clone(),
        preflight_gate: false,
        calls: calls.clone(),
    }));
    let first = a.start_dedicated("alpha", "run_command").unwrap();
    let work = e.dispatch(Some(request()), a.clone(), first);
    gate.entered.notified().await;
    let mut drain = Box::pin(e.drain());
    let pending =
        std::future::poll_fn(|cx| std::task::Poll::Ready(drain.as_mut().poll(cx).is_pending()))
            .await;
    let late = a.start_dedicated("alpha", "run_command").unwrap();
    let late_work = e.dispatch(Some(request()), a.clone(), late);
    let registered = e.workers.lock().unwrap().len();
    let db = rusqlite::Connection::open(dir.path().join("activity.sqlite3")).unwrap();
    let before = db.query_row("SELECT outcome FROM activity WHERE id=?", [first], |r| {
        r.get::<_, String>(0)
    });
    let late_before = db.query_row("SELECT outcome FROM activity WHERE id=?", [late], |r| {
        r.get::<_, String>(0)
    });
    drop(late_work); // Disconnect cannot undo a synchronous refusal.
    gate.release();
    let drained = drain.await;
    let value = work.await;
    e.drain().await.unwrap(); // Clean up replacement-set workers on old code.
    assert!(pending, "drain must await accepted backend cleanup");
    assert_eq!(before.unwrap(), "started");
    assert_eq!(registered, 0, "late dispatch registered during drain");
    assert_eq!(late_before.unwrap(), "refused");
    assert!(drained.is_ok());
    assert_eq!(value.ok().unwrap()["metadata"]["stop_reason"], "shutdown");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(e.workers.lock().unwrap().is_empty());
    assert!(a.finish(first, "succeeded").is_err());
    assert!(a.finish(late, "succeeded").is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn late_dispatch_after_shutdown_persists_even_when_caller_disconnects() {
    let dir = private_temp();
    let a = Arc::new(Activity::open_service_fixture(dir.path()).unwrap());
    let (e, calls) = fake(false, true);
    e.shutdown();
    let id = a.start_dedicated("alpha", "run_command").unwrap();
    let work = e.dispatch(Some(request()), a.clone(), id);
    let registered = e.workers.lock().unwrap().len();
    let db = rusqlite::Connection::open(dir.path().join("activity.sqlite3")).unwrap();
    let row = db.query_row(
        "SELECT outcome,reason FROM activity WHERE id=?",
        [id],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
    );
    drop(work);
    let drained = e.drain().await;
    assert!(drained.is_ok());
    assert_eq!(registered, 0);
    assert_eq!(row.unwrap(), ("refused".into(), "shutdown".into()));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(a.finish(id, "succeeded").is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn stopping_refusal_preserves_quarantine_precedence() {
    for discovery in [false, true] {
        let dir = private_temp();
        let a = Arc::new(Activity::open_service_fixture(dir.path()).unwrap());
        let (e, calls) = fake(false, true);
        e.quarantined.store(true, Ordering::Release);
        e.drain().await.unwrap();
        let capability = e.capability();
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
        let work = e.dispatch((!discovery).then(request), a.clone(), id);
        let registered = e.workers.lock().unwrap().len();
        let result = work.await;
        e.drain().await.unwrap();
        assert_eq!(registered, 0);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(capability["reason"], "quarantined");
        let failure = result.err().unwrap();
        assert_eq!(failure.reason, Reason::Quarantined);
        assert_eq!(failure.execution_reason, Some(Unavailable::Quarantined));
        let db = rusqlite::Connection::open(dir.path().join("activity.sqlite3")).unwrap();
        let row: (String, String, String) = db
            .query_row(
                "SELECT outcome,reason,identity_mode FROM activity WHERE id=?",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            row,
            (
                "refused".into(),
                "quarantined".into(),
                "dedicated_account".into()
            )
        );
    }
}
