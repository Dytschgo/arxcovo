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
        pin: Ok(windows_pin(Rights::Standard)),
        configured_rights: Some(Rights::Standard),
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
        let sid = format!("S-1-5-{}", vec!["4294967295"; 15].join("-"));
        let pin = Pin::for_test(Platform::Windows, &sid, None, Rights::Admin);
        let mut evidence = good(Platform::Windows, Rights::Admin);
        evidence.observation = Some(Observation::Windows {
            sid,
            integrity_rid: u32::MAX,
            elevated: true,
            elevation_type: ElevationType::Full,
            administrators: GroupState::Enabled,
        });
        evidence.exit_code = Some(i32::MIN);
        let (_, mut metadata, _) = assess(&pin, evidence, None);
        metadata.argc = Some(128);
        let inner = json!({"activity_id":i64::MAX,"outcome":"failed","reason":null,"exit_code":i32::MIN,"rights":"admin","cleanup_confirmed":true,"tree_boundary":"windows_job","observed_identity":serde_json::to_value(&metadata).unwrap(),"stdout":o(),"stderr":o(),"metadata":serde_json::to_value(&metadata).unwrap()});
        let outer=json!({"jsonrpc":"2.0","id":i64::MAX,"result":{"content":[{"type":"text","text":inner.to_string()}]}}).to_string();
        assert!(outer.len() + 1 < 65536, "{}", outer.len());
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

