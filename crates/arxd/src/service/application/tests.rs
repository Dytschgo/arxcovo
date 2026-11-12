use super::super::{
    Host, Reporter, State,
    config::{
        ServiceConfig,
        tests::{FakeResolver, fixture},
    },
    layout::Platform,
};
use super::*;
use std::{path::PathBuf, sync::Mutex};

pub(crate) fn private_temp() -> tempfile::TempDir {
    #[cfg(target_os = "macos")]
    let dir = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    #[cfg(not(target_os = "macos"))]
    let dir = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    #[cfg(windows)]
    unsafe {
        use std::os::windows::ffi::OsStrExt;
        use std::ptr;
        use windows_sys::Win32::{
            Foundation::LocalFree,
            Security::{Authorization::*, *},
        };
        let identity = crate::local::current_identity().unwrap();
        let text: Vec<u16> = format!("D:P(A;OICI;FA;;;{identity})")
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let mut sd = ptr::null_mut();
        assert_ne!(
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                text.as_ptr(),
                1,
                &mut sd,
                ptr::null_mut()
            ),
            0
        );
        let (mut present, mut defaulted) = (0, 0);
        let mut acl = ptr::null_mut();
        assert_ne!(
            GetSecurityDescriptorDacl(sd, &mut present, &mut acl, &mut defaulted),
            0
        );
        let path: Vec<u16> = dir
            .path()
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
        assert_eq!(
            SetNamedSecurityInfoW(
                path.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                acl,
                ptr::null_mut()
            ),
            0
        );
        LocalFree(sd);
    }
    dir
}
struct Injected {
    dir: PathBuf,
    endpoint: crate::local::Endpoint,
    events: Arc<Mutex<Vec<&'static str>>>,
    failure: Option<&'static str>,
}
impl Injected {
    fn event(&self, name: &'static str) -> Result<(), ExitCode> {
        self.events.lock().unwrap().push(name);
        if self.failure == Some(name) {
            Err(ExitCode::Layout)
        } else {
            Ok(())
        }
    }
}
impl Hooks for Injected {
    fn load(&self) -> Result<(Layout, ServiceConfig), ExitCode> {
        self.event("config")?;
        use sha2::{Digest, Sha256};
        let layout = Layout::for_test(Platform::Linux);
        let mut v = fixture();
        v["agents"][0]["token_sha256"] =
            hex::encode(Sha256::digest("a".repeat(64).as_bytes())).into();
        let config = ServiceConfig::parse_inventory(
            &serde_json::to_vec(&v).unwrap(),
            &layout,
            &FakeResolver::default(),
        )
        .unwrap();
        Ok((layout, config))
    }
    fn layout(&self, _: &Layout) -> Result<Vec<native::Observed>, ExitCode> {
        self.event("layout")?;
        Ok(Vec::new())
    }
    fn store(&self, _: &Layout) -> Result<Activity, ExitCode> {
        self.event("store")?;
        let activity = Activity::open_service_fixture(&self.dir).map_err(|_| ExitCode::Store)?;
        activity
            .start("abandoned", "list_hosts")
            .map_err(|_| ExitCode::Store)?;
        Ok(activity)
    }
    fn bind(&self, _: &Layout, _: String) -> Result<Listener, ExitCode> {
        self.event("bind")?;
        Listener::bind(&self.endpoint).map_err(|_| ExitCode::Endpoint)
    }
    fn preflight(&self, _: &Layout, _: &ServiceConfig) -> ExecutionUnavailable {
        self.event("preflight").unwrap();
        ExecutionUnavailable::PreflightFailed
    }
}
struct Report {
    ready: Arc<tokio::sync::Notify>,
    running: bool,
}
impl Reporter for Report {
    fn report(&mut self, state: State, _: ExitCode) -> Result<(), ExitCode> {
        if state == State::Running {
            self.running = true;
            self.ready.notify_one();
        }
        Ok(())
    }
}
fn injected(dir: &std::path::Path, failure: Option<&'static str>) -> Injected {
    #[cfg(windows)]
    let endpoint = crate::local::Endpoint {
        address: format!(
            r"\\.\pipe\arxcovo-app-test-{}-{}",
            std::process::id(),
            dir.file_name().unwrap().to_string_lossy().replace('.', "-")
        ),
        client_identity: crate::local::current_identity().unwrap(),
    };
    #[cfg(unix)]
    let endpoint = crate::local::Endpoint {
        address: dir.join("a.sock").to_str().unwrap().into(),
        client_identity: unsafe { libc::geteuid() }.to_string(),
    };
    Injected {
        dir: dir.into(),
        endpoint,
        events: Default::default(),
        failure,
    }
}
#[tokio::test]
async fn production_refuses_normal_user_before_fixed_config_access() {
    // No native account lookup, service manager or owner mutation is performed.
    if native::require_service_identity().is_err() {
        assert!(matches!(Native.load(), Err(ExitCode::Permissions)));
    }
}
#[tokio::test]
async fn prerequisites_fail_without_readiness_and_partial_start_audits_are_durable() {
    for stage in ["config", "layout", "store", "bind"] {
        let dir = private_temp();
        let hooks = injected(dir.path(), Some(stage));
        let events = hooks.events.clone();
        let mut app = ServiceApplication::new(hooks);
        let mut reporter = Report {
            ready: Arc::default(),
            running: false,
        };
        assert_eq!(
            Host::run(&mut app, Shutdown::default(), &mut reporter).await,
            ExitCode::Layout
        );
        assert!(!reporter.running);
        let expected = ["config", "layout", "store", "bind"];
        let end = expected.iter().position(|s| *s == stage).unwrap();
        assert_eq!(*events.lock().unwrap(), expected[..=end]);
        if stage == "bind" {
            let db = rusqlite::Connection::open(dir.path().join("activity.sqlite3")).unwrap();
            let count:i64 = db.query_row("SELECT count(*) FROM activity WHERE actor_kind='daemon' AND outcome='succeeded' AND tool IN ('service_start','service_stop')",[],|r| r.get(0)).unwrap();
            assert_eq!(count, 2);
        }
    }
}
#[tokio::test]
async fn cancelled_native_wait_stays_tracked_and_drain_does_not_claim_cancellation() {
    let blocking = Arc::<Blocking>::default();
    let (tx, rx) = std::sync::mpsc::channel();
    let (started, wait_started) = tokio::sync::oneshot::channel();
    let work = blocking.clone();
    let task = tokio::spawn(async move {
        work.run(move || {
            let _ = started.send(());
            rx.recv().unwrap();
            Ok(())
        })
        .await
    });
    wait_started.await.unwrap();
    task.abort();
    let _ = task.await;
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(20), blocking.drain())
            .await
            .is_err()
    );
    tx.send(()).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), blocking.drain())
        .await
        .unwrap();
}
#[tokio::test]
async fn real_normal_user_endpoint_inventory_and_shutdown_audit() {
    let dir = private_temp();
    let hooks = injected(dir.path(), None);
    let endpoint = hooks.endpoint.clone();
    let mut app = ServiceApplication::new(hooks);
    let request = Shutdown::default();
    let stop = request.clone();
    let ready = Arc::new(tokio::sync::Notify::new());
    let mut reporter = Report {
        ready: ready.clone(),
        running: false,
    };
    let client = async move {
        use rmcp::{ServiceExt, transport::async_rw::AsyncRwTransport};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        ready.notified().await;
        let mut stream = crate::local::fixture_connect(endpoint).await;
        stream
            .write_all(format!("ARX1 {}\n", "a".repeat(64)).as_bytes())
            .await
            .unwrap();
        let mut ack = [0; 3];
        stream.read_exact(&mut ack).await.unwrap();
        assert_eq!(&ack, b"OK\n");
        let (r, w) = tokio::io::split(stream);
        let peer = ().serve(AsyncRwTransport::new_client(r, w)).await.unwrap();
        let result = peer
            .call_tool(rmcp::model::CallToolRequestParams::new("list_hosts"))
            .await
            .unwrap();
        let text = serde_json::to_string(&result).unwrap();
        assert!(text.contains("preflight_failed"));
        assert!(!text.contains("development_current_user"));
        peer.cancel().await.unwrap();
        stop.cancel();
    };
    let (code, ()) = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        tokio::join!(Host::run(&mut app, request, &mut reporter), client)
    })
    .await
    .unwrap();
    assert_eq!(code, ExitCode::Clean);
    assert!(reporter.running);
    let activity = Activity::open(dir.path()).unwrap();
    let id = activity.start("other", "list_hosts").unwrap();
    activity.finish(id, "succeeded").unwrap();
    let read = activity.start("test", "list_activity").unwrap();
    let page = activity
        .list(
            "test",
            read,
            &serde_json::from_value(serde_json::json!({"include_reads":true})).unwrap(),
        )
        .unwrap();
    assert!(page.activities.iter().all(|r| r.id != id
        && r.tool != crate::store::Tool::ServiceStart
        && r.tool != crate::store::Tool::ServiceStop));
    activity.finish(read, "succeeded").unwrap();
    let host_id = activity.host_id().unwrap();
    let interrupted = activity.start("test", "list_hosts").unwrap();
    drop(activity);
    let reopened = Activity::open_service_fixture(dir.path()).unwrap();
    assert_eq!(reopened.host_id().unwrap(), host_id);
    drop(reopened);
    assert!(dir.path().join("activity.sqlite3-journal").is_file());
    let db = rusqlite::Connection::open(dir.path().join("activity.sqlite3")).unwrap();
    let rows: i64 = db
        .query_row(
            "SELECT count(*) FROM activity WHERE actor_kind='daemon' AND outcome='succeeded'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(rows, 2);
    let recovered: String = db
        .query_row(
            "SELECT outcome FROM activity WHERE id=?1",
            [interrupted],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(recovered, "interrupted");
    let stopped: i64 = db.query_row("SELECT count(*) FROM activity WHERE principal='abandoned' AND outcome='failed' AND reason='shutdown' AND stop_reason='shutdown'", [], |r|r.get(0)).unwrap();
    assert_eq!(stopped, 1);

    let pending: i64 = db
        .query_row(
            "SELECT count(*) FROM activity WHERE outcome='started'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(pending, 0);
}

#[tokio::test]
async fn response_only_reason_table_completes_and_reads_back_without_private_data() {
    use crate::service::preflight::ExecutionUnavailable::*;
    use crate::store::Reason;
    use rmcp::{ServiceExt, model::CallToolRequestParams};
    use serde_json::{Value, json};
    let dir = private_temp();
    let activity = Arc::new(Activity::open_service_fixture(dir.path()).unwrap());
    let inventory = crate::mcp::Inventory {
        execution: None,
        activity: activity.clone(),
        principal: Some(crate::mcp::Principal("fixture-agent".into())),
    };
    let (server_io, client_io) = tokio::io::duplex(65536);
    let server =
        tokio::spawn(async move { inventory.serve(server_io).await.unwrap().waiting().await });
    let client = ().serve(client_io).await.unwrap();
    let table = [
        (NotConfigured, "not_configured", Reason::Unavailable),
        (
            ServiceModeRequired,
            "service_mode_required",
            Reason::Unavailable,
        ),
        (
            PlatformUnsupported,
            "platform_unsupported",
            Reason::Unavailable,
        ),
        (AccountMissing, "account_missing", Reason::PreflightFailed),
        (
            AccountIdentityChanged,
            "account_identity_changed",
            Reason::PreflightFailed,
        ),
        (RightsMismatch, "rights_mismatch", Reason::PreflightFailed),
        (
            AdminUnsupportedOnPlatform,
            "admin_unsupported_on_platform",
            Reason::PreflightFailed,
        ),
        (
            BatchRightMissing,
            "batch_right_missing",
            Reason::PreflightFailed,
        ),
        (
            MechanismUnavailable,
            "mechanism_unavailable",
            Reason::PreflightFailed,
        ),
        (
            ServiceReconfigurationRequired,
            "service_reconfiguration_required",
            Reason::PreflightFailed,
        ),
        (PreflightFailed, "preflight_failed", Reason::PreflightFailed),
        (Quarantined, "quarantined", Reason::Quarantined),
        (Busy, "busy", Reason::Busy),
        (Stopping, "stopping", Reason::Shutdown),
    ];
    for (detail, token, stored) in table {
        activity.set_service_reason(detail);
        let hosts = client
            .call_tool(CallToolRequestParams::new("list_hosts"))
            .await
            .unwrap();
        let hosts: Value = serde_json::from_str(&hosts.content[0].as_text().unwrap().text).unwrap();
        assert_eq!(hosts["hosts"][0]["execution"]["reason"], token);
        assert_eq!(hosts["hosts"][0]["execution"]["available"], false);
        for tool in ["run_command", "discover_tools"] {
            let args = if tool == "run_command" {
                json!({"tool":"cargo", "args":["--version", "private-argument-marker"]})
            } else {
                json!({})
            };
            let result = client
                .call_tool(
                    CallToolRequestParams::new(tool)
                        .with_arguments(args.as_object().unwrap().clone()),
                )
                .await;
            let error = result.unwrap_err().to_string();
            assert!(error.contains(token));
            let page = client
                .call_tool(CallToolRequestParams::new("list_activity"))
                .await
                .unwrap();
            let page: Value =
                serde_json::from_str(&page.content[0].as_text().unwrap().text).unwrap();
            let row = page["activities"]
                .as_array()
                .unwrap()
                .iter()
                .find(|r| r["tool"] == tool)
                .unwrap();
            assert_eq!(row["reason"], stored.as_str());
            assert_eq!(row["outcome"], "refused");
            assert!(!page.to_string().contains("private-argument-marker"));
            let db = rusqlite::Connection::open(dir.path().join("activity.sqlite3")).unwrap();
            let row: (String,String) = db.query_row("SELECT outcome, reason FROM activity WHERE principal='fixture-agent' AND tool=?1 ORDER BY id DESC LIMIT 1", [tool], |r| Ok((r.get(0)?,r.get(1)?))).unwrap();
            assert_eq!(row, ("refused".into(), stored.as_str().into()));
            assert_eq!(
                db.pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))
                    .unwrap(),
                3
            );
            let columns: Vec<String> = db
                .prepare("PRAGMA table_info(activity)")
                .unwrap()
                .query_map([], |r| r.get(1))
                .unwrap()
                .map(Result::unwrap)
                .collect();
            assert!(
                !columns
                    .iter()
                    .any(|s| s == "execution_reason" || s == "os_code")
            );
        }
    }
    client.cancel().await.unwrap();
    server.await.unwrap().unwrap();
    drop(activity);
    let bytes = std::fs::read(dir.path().join("activity.sqlite3")).unwrap();
    for token in [
        "account_missing",
        "account_identity_changed",
        "rights_mismatch",
        "admin_unsupported_on_platform",
        "batch_right_missing",
        "mechanism_unavailable",
        "service_reconfiguration_required",
        "platform_unsupported",
        "service_mode_required",
        "not_configured",
        "os_code",
        "private-argument-marker",
    ] {
        assert!(!bytes.windows(token.len()).any(|w| w == token.as_bytes()));
    }
}

