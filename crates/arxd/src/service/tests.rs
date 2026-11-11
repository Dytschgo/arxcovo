use super::*;
use std::sync::{Arc, Mutex};

// Lifecycle contract double only. It never exposes an endpoint or an identity
// and cannot stand in for service/inventory/dedicated-account acceptance.
#[derive(Default)]
struct FakeApp {
    events: Arc<Mutex<Vec<&'static str>>>,
    request: Shutdown,
    cancellation: Option<Shutdown>,
    fail: Option<(&'static str, ExitCode)>,
    stop_before_ready: bool,
    stuck: bool,
    delay_start: bool,
    wait_signal: bool,
    delay_drain: bool,
}
impl FakeApp {
    fn event(&self, name: &'static str) -> Result<(), ExitCode> {
        self.events.lock().unwrap().push(name);
        if let Some((stage, code)) = self.fail
            && stage == name
        {
            return Err(code);
        }
        Ok(())
    }
}
impl Application for FakeApp {
    async fn load_config(&mut self) -> Result<(), ExitCode> {
        self.event("config")
    }
    async fn verify_layout(&mut self) -> Result<(), ExitCode> {
        self.event("layout")
    }
    async fn open_store(&mut self) -> Result<(), ExitCode> {
        self.event("store")
    }
    async fn service_start(&mut self) -> Result<(), ExitCode> {
        self.event("start")
    }
    async fn bind(&mut self, cancellation: Shutdown) -> Result<(), ExitCode> {
        self.cancellation = Some(cancellation);
        self.event("bind")
    }
    async fn preflight(&mut self) {
        self.event("preflight").unwrap();
        if self.stop_before_ready {
            self.request.cancel();
        }
        if self.delay_start {
            tokio::time::sleep(Duration::from_millis(2500)).await;
        }
    }
    fn start_discovery(&mut self) {
        self.event("discovery").unwrap();
    }
    async fn serve(&mut self) -> Result<(), ExitCode> {
        self.event("serve")?;
        if !self.wait_signal {
            self.request.cancel();
        }
        std::future::pending().await
    }
    fn stop_accepting(&mut self) -> Result<(), ExitCode> {
        if let Some(cancellation) = &self.cancellation {
            assert!(!cancellation.is_cancelled());
        }
        self.event("stop-accepting")
    }
    async fn drain(&mut self) -> Result<(), ExitCode> {
        if let Some(cancellation) = &self.cancellation {
            assert!(cancellation.is_cancelled());
        }
        self.event("drain")?;
        if self.delay_drain {
            tokio::time::sleep(Duration::from_millis(2500)).await;
        }
        if self.stuck {
            std::future::pending::<()>().await;
        }
        Ok(())
    }
    async fn service_stop(&mut self) -> Result<(), ExitCode> {
        self.event("stop-audit")
    }
    fn release(&mut self) -> Result<(), ExitCode> {
        self.event("release")
    }
}
#[derive(Default)]
struct Reports(Vec<(State, ExitCode)>);
impl Reporter for Reports {
    fn report(&mut self, state: State, code: ExitCode) -> Result<(), ExitCode> {
        self.0.push((state, code));
        Ok(())
    }
}
async fn execute(app: &mut FakeApp) -> (ExitCode, Reports) {
    let request = app.request.clone();
    let mut reports = Reports::default();
    (Host::run(app, request, &mut reports).await, reports)
}
#[tokio::test]
async fn startup_and_stop_order_includes_cancellation_and_durable_audit() {
    let mut app = FakeApp::default();
    let (code, reports) = execute(&mut app).await;
    assert_eq!(code, ExitCode::Clean);
    assert_eq!(
        *app.events.lock().unwrap(),
        [
            "config",
            "layout",
            "store",
            "start",
            "bind",
            "preflight",
            "discovery",
            "serve",
            "stop-accepting",
            "drain",
            "stop-audit",
            "release"
        ]
    );
    assert_eq!(
        reports.0,
        [
            (State::Starting, ExitCode::Clean),
            (State::Running, ExitCode::Clean),
            (State::Stopping, ExitCode::Clean),
            (State::Stopped, ExitCode::Clean)
        ]
    );
}
#[tokio::test]
async fn stop_during_preflight_never_announces_ready_or_discovers() {
    let mut app = FakeApp {
        stop_before_ready: true,
        ..Default::default()
    };
    let (code, reports) = execute(&mut app).await;
    assert_eq!(code, ExitCode::Clean);
    assert!(!reports.0.iter().any(|(state, _)| *state == State::Running));
    assert_eq!(
        &app.events.lock().unwrap()[6..],
        ["stop-accepting", "drain", "stop-audit", "release"]
    );
}
#[tokio::test]
async fn stop_before_start_does_not_open_any_state() {
    let mut app = FakeApp::default();
    app.request.cancel();
    let (code, reports) = execute(&mut app).await;
    assert_eq!(code, ExitCode::Clean);
    assert_eq!(
        *app.events.lock().unwrap(),
        ["stop-accepting", "drain", "release"]
    );
    assert!(!reports.0.iter().any(|(state, _)| *state == State::Running));
}
#[tokio::test]
async fn startup_and_cleanup_failures_are_not_reported_clean() {
    for (stage, expected) in [
        ("config", ExitCode::Config),
        ("layout", ExitCode::Permissions),
        ("store", ExitCode::Store),
        ("start", ExitCode::Store),
        ("bind", ExitCode::Endpoint),
        ("serve", ExitCode::Endpoint),
        ("drain", ExitCode::Store),
        ("stop-audit", ExitCode::Store),
        ("release", ExitCode::Store),
    ] {
        let mut app = FakeApp {
            fail: Some((stage, expected)),
            ..Default::default()
        };
        let (code, reports) = execute(&mut app).await;
        assert_eq!(code, expected, "{stage}");
        assert_eq!(reports.0.last(), Some(&(State::Stopped, expected)));
        assert_eq!(app.events.lock().unwrap().last(), Some(&"release"));
        if stage == "drain" {
            assert!(app.events.lock().unwrap().contains(&"stop-audit"));
        }
    }
}
#[tokio::test(start_paused = true)]
async fn stuck_drain_is_bounded_and_never_claims_stop_audit() {
    let mut app = FakeApp {
        stuck: true,
        ..Default::default()
    };
    let before = tokio::time::Instant::now();
    let (code, reports) = execute(&mut app).await;
    assert_eq!(code, ExitCode::Store);
    assert_eq!(before.elapsed(), Duration::from_secs(10));
    assert!(!app.events.lock().unwrap().contains(&"stop-audit"));
    assert_eq!(app.events.lock().unwrap().last(), Some(&"release"));
    assert!(
        reports
            .0
            .iter()
            .filter(|(state, _)| *state == State::Stopping)
            .count()
            >= 10
    );
}
#[tokio::test(start_paused = true)]
async fn startup_heartbeats_and_late_cancellation_subscribers() {
    let token = Shutdown::default();
    token.cancel();
    token.cancelled().await;
    let mut app = FakeApp {
        delay_start: true,
        ..Default::default()
    };
    let (_, reports) = execute(&mut app).await;
    assert_eq!(
        reports
            .0
            .iter()
            .filter(|(state, _)| *state == State::Starting)
            .count(),
        3
    );
}

#[cfg(unix)]
#[tokio::test]
#[ignore = "subprocess-only signal fixture; parent runs this without privileges"]
async fn unix_signal_fixture() {
    assert_eq!(
        std::env::var("ARX_SERVICE_SIGNAL_FIXTURE").as_deref(),
        Ok("1")
    );
    struct Ready(Reports);
    impl Reporter for Ready {
        fn report(&mut self, state: State, code: ExitCode) -> Result<(), ExitCode> {
            if state == State::Running {
                use std::io::Write;
                println!("SIGNALS_READY");
                std::io::stdout().flush().unwrap();
            }
            self.0.report(state, code)
        }
    }
    let mut app = FakeApp {
        wait_signal: true,
        ..Default::default()
    };
    let request = app.request.clone();
    let signals = super::unix::Signals::install().unwrap();
    let forward = tokio::spawn(signals.forward(request.clone()));
    let mut reports = Ready(Reports::default());
    let code = tokio::time::timeout(
        Duration::from_secs(10),
        Host::run(&mut app, request, &mut reports),
    )
    .await
    .unwrap();
    forward.await.unwrap();
    assert_eq!(code, ExitCode::Clean);
    assert_eq!(
        &app.events.lock().unwrap()[8..],
        ["stop-accepting", "drain", "stop-audit", "release"]
    );
}
#[cfg(unix)]
#[tokio::test]
async fn native_term_and_int_drain_while_hup_and_pipe_are_ignored() {
    use tokio::io::{AsyncBufReadExt, BufReader};
    unsafe extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
    }
    for stop in [15, 2] {
        let mut child = tokio::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "service::tests::unix_signal_fixture",
                "--ignored",
                "--nocapture",
            ])
            .env("ARX_SERVICE_SIGNAL_FIXTURE", "1")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let mut stdout = BufReader::new(child.stdout.take().unwrap()).lines();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if stdout
                    .next_line()
                    .await
                    .unwrap()
                    .expect("fixture alive")
                    .contains("SIGNALS_READY")
                {
                    break;
                }
            }
        })
        .await
        .unwrap();
        let pid = child.id().unwrap() as i32;
        for ignored in [1, 13] {
            assert_eq!(unsafe { kill(pid, ignored) }, 0);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(
            child.try_wait().unwrap().is_none(),
            "HUP/PIPE must not stop service"
        );
        assert_eq!(unsafe { kill(pid, stop) }, 0);
        let status = tokio::time::timeout(Duration::from_secs(5), child.wait())
            .await
            .unwrap()
            .unwrap();
        assert!(status.success(), "signal fixture {stop}: {status}");
    }
}