pub(crate) fn default_fixture(backend: Arc<dyn Executor>) -> Arc<Execution> {
    let (e, _) = fake(false, true);
    let mut e = Arc::try_unwrap(e).ok().unwrap();
    e.backend = backend;
    Arc::new(e)
}
pub(crate) struct OnceReport(pub(crate) Mutex<Option<Report>>);
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
    let mut m = good(Platform::Windows, Rights::Standard);
    let (report, reason, quarantine) = match case {
        0 => {
            m.cleanup_confirmed = Some(false);
            (
                Report::Discovered {
                    evidence: m,
                    tools: vec![],
                },
                Some(Reason::CleanupUnconfirmed),
                true,
            )
        }
        1 => {
            m.stop = Some(StopReason::Failed);
            (
                Report::Discovered {
                    evidence: m,
                    tools: tools(),
                },
                Some(Reason::ExecutionFailed),
                true,
            )
        }
        2 => (
            Report::Discovered {
                evidence: m,
                tools: vec![],
            },
            Some(Reason::ExecutionFailed),
            true,
        ),
        3 => {
            m.stop = None;
            (
                Report::Discovered {
                    evidence: m,
                    tools: tools(),
                },
                Some(Reason::ExecutionFailed),
                true,
            )
        }
        4 => (
            Report::Failed {
                creation: Creation::NotInvoked,
            },
            Some(Reason::ExecutionFailed),
            false,
        ),
        5 => (
            Report::Failed {
                creation: Creation::Created,
            },
            Some(Reason::CleanupUnconfirmed),
            true,
        ),
        6 => {
            m.observation = Some(Observation::Windows {
                sid: SID_B.into(),
                integrity_rid: 8192,
                elevated: false,
                elevation_type: ElevationType::Default,
                administrators: GroupState::Absent,
            });
            (
                Report::Discovered {
                    evidence: m,
                    tools: vec![],
                },
                Some(Reason::IdentityMismatch),
                true,
            )
        }
        _ => (
            Report::Completed {
                evidence: m,
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
    let e = default_fixture(Arc::new(OnceReport(Mutex::new(Some(report)))));
    let id = if case == 7 {
        a.start("alpha", "discover_tools")
    } else {
        a.start_dedicated("alpha", "discover_tools")
    }
    .unwrap();
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
    let e = default_fixture(Arc::new(GatedBackend {
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
    let e = default_fixture(Arc::new(GatedBackend {
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

pub(crate) const SID_A: &str = "S-1-5-21-100-200-300-1001";
pub(crate) const SID_B: &str = "S-1-5-21-700-800-900-4242";
fn windows_pin(rights: Rights) -> Pin {
    Pin::for_test(Platform::Windows, SID_A, None, rights)
}
pub(crate) fn good(platform: Platform, rights: Rights) -> Evidence {
    let (observation, point, boundary) = match platform {
        Platform::Windows => (
            Observation::Windows {
                sid: SID_A.into(),
                integrity_rid: if rights == Rights::Admin { 12288 } else { 8192 },
                elevated: rights == Rights::Admin,
                elevation_type: if rights == Rights::Admin {
                    ElevationType::Full
                } else {
                    ElevationType::Default
                },
                administrators: if rights == Rights::Admin {
                    GroupState::Enabled
                } else {
                    GroupState::Absent
                },
            },
            ObservationPoint::PostSwitch,
            TreeBoundary::WindowsJob,
        ),
        Platform::Linux | Platform::Macos => (
            Observation::Unix {
                uid: 61001,
                gid: 61001,
            },
            ObservationPoint::PreExec,
            if platform == Platform::Linux {
                TreeBoundary::LinuxCgroup
            } else {
                TreeBoundary::MacUidSweep
            },
        ),
    };
    Evidence {
        observation: Some(observation),
        point: Some(point),
        stop: Some(StopReason::Exited),
        exit_code: Some(0),
        tree_boundary: Some(boundary),
        cleanup_confirmed: Some(true),
    }
}
pub(crate) fn backend_fixture(
    backend: Arc<dyn Executor>,
    pin: Result<Pin, Unavailable>,
) -> Arc<Execution> {
    let mut e = Arc::try_unwrap(default_fixture(backend)).ok().unwrap();
    e.configured_rights = pin.as_ref().ok().map(|p| p.rights);
    e.pin = pin;
    Arc::new(e)
}
pub(crate) const OUTPUT_MARKER: &str = "ARX0017-OUTPUT-MARKER";
pub(crate) fn completed(evidence: Evidence) -> Report {
    Report::Completed {
        evidence,
        stdout: CapturedOutput {
            head: SID_B.as_bytes().to_vec(),
            tail: OUTPUT_MARKER.as_bytes().to_vec(),
            dropped_bytes: 5,
        },
        stderr: CapturedOutput {
            head: OUTPUT_MARKER.as_bytes().to_vec(),
            tail: b"62002".to_vec(),
            dropped_bytes: 7,
        },
    }
}
pub(crate) fn assert_output_release(v: &Value, released: bool, name: &str) {
    if released {
        assert_eq!(
            v["stdout"],
            json!({"head":SID_B,"tail":OUTPUT_MARKER,"dropped_bytes":5}),
            "{name} stdout released verbatim"
        );
        assert_eq!(
            v["stderr"],
            json!({"head":OUTPUT_MARKER,"tail":"62002","dropped_bytes":7}),
            "{name} stderr released verbatim"
        );
        let mut metadata = v.clone();
        metadata.as_object_mut().unwrap().remove("stdout");
        metadata.as_object_mut().unwrap().remove("stderr");
        assert_private(&metadata);
    } else {
        for stream in ["stdout", "stderr"] {
            assert_eq!(
                v[stream],
                json!({"head":"","tail":"","dropped_bytes":null}),
                "{name} {stream} withheld constant"
            );
        }
        assert_private(v);
        assert!(
            !v.to_string().contains(OUTPUT_MARKER),
            "{name} marker withheld"
        );
    }
}
pub(crate) fn assert_private(v: &Value) {
    match v {
        Value::String(s) => {
            assert!(!s.contains(SID_B));
            assert!(!["62002", "63003"].contains(&s.as_str()));
        }
        Value::Number(n) => assert!(![Some(62002), Some(63003)].contains(&n.as_u64())),
        Value::Array(a) => {
            for x in a {
                assert_private(x);
            }
        }
        Value::Object(o) => {
            for x in o.values() {
                assert_private(x);
            }
        }
        Value::Null | Value::Bool(_) => {}
    }
}
#[tokio::test]
async fn adr0017_dispatch_durable_matrix() {
    // Each case uses a new store and backend, real dispatch, and every metadata
    // column is compared with the expected sanitized assessment.
    type Case = (
        String,
        Platform,
        Rights,
        Evidence,
        Option<bool>,
        Option<Reason>,
        bool,
    );
    let mut cases: Vec<Case> = vec![];
    let w = good(Platform::Windows, Rights::Standard);
    let mut add = |name: &str, platform, rights, e, discovery, reason, quarantine| {
        cases.push((
            name.into(),
            platform,
            rights,
            e,
            discovery,
            reason,
            quarantine,
        ));
    };
    for (name, platform, rights) in [
        ("windows_standard", Platform::Windows, Rights::Standard),
        ("windows_admin", Platform::Windows, Rights::Admin),
        ("linux_standard", Platform::Linux, Rights::Standard),
        ("linux_admin", Platform::Linux, Rights::Admin),
        ("mac_standard", Platform::Macos, Rights::Standard),
    ] {
        add(
            name,
            platform,
            rights,
            good(platform, rights),
            None,
            None,
            false,
        );
    }
    add(
        "discovery_N",
        Platform::Windows,
        Rights::Standard,
        w.clone(),
        Some(true),
        None,
        false,
    );
    let negatives: Vec<(&str, Evidence, Reason)> = vec![
        (
            "P0",
            Evidence {
                observation: None,
                point: None,
                ..w.clone()
            },
            Reason::ExecutionFailed,
        ),
        (
            "P1",
            Evidence {
                cleanup_confirmed: None,
                ..w.clone()
            },
            Reason::CleanupUnconfirmed,
        ),
        (
            "P2",
            good(Platform::Windows, Rights::Admin),
            Reason::IdentityMismatch,
        ),
        (
            "P3",
            Evidence {
                point: Some(ObservationPoint::Child),
                ..w.clone()
            },
            Reason::ExecutionFailed,
        ),
        (
            "P4",
            Evidence {
                tree_boundary: Some(TreeBoundary::ProcessGroup),
                ..w.clone()
            },
            Reason::CleanupUnconfirmed,
        ),
        (
            "missing_boundary",
            Evidence {
                tree_boundary: None,
                ..w.clone()
            },
            Reason::CleanupUnconfirmed,
        ),
        (
            "linux_boundary_windows",
            Evidence {
                tree_boundary: Some(TreeBoundary::LinuxCgroup),
                ..w.clone()
            },
            Reason::CleanupUnconfirmed,
        ),
        (
            "cleanup_false",
            Evidence {
                cleanup_confirmed: Some(false),
                ..w.clone()
            },
            Reason::CleanupUnconfirmed,
        ),
        (
            "stop_NULL",
            Evidence {
                stop: None,
                ..w.clone()
            },
            Reason::ExecutionFailed,
        ),
        (
            "exited_no_code",
            Evidence {
                exit_code: None,
                ..w.clone()
            },
            Reason::ExecutionFailed,
        ),
        (
            "timeout_with_code",
            Evidence {
                stop: Some(StopReason::Timeout),
                ..w.clone()
            },
            Reason::ExecutionFailed,
        ),
        (
            "failed",
            Evidence {
                stop: Some(StopReason::Failed),
                ..w.clone()
            },
            Reason::ExecutionFailed,
        ),
        (
            "pre_exec_windows",
            Evidence {
                point: Some(ObservationPoint::PreExec),
                ..w.clone()
            },
            Reason::ExecutionFailed,
        ),
        (
            "unix_windows",
            Evidence {
                observation: Some(Observation::Unix {
                    uid: 61001,
                    gid: 61001,
                }),
                ..w.clone()
            },
            Reason::ExecutionFailed,
        ),
        (
            "no_observation_failed",
            Evidence {
                observation: None,
                stop: Some(StopReason::Failed),
                ..w.clone()
            },
            Reason::ExecutionFailed,
        ),
    ];
    for (n, e, r) in negatives {
        add(
            n,
            Platform::Windows,
            Rights::Standard,
            e,
            None,
            Some(r),
            true,
        );
    }
    for (name, sid) in [
        ("SID_B", SID_B.to_string()),
        ("malformed", "S-1-x".into()),
        ("overlong", format!("S-1-5-{}1", "0".repeat(178))),
    ] {
        let mut e = w.clone();
        if let Some(Observation::Windows { sid: s, .. }) = &mut e.observation {
            *s = sid;
        }
        let r = if name == "SID_B" {
            Reason::IdentityMismatch
        } else {
            Reason::ExecutionFailed
        };
        add(
            name,
            Platform::Windows,
            Rights::Standard,
            e.clone(),
            None,
            Some(r),
            true,
        );
        if name == "SID_B" {
            add(
                "SID_B_cleanup_NULL",
                Platform::Windows,
                Rights::Standard,
                Evidence {
                    cleanup_confirmed: None,
                    ..e.clone()
                },
                None,
                Some(Reason::CleanupUnconfirmed),
                true,
            );
            e.stop = Some(StopReason::Failed);
            add(
                "mismatch_failed",
                Platform::Windows,
                Rights::Standard,
                e.clone(),
                None,
                Some(r),
                true,
            );
            e.cleanup_confirmed = None;
            add(
                "cleanup_mismatch_failed",
                Platform::Windows,
                Rights::Standard,
                e,
                None,
                Some(Reason::CleanupUnconfirmed),
                true,
            );
        }
    }
    for (name, ty, group, elevated) in [
        (
            "deny_only",
            ElevationType::Default,
            GroupState::DenyOnly,
            false,
        ),
        (
            "other_group",
            ElevationType::Default,
            GroupState::PresentOther,
            false,
        ),
        ("limited", ElevationType::Limited, GroupState::Absent, false),
        (
            "default_elevated",
            ElevationType::Default,
            GroupState::Absent,
            true,
        ),
        (
            "full_not_elevated",
            ElevationType::Full,
            GroupState::Absent,
            false,
        ),
    ] {
        let mut e = w.clone();
        if let Some(Observation::Windows {
            elevation_type,
            administrators,
            elevated: b,
            ..
        }) = &mut e.observation
        {
            *elevation_type = ty;
            *administrators = group;
            *b = elevated;
        }
        add(
            name,
            Platform::Windows,
            Rights::Standard,
            e,
            None,
            Some(Reason::IdentityMismatch),
            true,
        );
    }
    add(
        "standard_admin_pin",
        Platform::Windows,
        Rights::Admin,
        w.clone(),
        None,
        Some(Reason::IdentityMismatch),
        true,
    );
    add(
        "invalid_discovery",
        Platform::Windows,
        Rights::Standard,
        w.clone(),
        Some(false),
        Some(Reason::ExecutionFailed),
        true,
    );
    add(
        "failed_discovery",
        Platform::Windows,
        Rights::Standard,
        Evidence {
            stop: Some(StopReason::Failed),
            ..w.clone()
        },
        Some(true),
        Some(Reason::ExecutionFailed),
        true,
    );
    for (name, uid, gid) in [
        ("UID_B", 62002, 61001),
        ("GID_B", 61001, 63003),
        ("both_B", 62002, 63003),
    ] {
        add(
            name,
            Platform::Linux,
            Rights::Standard,
            Evidence {
                observation: Some(Observation::Unix { uid, gid }),
                ..good(Platform::Linux, Rights::Standard)
            },
            None,
            Some(Reason::IdentityMismatch),
            true,
        );
    }
    for (name, platform, e) in [
        (
            "windows_linux",
            Platform::Linux,
            Evidence {
                observation: w.observation.clone(),
                ..good(Platform::Linux, Rights::Standard)
            },
        ),
        (
            "child_unix",
            Platform::Linux,
            Evidence {
                point: Some(ObservationPoint::Child),
                ..good(Platform::Linux, Rights::Standard)
            },
        ),
        (
            "process_group_mac",
            Platform::Macos,
            Evidence {
                tree_boundary: Some(TreeBoundary::ProcessGroup),
                ..good(Platform::Macos, Rights::Standard)
            },
        ),
    ] {
        let r = if name == "process_group_mac" {
            Reason::CleanupUnconfirmed
        } else {
            Reason::ExecutionFailed
        };
        add(name, platform, Rights::Standard, e, None, Some(r), true);
    }
    for stop in [
        StopReason::Timeout,
        StopReason::Cancelled,
        StopReason::OutputLimit,
        StopReason::Shutdown,
    ] {
        for cleanup in [Some(true), None] {
            add(
                &format!("{stop:?}_{cleanup:?}"),
                Platform::Windows,
                Rights::Standard,
                Evidence {
                    stop: Some(stop),
                    exit_code: None,
                    cleanup_confirmed: cleanup,
                    ..w.clone()
                },
                None,
                if cleanup.is_none() {
                    Some(Reason::CleanupUnconfirmed)
                } else {
                    None
                },
                cleanup.is_none(),
            );
        }
    }
    add(
        "nonzero",
        Platform::Windows,
        Rights::Standard,
        Evidence {
            exit_code: Some(7),
            ..w
        },
        None,
        None,
        false,
    );
    assert_eq!(cases.len(), 50, "original assessment matrix");
    let aliases = [
        ("out_sid_b", "SID_B"),
        ("out_same_sid_full", "P2"),
        ("out_same_sid_limited_denyonly", "limited"),
        ("out_admin_pin_standard_token", "standard_admin_pin"),
        ("out_unix_uid_b", "UID_B"),
        ("out_unix_gid_b", "GID_B"),
        ("out_no_observation", "P0"),
        ("out_wrong_kind", "unix_windows"),
        ("out_wrong_point", "pre_exec_windows"),
        ("out_malformed", "malformed"),
        ("out_sid_b_cleanup_null", "SID_B_cleanup_NULL"),
        ("out_none_wrong_boundary", "P0"),
        ("out_ok", "windows_standard"),
        ("out_exit_nonzero", "nonzero"),
        ("out_timeout", "Timeout_Some(true)"),
        ("out_cancelled", "Cancelled_Some(true)"),
        ("out_output_limit", "OutputLimit_Some(true)"),
        ("out_shutdown", "Shutdown_Some(true)"),
        ("out_sid_a_cleanup_null", "P1"),
        ("out_sid_a_wrong_boundary", "P4"),
        ("out_stop_failed", "failed"),
        ("out_linux_ok", "linux_standard"),
        ("out_macos_exit_nonzero", "mac_standard"),
    ];
    let mut extra = vec![];
    for (label, original) in aliases {
        let mut case = cases.iter().find(|c| c.0 == original).unwrap().clone();
        case.0 = label.into();
        if label == "out_same_sid_limited_denyonly"
            && let Some(Observation::Windows { administrators, .. }) = &mut case.3.observation
        {
            *administrators = GroupState::DenyOnly;
        }
        if label == "out_none_wrong_boundary" {
            case.3.tree_boundary = Some(TreeBoundary::ProcessGroup);
            case.5 = Some(Reason::CleanupUnconfirmed);
        }
        if ["out_exit_nonzero", "out_macos_exit_nonzero"].contains(&label) {
            case.3.exit_code = Some(2);
        }
        if label == "out_stop_failed" {
            case.3.exit_code = None;
        }
        extra.push(case);
    }
    for (name, rights, rid, elevated) in [
        (
            "review_standard_low_integrity",
            Rights::Standard,
            4096,
            false,
        ),
        (
            "review_standard_high_integrity",
            Rights::Standard,
            12288,
            false,
        ),
        ("review_admin_medium_integrity", Rights::Admin, 8192, true),
        ("review_admin_not_elevated", Rights::Admin, 12288, false),
    ] {
        let mut evidence = good(Platform::Windows, rights);
        if let Some(Observation::Windows {
            integrity_rid,
            elevated: value,
            ..
        }) = &mut evidence.observation
        {
            *integrity_rid = rid;
            *value = elevated;
        }
        extra.push((
            name.into(),
            Platform::Windows,
            rights,
            evidence,
            None,
            Some(Reason::IdentityMismatch),
            true,
        ));
    }
    for (name, platform) in [
        ("review_linux_post_switch", Platform::Linux),
        ("review_macos_post_switch", Platform::Macos),
    ] {
        let mut evidence = good(platform, Rights::Standard);
        evidence.point = Some(ObservationPoint::PostSwitch);
        extra.push((
            name.into(),
            platform,
            Rights::Standard,
            evidence,
            None,
            Some(Reason::ExecutionFailed),
            true,
        ));
    }
    for (name, platform, boundary) in [
        (
            "review_linux_mac_boundary",
            Platform::Linux,
            TreeBoundary::MacUidSweep,
        ),
        (
            "review_macos_linux_boundary",
            Platform::Macos,
            TreeBoundary::LinuxCgroup,
        ),
    ] {
        let mut evidence = good(platform, Rights::Standard);
        evidence.tree_boundary = Some(boundary);
        extra.push((
            name.into(),
            platform,
            Rights::Standard,
            evidence,
            None,
            Some(Reason::CleanupUnconfirmed),
            true,
        ));
    }
    extra.extend(cases);
    assert_eq!(
        extra.len(),
        81,
        "assessment and output matrix plus eight review cases"
    );
    let cases = extra;
    let mut cleanup_pair = vec![];
    let mut output_counts = (0, 0);
    for (name, platform, rights, evidence, discovery, reason, quarantine) in cases {
        let label = name.clone();
        let name = aliases
            .iter()
            .find(|(label, _)| *label == name)
            .map_or(name.as_str(), |(_, original)| *original)
            .to_string();
        let dir = private_temp();
        let a = Arc::new(Activity::open_service_fixture(dir.path()).unwrap());
        let pin = if platform == Platform::Windows {
            windows_pin(rights)
        } else {
            Pin::for_test(platform, "61001", Some(61001), rights)
        };
        // Expected row is built from the named fixture and contract, never assess.

        let admissible = ![
            "P0",
            "P3",
            "pre_exec_windows",
            "unix_windows",
            "no_observation_failed",
            "malformed",
            "overlong",
            "windows_linux",
            "child_unix",
            "review_linux_post_switch",
            "review_macos_post_switch",
        ]
        .contains(&name.as_str());

        let expected_id = if !admissible
            || [
                "SID_B",
                "mismatch_failed",
                "cleanup_mismatch_failed",
                "SID_B_cleanup_NULL",
                "UID_B",
                "both_B",
            ]
            .contains(&name.as_str())
        {
            None
        } else {
            Some(if platform == Platform::Windows {
                SID_A
            } else {
                "61001"
            })
        };

        let expected_gid = if platform != Platform::Windows
            && admissible
            && !["GID_B", "both_B"].contains(&name.as_str())
        {
            Some(61001)
        } else {
            None
        };
        let (integrity_rid, elevation) = if admissible {
            match &evidence.observation {
                Some(Observation::Windows {
                    integrity_rid,
                    elevated,
                    ..
                }) => (Some(*integrity_rid), Some(*elevated)),
                Some(Observation::Unix { .. }) | None => (None, None),
            }
        } else {
            (None, None)
        };
        let expected = Metadata {
            reason,
            rights: Some(rights),
            identity_mode: Some(IdentityMode::DedicatedAccount),
            stop_reason: evidence.stop,
            exit_code: evidence.exit_code,
            tree_boundary: evidence.tree_boundary,
            cleanup_confirmed: Some(reason != Some(Reason::CleanupUnconfirmed)),
            observed_kind: if admissible {
                Some(if platform == Platform::Windows {
                    ObservedKind::WindowsSid
                } else {
                    ObservedKind::UnixUid
                })
            } else {
                None
            },
            observed_id: expected_id.map(str::to_string),
            observed_gid: expected_gid,
            integrity_rid,
            elevation,
            observation_point: if admissible { evidence.point } else { None },
            identity_matches_pin: if admissible {
                Some(
                    reason != Some(Reason::IdentityMismatch)
                        && !["cleanup_mismatch_failed", "SID_B_cleanup_NULL"]
                            .contains(&name.as_str()),
                )
            } else {
                None
            },
            ..Default::default()
        };
        let succeeded = reason.is_none()
            && evidence.stop == Some(StopReason::Exited)
            && evidence.exit_code == Some(0);
        let report = if let Some(valid) = discovery {
            Report::Discovered {
                evidence,
                tools: if valid { tools() } else { vec![] },
            }
        } else {
            completed(evidence)
        };
        let e = backend_fixture(Arc::new(OnceReport(Mutex::new(Some(report)))), Ok(pin));
        let id = a
            .start_dedicated(
                "alpha",
                if discovery.is_some() {
                    "discover_tools"
                } else {
                    "run_command"
                },
            )
            .unwrap();
        let v = e
            .dispatch(discovery.is_none().then(request), a.clone(), id)
            .await
            .unwrap_or_else(|failure| {
                panic!(
                    "{name}: dispatch must finish durably, got {:?}",
                    failure.reason
                )
            });
        assert_eq!(
            v["outcome"],
            if succeeded { "succeeded" } else { "failed" },
            "{name}"
        );
        assert_eq!(v["reason"], json!(reason), "{name}");
        assert_eq!(
            v.get("tools").is_some(),
            discovery.is_some() && succeeded,
            "{name}"
        );
        assert_eq!(v["observed_identity"]["id"], json!(expected_id), "{name}");
        assert_eq!(v["metadata"]["identity_mode"], "dedicated_account");
        assert_eq!(v["rights"], json!(rights), "{name} pinned rights");
        assert_eq!(
            v["observed_identity"],
            if admissible {
                json!({"kind":expected.observed_kind,"id":expected.observed_id,"gid":expected.observed_gid,
                "integrity_rid":expected.integrity_rid,"elevation":expected.elevation,"observation_point":expected.observation_point})
            } else {
                Value::Null
            },
            "{name} observed identity"
        );
        if discovery.is_none() {
            let released = expected.identity_matches_pin == Some(true);
            assert_output_release(&v, released, &label);
            if label.starts_with("out_") {
                if released {
                    output_counts.1 += 1;
                } else {
                    output_counts.0 += 1;
                }
            }
            if ["out_sid_b_cleanup_null", "out_sid_a_cleanup_null"].contains(&label.as_str()) {
                cleanup_pair.push((
                    v["outcome"].clone(),
                    v["reason"].clone(),
                    quarantine,
                    released,
                ));
            }
        } else {
            assert_private(&v);
        }
        let db = rusqlite::Connection::open(dir.path().join("activity.sqlite3")).unwrap();
        let stored:Value=db.query_row("SELECT outcome,reason,stop_reason,exit_code,rights,observed_kind,observed_id,observed_gid,integrity_rid,elevation,observation_point,identity_matches_pin,tree_boundary,cleanup_confirmed,identity_mode FROM activity WHERE id=?",[id],|r| {
            let mut obj=serde_json::Map::new();
            for (i,key) in ["outcome","reason","stop_reason","exit_code","rights","observed_kind","observed_id","observed_gid","integrity_rid","elevation","observation_point","identity_matches_pin","tree_boundary","cleanup_confirmed","identity_mode"].iter().enumerate() {
                let value=match r.get_ref(i)? {
                    rusqlite::types::ValueRef::Null=>Value::Null,
                    rusqlite::types::ValueRef::Integer(n) if [9,11,13].contains(&i)=>json!(n!=0),
                    rusqlite::types::ValueRef::Integer(n)=>json!(n),
                    rusqlite::types::ValueRef::Text(t)=>json!(std::str::from_utf8(t).unwrap()),
                    _=>panic!("unexpected column type"),
                };obj.insert((*key).into(),value);
            } Ok(Value::Object(obj))
        }).unwrap();
        assert_eq!(stored["outcome"], v["outcome"], "{name}");
        let serialized = serde_json::to_value(&expected).unwrap();
        for key in [
            "reason",
            "stop_reason",
            "exit_code",
            "rights",
            "observed_kind",
            "observed_id",
            "observed_gid",
            "integrity_rid",
            "elevation",
            "observation_point",
            "identity_matches_pin",
            "tree_boundary",
            "cleanup_confirmed",
            "identity_mode",
        ] {
            assert_eq!(stored[key], serialized[key], "{name} {key}");
            assert_eq!(
                v["metadata"][key], serialized[key],
                "{name} immediate {key}"
            );
        }
        assert_private(&stored);
        let read = a.start("alpha", "list_activity").unwrap();
        let page = a
            .list(
                "alpha",
                read,
                &crate::store::ListActivity {
                    limit: 50,
                    before_id: None,
                    include_reads: false,
                },
            )
            .unwrap();
        let page = serde_json::to_value(page).unwrap();
        assert_private(&page);
        assert!(!page.to_string().contains(OUTPUT_MARKER));
        assert_eq!(
            page["activities"][0]["observed_id"],
            json!(expected_id),
            "{name}"
        );
        assert_eq!(e.quarantined.load(Ordering::Acquire), quarantine, "{name}");
        if quarantine {
            let next = a.start_dedicated("alpha", "run_command").unwrap();
            assert_eq!(
                e.dispatch(Some(request()), a.clone(), next)
                    .await
                    .err()
                    .unwrap()
                    .execution_reason,
                Some(Unavailable::Quarantined)
            );
        }
        e.drain().await.unwrap();
        drop(db);
        drop(a);
        drop(e);
        for path in [
            dir.path().join("activity.sqlite3"),
            dir.path().join("activity.sqlite3-journal"),
        ] {
            if path.exists() {
                let bytes = std::fs::read(path).unwrap();
                for secret in [
                    SID_B,
                    OUTPUT_MARKER,
                    "62002",
                    "63003",
                    "S-1-x",
                    &format!("S-1-5-{}1", "0".repeat(178)),
                ] {
                    assert!(
                        !bytes.windows(secret.len()).any(|b| b == secret.as_bytes()),
                        "{name}"
                    );
                }
            }
        }
    }
    assert_eq!(output_counts, (12, 11), "exact output follow-up cases");
    assert_eq!(
        cleanup_pair,
        vec![
            (json!("failed"), json!("cleanup_unconfirmed"), true, false),
            (json!("failed"), json!("cleanup_unconfirmed"), true, true),
        ],
        "identity versus cleanup-only release"
    );
}
#[tokio::test]
async fn adr0017_withheld_capture_shape_constant() {
    let mut shapes = vec![];
    for large in [false, true] {
        let dir = private_temp();
        let a = Arc::new(Activity::open_service_fixture(dir.path()).unwrap());
        let mut evidence = good(Platform::Windows, Rights::Standard);
        evidence.observation = None;
        evidence.point = None;
        let capture = || CapturedOutput {
            head: if large { vec![0xff; 8192] } else { vec![] },
            tail: if large { vec![0xff; 8192] } else { vec![] },
            dropped_bytes: if large { u64::MAX } else { 0 },
        };
        let e = backend_fixture(
            Arc::new(OnceReport(Mutex::new(Some(Report::Completed {
                evidence,
                stdout: capture(),
                stderr: capture(),
            })))),
            Ok(windows_pin(Rights::Standard)),
        );
        let id = a.start_dedicated("alpha", "run_command").unwrap();
        let v = e
            .dispatch(Some(request()), a, id)
            .await
            .unwrap_or_else(|_| panic!("constant-shape dispatch failed"));
        assert_output_release(&v, false, "empty versus 8192FF/u64MAX");
        shapes.push(serde_json::to_vec(&json!([v["stdout"], v["stderr"]])).unwrap());
        e.drain().await.unwrap();
    }
    assert_eq!(shapes[0], shapes[1], "capture-independent withheld shape");
}
#[test]
fn adr0017_debug_and_pins() {
    for observation in [
        Observation::Windows {
            sid: SID_B.into(),
            integrity_rid: 8192,
            elevated: false,
            elevation_type: ElevationType::Default,
            administrators: GroupState::Absent,
        },
        Observation::Unix {
            uid: 62002,
            gid: 63003,
        },
    ] {
        let debug = format!("{observation:?}");
        for secret in [SID_B, "62002", "63003"] {
            assert!(!debug.contains(secret));
        }
        assert!(debug.contains("<withheld>"));
    }
    assert!(matches!(
        Pin::new(Platform::Macos, "61001".into(), Some(61001), Rights::Admin),
        Err(Unavailable::AdminUnsupportedOnPlatform)
    ));
    for (platform, id, gid) in [
        (Platform::Windows, SID_A, Some(1)),
        (Platform::Linux, "abc", Some(1)),
        (Platform::Linux, "61001", None),
        (Platform::Windows, "S-1-x", None),
    ] {
        assert!(matches!(
            Pin::new(platform, id.into(), gid, Rights::Standard),
            Err(Unavailable::PreflightFailed)
        ));
    }
}
struct PhaseScript {
    phase: Creation,
    cancel: bool,
    events: Arc<Mutex<Vec<&'static str>>>,
}
impl Executor for PhaseScript {
    fn preflight(&self) -> Result<(), Unavailable> {
        Ok(())
    }
    fn run(&self, _: Option<&Request>, _: &Cancellation, _: &Cancellation) -> Report {
        let mut events = self.events.lock().unwrap();
        if self.cancel {
            events.push("final cancel recheck");
            return Report::CancelledBeforeSpawn;
        }
        match self.phase {
            Creation::NotInvoked => {}
            Creation::ProvenNone => {
                events.push("creation primitive called");
                events.push("proven no child");
            }
            Creation::Created => {
                events.push("creation primitive called");
                events.push("child created");
                events.push("pre-exec failed");
            }
            Creation::Unknown => {
                events.push("creation primitive called");
                events.push("outcome indistinguishable");
            }
        }
        Report::Failed {
            creation: self.phase,
        }
    }
}
#[tokio::test]
async fn adr0017_creation_phases_and_pin_refusal() {
    for (phase, cancel, spawned) in [
        (Creation::NotInvoked, false, false),
        (Creation::ProvenNone, false, false),
        (Creation::Created, false, true),
        (Creation::Unknown, false, true),
        (Creation::NotInvoked, true, false),
    ] {
        let dir = private_temp();
        let a = Arc::new(Activity::open_service_fixture(dir.path()).unwrap());
        let events = Arc::new(Mutex::new(vec![]));
        let e = default_fixture(Arc::new(PhaseScript {
            phase,
            cancel,
            events: events.clone(),
        }));
        let id = a.start_dedicated("alpha", "run_command").unwrap();
        let v = e
            .dispatch(Some(request()), a.clone(), id)
            .await
            .ok()
            .unwrap();
        assert_eq!(v["outcome"], "failed");
        assert_eq!(
            v["reason"],
            json!(if cancel {
                None
            } else {
                Some(if spawned {
                    Reason::CleanupUnconfirmed
                } else {
                    Reason::ExecutionFailed
                })
            })
        );
        assert_eq!(v["observed_identity"], Value::Null);
        assert_eq!(
            v["rights"],
            if spawned {
                json!("standard")
            } else {
                Value::Null
            }
        );
        assert_eq!(
            v["cleanup_confirmed"],
            if spawned { json!(false) } else { Value::Null }
        );
        let db = rusqlite::Connection::open(dir.path().join("activity.sqlite3")).unwrap();
        type PhaseRow = (
            String,
            Option<String>,
            String,
            Option<String>,
            Option<i64>,
            Option<String>,
            Option<String>,
            Option<i64>,
            Option<i64>,
            Option<i64>,
            Option<String>,
            Option<String>,
            Option<i64>,
            Option<i64>,
        );
        let row: PhaseRow=db.query_row("SELECT outcome,reason,stop_reason,rights,cleanup_confirmed,observed_kind,observed_id,observed_gid,integrity_rid,elevation,observation_point,tree_boundary,identity_matches_pin,exit_code FROM activity WHERE id=?",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?,r.get(9)?,r.get(10)?,r.get(11)?,r.get(12)?,r.get(13)?))).unwrap();
        assert_eq!(row.0, "failed");
        assert_eq!(json!(row.1), v["reason"]);
        assert_eq!(row.2, if cancel { "cancelled" } else { "failed" });
        assert_eq!(row.3, spawned.then(|| "standard".into()));
        assert_eq!(row.4, spawned.then_some(0));
        assert_eq!(
            (
                row.5, row.6, row.7, row.8, row.9, row.10, row.11, row.12, row.13
            ),
            (None, None, None, None, None, None, None, None, None)
        );
        assert_eq!(e.quarantined.load(Ordering::Acquire), spawned);
        let before = events.lock().unwrap().clone();
        assert_eq!(
            before.contains(&"creation primitive called"),
            phase != Creation::NotInvoked
        );
        assert_eq!(
            before.contains(&"child created"),
            phase == Creation::Created
        );
        if spawned {
            let next = a.start_dedicated("alpha", "run_command").unwrap();
            assert_eq!(
                e.dispatch(Some(request()), a.clone(), next)
                    .await
                    .err()
                    .unwrap()
                    .execution_reason,
                Some(Unavailable::Quarantined)
            );
            assert_eq!(*events.lock().unwrap(), before);
        }
        e.drain().await.unwrap();
    }
    let dir = private_temp();
    let a = Arc::new(Activity::open_service_fixture(dir.path()).unwrap());
    let events = Arc::new(Mutex::new(vec![]));
    let e = backend_fixture(
        Arc::new(PhaseScript {
            phase: Creation::NotInvoked,
            cancel: false,
            events: events.clone(),
        }),
        Pin::new(Platform::Macos, "61001".into(), Some(61001), Rights::Admin),
    );
    let id = a.start_dedicated("alpha", "run_command").unwrap();
    assert_eq!(
        e.dispatch(Some(request()), a.clone(), id)
            .await
            .err()
            .unwrap()
            .reason,
        Reason::PreflightFailed
    );
    assert!(events.lock().unwrap().is_empty());
    e.drain().await.unwrap();
}

#[tokio::test]
async fn adr0017_backend_refusal_rows() {
    for refusal in [Unavailable::MechanismUnavailable, Unavailable::Busy] {
        let dir = private_temp();
        let a = Arc::new(Activity::open_service_fixture(dir.path()).unwrap());
        let e = default_fixture(Arc::new(OnceReport(Mutex::new(Some(Report::Unavailable(
            refusal,
        ))))));
        let id = a.start_dedicated("alpha", "run_command").unwrap();
        let failure = e
            .dispatch(Some(request()), a.clone(), id)
            .await
            .err()
            .unwrap();
        assert_eq!(failure.execution_reason, Some(refusal));
        assert_eq!(failure.reason, refusal.stored_reason());
        let db = rusqlite::Connection::open(dir.path().join("activity.sqlite3")).unwrap();
        type RefusalRow = (
            String,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<i64>,
            String,
        );
        let row: RefusalRow =
            db.query_row("SELECT outcome,reason,rights,stop_reason,observed_kind,cleanup_confirmed,identity_mode FROM activity WHERE id=?", [id],
                |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?))).unwrap();
        assert_eq!(
            row,
            (
                "refused".into(),
                refusal.stored_reason().as_str().into(),
                None,
                None,
                None,
                None,
                "dedicated_account".into()
            )
        );
        assert!(!e.quarantined.load(Ordering::Acquire));
        e.drain().await.unwrap();
    }
}

#[test]
fn adr0017_capability_retains_configured_rights_with_missing_unix_gid() {
    use crate::service::{
        config::{
            ServiceConfig,
            tests::{FakeResolver, fixture},
        },
        layout::Layout,
    };
    for rights in ["standard", "admin"] {
        for gid in [Value::Null, json!(1002)] {
            let layout = Layout::for_test(Platform::Linux);
            let mut wire = fixture();
            wire["rights"] = json!(rights);
            wire["build_account"]["gid"] = gid.clone();
            let config = ServiceConfig::parse_inventory(
                &serde_json::to_vec(&wire).unwrap(),
                &layout,
                &FakeResolver::default(),
            )
            .unwrap();
            let e = Execution::configured(layout, config);
            assert_eq!(e.pin.is_err(), gid.is_null());
            // capability is cached; do not call preflight or its NativeResolver.
            assert_eq!(
                e.capability(),
                json!({"available":false,
                "reason":"mechanism_unavailable", "rights":rights,
                "service_hardening":null, "service_state_protected":rights != "admin"})
            );
        }
    }
}

struct ReviewRefusingBackend {
    preflights: Arc<AtomicUsize>,
    runs: Arc<AtomicUsize>,
}
impl Executor for ReviewRefusingBackend {
    fn preflight(&self) -> Result<(), Unavailable> {
        self.preflights.fetch_add(1, Ordering::SeqCst);
        Err(Unavailable::AccountMissing)
    }
    fn run(&self, _: Option<&Request>, _: &Cancellation, _: &Cancellation) -> Report {
        self.runs.fetch_add(1, Ordering::SeqCst);
        Report::Failed {
            creation: Creation::NotInvoked,
        }
    }
}
#[tokio::test]
async fn adr0017_backend_preflight_refusal_precedes_invalid_pin() {
    let dir = private_temp();
    let a = Arc::new(Activity::open_service_fixture(dir.path()).unwrap());
    let preflights = Arc::new(AtomicUsize::new(0));
    let runs = Arc::new(AtomicUsize::new(0));
    let e = backend_fixture(
        Arc::new(ReviewRefusingBackend {
            preflights: preflights.clone(),
            runs: runs.clone(),
        }),
        Err(Unavailable::PreflightFailed),
    );
    let id = a.start_dedicated("alpha", "run_command").unwrap();
    let refusal = e
        .dispatch(Some(request()), a.clone(), id)
        .await
        .err()
        .unwrap();
    assert_eq!(refusal.execution_reason, Some(Unavailable::AccountMissing));
    assert_eq!(refusal.reason, Reason::PreflightFailed);
    assert_eq!(preflights.load(Ordering::SeqCst), 1);
    assert_eq!(runs.load(Ordering::SeqCst), 0);
    let db = rusqlite::Connection::open(dir.path().join("activity.sqlite3")).unwrap();
    let row: (String, String) = db
        .query_row(
            "SELECT outcome,reason FROM activity WHERE id=?",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(row, ("refused".into(), "preflight_failed".into()));
    assert!(!e.quarantined.load(Ordering::Acquire));
    e.drain().await.unwrap();
}