#[tokio::test]
async fn endpoint_failure_still_drains_dedicated_worker_before_stop_audit() {
    use crate::service::executor::tests::{Gate, GatedBackend, default_fixture as backend_fixture};
    use std::sync::atomic::AtomicUsize;
    let dir = private_temp();
    let mut app = ServiceApplication::new(injected(dir.path(), None));
    let gate = Gate::new();
    let executor = backend_fixture(Arc::new(GatedBackend {
        gate: gate.clone(),
        preflight_gate: false,
        calls: Arc::new(AtomicUsize::new(0)),
    }));
    let mut store = Activity::open_service_fixture(dir.path()).unwrap();
    store.dedicated = Some(executor.clone());
    let activity = Arc::new(store);
    app.activity = Some(activity.clone());
    let id = activity.start("alpha", "run_command").unwrap();
    let work = executor.dispatch(None, activity.clone(), id);
    gate.entered.notified().await;
    app.serving = Some(tokio::spawn(async { Err(ExitCode::Endpoint) }));
    while !app.serving.as_ref().unwrap().is_finished() {
        tokio::task::yield_now().await;
    }
    app.stop_accepting().unwrap();
    let mut drain = Box::pin(app.drain());
    let pending =
        std::future::poll_fn(|cx| std::task::Poll::Ready(drain.as_mut().poll(cx).is_pending()))
            .await;
    let db = rusqlite::Connection::open(dir.path().join("activity.sqlite3")).unwrap();
    assert_eq!(
        db.query_row::<String, _, _>("SELECT outcome FROM activity WHERE id=?", [id], |r| r
            .get(0))
            .unwrap(),
        "started"
    );
    assert_eq!(
        db.query_row::<i64, _, _>(
            "SELECT count(*) FROM activity WHERE tool='service_stop'",
            [],
            |r| r.get(0)
        )
        .unwrap(),
        0
    );
    gate.release();
    assert!(pending, "application drain returned before backend cleanup");
    assert_eq!(drain.await, Err(ExitCode::Endpoint));
    let result = work.await.ok().unwrap();
    assert_eq!(result["metadata"]["stop_reason"], "shutdown");
    app.service_stop().await.unwrap();
    assert_eq!(
        db.query_row::<i64, _, _>(
            "SELECT count(*) FROM activity WHERE outcome='started'",
            [],
            |r| r.get(0)
        )
        .unwrap(),
        0
    );
}