#[tokio::test(start_paused = true)]
async fn cancellation_interrupts_pending_startup_without_readiness() {
    let mut app = FakeApp {
        delay_start: true,
        ..Default::default()
    };
    let request = app.request.clone();
    let (result, ()) = tokio::join!(execute(&mut app), async {
        tokio::time::sleep(Duration::from_millis(100)).await;
        request.cancel();
    });
    assert_eq!(result.0, ExitCode::Clean);
    assert!(!result.1.0.iter().any(|(state, _)| *state == State::Running));
    assert!(app.events.lock().unwrap().contains(&"stop-audit"));
    assert!(!app.events.lock().unwrap().contains(&"discovery"));
}
#[tokio::test(start_paused = true)]
async fn checkpoint_failure_does_not_discard_cleanup() {
    struct FailingReports {
        stopping: usize,
    }
    impl Reporter for FailingReports {
        fn report(&mut self, state: State, _: ExitCode) -> Result<(), ExitCode> {
            if state == State::Stopping {
                self.stopping += 1;
            }
            if state == State::Stopping && self.stopping > 1 {
                Err(ExitCode::NotServiceManager)
            } else {
                Ok(())
            }
        }
    }
    let mut app = FakeApp {
        delay_drain: true,
        ..Default::default()
    };
    let request = app.request.clone();
    let result = Host::run(&mut app, request, &mut FailingReports { stopping: 0 }).await;
    assert_eq!(result, ExitCode::NotServiceManager);
    assert_eq!(
        &app.events.lock().unwrap()[9..],
        ["drain", "stop-audit", "release"]
    );
}
