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