#[tokio::test]
async fn endpoint_error_survives_shutdown_pending_failure() {
    let dir = private_temp();
    let mut app = ServiceApplication::new(injected(dir.path(), None));
    let a = Arc::new(Activity::open_service_fixture(dir.path()).unwrap());
    a.start("alpha", "list_hosts").unwrap();
    let db = rusqlite::Connection::open(dir.path().join("activity.sqlite3")).unwrap();
    db.execute_batch("PRAGMA journal_mode=TRUNCATE; CREATE TRIGGER fail_pending BEFORE UPDATE ON activity BEGIN SELECT RAISE(FAIL,'synthetic'); END;").unwrap();
    app.activity = Some(a);
    app.serving = Some(tokio::spawn(async { Err(ExitCode::Endpoint) }));
    assert_eq!(app.drain().await, Err(ExitCode::Endpoint));
}

#[derive(Clone)]
struct HttpPrincipal(crate::mcp::Inventory);
impl rmcp::ServerHandler for HttpPrincipal {
    async fn call_tool(
        &self,
        request: rmcp::model::CallToolRequestParams,
        mut context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<rmcp::model::CallToolResponse, rmcp::ErrorData> {
        let (mut parts, _) = axum::http::Request::new(()).into_parts();
        parts
            .extensions
            .insert(crate::mcp::Principal("fixture-agent".into()));
        context.extensions.insert(parts);
        self.0.call_tool(request, context).await
    }
}
#[tokio::test]
async fn dedicated_http_principal_rejected_and_audited() {
    use rmcp::ServiceExt;
    let dir = private_temp();
    let (e, calls) = crate::service::executor::tests::fake(false, true);
    let mut a = Activity::open_service_fixture(dir.path()).unwrap();
    a.dedicated = Some(e);
    let handler = HttpPrincipal(crate::mcp::Inventory {
        execution: None,
        activity: Arc::new(a),
        principal: None,
    });
    let (s, c) = tokio::io::duplex(65536);
    let server = tokio::spawn(async move { handler.serve(s).await.unwrap().waiting().await });
    let client = ().serve(c).await.unwrap();
    for tool in ["run_command", "discover_tools"] {
        let args = if tool == "run_command" {
            serde_json::json!({"tool":"cargo"})
        } else {
            serde_json::json!({})
        };
        assert!(
            client
                .call_tool(
                    rmcp::model::CallToolRequestParams::new(tool)
                        .with_arguments(args.as_object().unwrap().clone())
                )
                .await
                .is_err()
        );
    }
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    let db = rusqlite::Connection::open(dir.path().join("activity.sqlite3")).unwrap();
    assert_eq!(db.query_row::<i64,_,_>("SELECT count(*) FROM activity WHERE outcome='rejected' AND reason='invalid_arguments' AND identity_mode='dedicated_account'",[],|r|r.get(0)).unwrap(),2);
    client.cancel().await.unwrap();
    server.await.unwrap().unwrap();
}

#[cfg(windows)]
#[tokio::test]
async fn dedicated_start_mode_precedes_development_and_survives_refusal() {
    use rmcp::ServiceExt;
    use serde_json::{Value, json};
    let dir = private_temp();
    let (_fixture, settings) = crate::development::fixture_settings("echo");
    let development = Arc::new(crate::development::Execution::policy_fixture(settings));
    let mut a = Activity::open_service_fixture(dir.path()).unwrap();
    let e = a.dedicated.as_ref().unwrap().clone();
    a.dedicated = Some(e.clone());
    let activity = Arc::new(a);
    let inventory = crate::mcp::Inventory {
        execution: Some(development),
        activity: activity.clone(),
        principal: Some(crate::mcp::Principal("fixture-agent".into())),
    };
    let db = rusqlite::Connection::open(dir.path().join("activity.sqlite3")).unwrap();
    db.execute_batch("PRAGMA journal_mode=TRUNCATE; CREATE TRIGGER check_mode BEFORE INSERT ON activity WHEN NEW.tool IN ('run_command','discover_tools') AND NEW.identity_mode IS NOT 'dedicated_account' BEGIN SELECT RAISE(FAIL,'wrong start mode'); END;").unwrap();
    let (s, c) = tokio::io::duplex(65536);
    let server = tokio::spawn(async move { inventory.serve(s).await.unwrap().waiting().await });
    let client = ().serve(c).await.unwrap();
    for tool in ["run_command", "discover_tools"] {
        let args = if tool == "run_command" {
            json!({"tool":"cargo"})
        } else {
            json!({})
        };
        let err = client
            .call_tool(
                rmcp::model::CallToolRequestParams::new(tool)
                    .with_arguments(args.as_object().unwrap().clone()),
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("preflight_failed"), "{err}");
    }
    assert!(
        client
            .call_tool(
                rmcp::model::CallToolRequestParams::new("discover_tools")
                    .with_arguments(json!({"unexpected":true}).as_object().unwrap().clone())
            )
            .await
            .unwrap_err()
            .to_string()
            .contains("invalid execution arguments")
    );
    assert_eq!(db.query_row::<i64,_,_>("SELECT count(*) FROM activity WHERE identity_mode='dedicated_account' AND ((outcome='refused' AND reason='preflight_failed') OR (outcome='rejected' AND reason='invalid_arguments'))",[],|r|r.get(0)).unwrap(),3);
    let hosts = client
        .call_tool(rmcp::model::CallToolRequestParams::new("list_hosts"))
        .await
        .unwrap();
    let text = hosts.content[0].as_text().unwrap();
    let value: Value = serde_json::from_str(&text.text).unwrap();
    let h = &value["hosts"][0];
    assert_eq!(h["tools"], json!([]));
    assert_eq!(h.get("tools_observed_at"), Some(&Value::Null));
    assert_eq!(h.get("last_observed_identity"), Some(&Value::Null));
    assert_eq!(h["tool_discovery"], "unavailable");
    client.cancel().await.unwrap();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn dedicated_started_row_is_authoritative_even_on_refusal() {
    use crate::store::{IdentityMode, Metadata, Outcome, Reason};
    let dir = private_temp();
    let a = Activity::open_service_fixture(dir.path()).unwrap();
    let db = rusqlite::Connection::open(dir.path().join("activity.sqlite3")).unwrap();
    for tool in ["run_command", "discover_tools"] {
        let id = a.start_dedicated("alpha", tool).unwrap();
        let read = || {
            db.query_row::<(String, String), _, _>(
                "SELECT outcome,identity_mode FROM activity WHERE id=?",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap()
        };
        assert_eq!(read(), ("started".into(), "dedicated_account".into()));
        assert!(
            a.finish_typed(
                id,
                Outcome::Refused,
                &Metadata {
                    identity_mode: Some(IdentityMode::DevelopmentCurrentUser),
                    reason: Some(Reason::PreflightFailed),
                    ..Default::default()
                },
            )
            .is_err()
        );
        assert_eq!(read(), ("started".into(), "dedicated_account".into()));
        a.finish_typed(
            id,
            Outcome::Refused,
            &Metadata {
                reason: Some(Reason::PreflightFailed),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(read(), ("refused".into(), "dedicated_account".into()));
        assert!(a.finish(id, "failed").is_err());
    }
}

#[tokio::test]
async fn dedicated_report_mcp_privacy_in_memory() {
    use crate::service::executor::{
        self,
        tests::{
            OUTPUT_MARKER, OnceReport, SID_A, SID_B, assert_output_release, assert_private,
            backend_fixture, completed, good,
        },
    };
    use crate::store::Rights;
    use rmcp::ServiceExt;
    use serde_json::{Value, json};
    use std::sync::Mutex;
    // A matching positive control uses the same real MCP path.
    for case in [
        "out_sid_b",
        "out_no_observation",
        "out_ok",
        "out_sid_a_cleanup_null",
    ] {
        let released = ["out_ok", "out_sid_a_cleanup_null"].contains(&case);
        let sid = if case == "out_sid_b" { SID_B } else { SID_A };
        let dir = private_temp();
        let mut evidence = good(super::super::layout::Platform::Windows, Rights::Standard);
        if let Some(executor::Observation::Windows { sid: observed, .. }) =
            &mut evidence.observation
        {
            *observed = sid.into();
        }
        if case == "out_no_observation" {
            evidence.observation = None;
            evidence.point = None;
        }
        if case == "out_sid_a_cleanup_null" {
            evidence.cleanup_confirmed = None;
        }
        let e = backend_fixture(
            Arc::new(OnceReport(Mutex::new(Some(completed(evidence))))),
            Ok(executor::Pin::for_test(
                super::super::layout::Platform::Windows,
                SID_A,
                None,
                Rights::Standard,
            )),
        );
        let mut activity = Activity::open_service_fixture(dir.path()).unwrap();
        activity.dedicated = Some(e.clone());
        let inventory = crate::mcp::Inventory {
            execution: None,
            activity: Arc::new(activity),
            principal: Some(crate::mcp::Principal("fixture-agent".into())),
        };
        let (s, c) = tokio::io::duplex(65536);
        let server = tokio::spawn(async move { inventory.serve(s).await.unwrap().waiting().await });
        let client = ().serve(c).await.unwrap();
        for tool in ["run_command", "list_activity", "list_hosts"] {
            let args = if tool == "run_command" {
                json!({"tool":"cargo"})
            } else {
                json!({})
            };
            let response = client
                .call_tool(
                    rmcp::model::CallToolRequestParams::new(tool)
                        .with_arguments(args.as_object().unwrap().clone()),
                )
                .await
                .unwrap();
            assert_eq!(response.content.len(), 1, "{case} {tool} one text envelope");
            assert!(response.content[0].as_text().is_some());
            for content in &response.content {
                if let Some(text) = content.as_text() {
                    if !released || tool != "run_command" {
                        assert!(!text.text.contains(SID_B), "{case} {tool} raw SID privacy");
                        assert!(
                            !text.text.contains(OUTPUT_MARKER),
                            "{case} {tool} raw marker privacy"
                        );
                    }
                    let value: Value = serde_json::from_str(&text.text).unwrap();
                    if tool == "run_command" {
                        assert_output_release(&value, released, case);
                    } else {
                        assert!(!text.text.contains(SID_B));
                        assert!(!text.text.contains(OUTPUT_MARKER));
                        assert_private(&value);
                    }
                    match tool {
                        "run_command" => {
                            assert_eq!(
                                value["reason"],
                                match case {
                                    "out_sid_b" => json!("identity_mismatch"),
                                    "out_no_observation" => json!("execution_failed"),
                                    "out_sid_a_cleanup_null" => json!("cleanup_unconfirmed"),
                                    "out_ok" => Value::Null,
                                    _ => unreachable!(),
                                }
                            );
                            assert_eq!(
                                value["observed_identity"]["id"],
                                if released { json!(SID_A) } else { Value::Null }
                            );
                            assert_eq!(
                                value["metadata"]["observed_id"],
                                if released { json!(SID_A) } else { Value::Null }
                            );
                            assert_eq!(
                                value["outcome"],
                                if case == "out_ok" {
                                    "succeeded"
                                } else {
                                    "failed"
                                }
                            );
                        }
                        "list_activity" => assert_eq!(
                            value["activities"][0]["observed_id"],
                            if released { json!(SID_A) } else { Value::Null }
                        ),
                        "list_hosts" => {
                            assert_eq!(value["hosts"][0]["last_observed_identity"], Value::Null)
                        }
                        _ => unreachable!(),
                    }
                }
            }
        }
        client.cancel().await.unwrap();
        server.await.unwrap().unwrap();
        e.drain().await.unwrap();
    }
}

#[test]
fn state_store_errors_keep_permissions_separate_from_storage_failures() {
    for error in [Error::Permissions, Error::OwnerRequired] {
        let error = anyhow::Error::new(error).context("fixed test context");
        assert_eq!(store_category(&error), ExitCode::Permissions);
    }
    for error in [
        Error::Config,
        Error::Identity,
        Error::AccountMissing,
        Error::Rights,
        Error::Workspace,
        Error::Layout,
        Error::Manager,
        Error::NativeAdapterMissing,
        Error::NotInstalled,
        Error::Foreign,
        Error::Active,
    ] {
        assert_eq!(store_category(&anyhow::Error::new(error)), ExitCode::Store);
    }
    assert_eq!(
        store_category(&anyhow::anyhow!("synthetic store failure")),
        ExitCode::Store
    );
    assert_eq!(
        store_category(&anyhow::Error::new(std::io::Error::from(
            std::io::ErrorKind::PermissionDenied
        ))),
        ExitCode::Store
    );
}
