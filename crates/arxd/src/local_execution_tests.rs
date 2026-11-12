//! Development execution end to end over the real local transport:
//! SDK client -> real stdio bridge code (`bridge_io`) -> real named pipe with
//! kernel server-identity check -> authenticated listener -> SDK server ->
//! unit-test-only policy-fixture executor (Job, token comparison and bounds are
//! production code; only the token *policy* is skipped for elevated runners).
//! Assertions use synthetic markers and booleans; no private value is printed.
use super::*;
use crate::development::{CHILD, Execution, fixture_settings};
use rmcp::{
    model::{CallToolRequest, CallToolRequestParams, ClientRequest},
    service::PeerRequestOptions,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

type Peer = rmcp::service::RunningService<rmcp::RoleClient, ()>;

fn private_dir() -> tempfile::TempDir {
    use std::{os::windows::ffi::OsStrExt, ptr};
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::{Authorization::*, *},
    };
    let dir = tempfile::tempdir().unwrap();
    let identity = platform::current_identity().unwrap();
    let text: Vec<u16> = format!("D:P(A;OICI;FA;;;{identity})")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    unsafe {
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
async fn connect(address: &str, token: &str) -> Peer {
    let config = super::ClientConfig {
        address: address.into(),
        expected_server_identity: platform::current_identity().unwrap(),
        token: token.into(),
    };
    let (sdk_write, bridge_read) = tokio::io::duplex(FRAME_LIMIT);
    let (bridge_write, sdk_read) = tokio::io::duplex(RESPONSE_LIMIT);
    tokio::spawn(bridge_io(config, bridge_read, bridge_write));
    tokio::time::timeout(
        Duration::from_secs(10),
        ().serve(
            AsyncRwTransport::new_client(sdk_read, sdk_write).with_max_line_length(RESPONSE_LIMIT),
        ),
    )
    .await
    .unwrap()
    .unwrap()
}
fn params(name: &'static str, args: Value) -> CallToolRequestParams {
    CallToolRequestParams::new(name).with_arguments(args.as_object().unwrap().clone())
}
async fn call(peer: &Peer, name: &'static str, args: Value) -> std::result::Result<Value, String> {
    match peer.call_tool(params(name, args)).await {
        Ok(r) => Ok(serde_json::from_str(&r.content[0].as_text().unwrap().text).unwrap()),
        Err(e) => Err(e.to_string()),
    }
}
fn run(action: &str) -> Value {
    json!({"tool":"cargo","args":["--exact",CHILD,"--nocapture",format!("action={action}")],"timeout_s":20,"output_bytes":4096})
}
async fn rows(peer: &Peer) -> Vec<Value> {
    let page = call(
        peer,
        "list_activity",
        json!({"limit":50,"include_reads":true}),
    )
    .await
    .unwrap();
    page["activities"].as_array().unwrap().clone()
}
fn latest<'a>(rows: &'a [Value], tool: &str) -> &'a Value {
    rows.iter().find(|r| r["tool"] == tool).unwrap()
}
fn db(dir: &Path) -> rusqlite::Connection {
    rusqlite::Connection::open(dir.join("activity.sqlite3")).unwrap()
}
async fn until(mut done: impl FnMut() -> bool) {
    for _ in 0..1000 {
        if done() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("condition not reached");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn development_execution_over_real_bridge_pipe_and_sdk() {
    let state = private_dir();
    let (_fixture, settings) = fixture_settings("");
    let workspace = settings.workspace.clone();
    let workspace_text = workspace.to_string_lossy().to_ascii_lowercase();
    let execution = Arc::new(Execution::policy_fixture(settings));
    let (alpha, beta) = ("a".repeat(64), "b".repeat(64));
    let agent = |principal: &str, token: &str| crate::config::Agent {
        principal: principal.into(),
        token_sha256: hex::encode(Sha256::digest(token.as_bytes())),
    };
    let address = format!(r"\\.\pipe\arxcovo-{}", rand::random::<u64>());
    let endpoint = Endpoint {
        address: address.clone(),
        client_identity: platform::current_identity().unwrap(),
    };
    let cfg = Arc::new(crate::config::Config {
        execution: Some(execution.clone()),
        development_execution: None,
        local: None,
        listen: None,
        state_dir: state.path().into(),
        agents: vec![agent("alpha", &alpha), agent("beta", &beta)],
    });
    let activity = Arc::new(crate::store::Activity::open(state.path()).unwrap());
    let listener = Listener::bind(&endpoint).unwrap();
    let server = tokio::spawn(listener.run(cfg, activity));
    let a = connect(&address, &alpha).await;
    let b = connect(&address, &beta).await;
    let sid = platform::current_identity().unwrap();
    let private = |text: &str| {
        let lower = text.to_ascii_lowercase();
        text.contains(&sid) || lower.contains(&workspace_text) || text.contains("S-1-5-21")
    };

    // Schema: no mode, identity, executable or environment field can be named.
    let tools = a.list_tools(None).await.unwrap().tools;
    let schema = &tools
        .iter()
        .find(|t| t.name == "run_command")
        .unwrap()
        .input_schema;
    let mut keys = schema["properties"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    keys.sort();
    assert_eq!(keys, ["args", "cwd", "output_bytes", "timeout_s", "tool"]);
    assert_eq!(schema["additionalProperties"], false);

    let hosts = call(&a, "list_hosts", json!({})).await.unwrap();
    let e = &hosts["hosts"][0]["execution"];
    assert_eq!(e["available"], true);
    assert_eq!(e["mode"], "development_current_user");
    assert_eq!(e["isolation"], "none_shares_owner_user");
    assert_eq!(e["claim"], "development_only_not_dedicated_account");
    assert!(e["rights"].is_null() && e["dedicated"] == false && e["elevated"] == false);
    assert!(
        !private(&hosts.to_string()),
        "list_hosts leaked a private value"
    );

    // Positive fixture run.
    let r = call(&a, "run_command", run("echo")).await.unwrap();
    assert_eq!(r["execution_mode"], "development_current_user");
    assert_eq!(r["claim"], "development_only_not_dedicated_account");
    assert_eq!(
        (r["stop_reason"].as_str(), r["exit_code"].as_i64()),
        (Some("exited"), Some(0))
    );
    assert!(
        r["cleanup_confirmed"] == true
            && r["matches_current_user"] == true
            && r["dedicated"] == false
    );
    let stdout = r["stdout"].as_str().unwrap();
    assert!(stdout.contains("ARX-ECHO-MARKER <workspace>") && stdout.contains("<sid>"));
    assert!(r["stderr"].as_str().unwrap().contains("ARX-ERR-MARKER"));
    assert!(
        !private(&r.to_string()),
        "run_command leaked a private value"
    );
    let list = rows(&a).await;
    let row = latest(&list, "run_command");
    assert_eq!(row["outcome"], "succeeded");
    assert_eq!(row["execution_mode"], "development_current_user");
    assert_eq!(row["identity_mode"], "development_current_user");
    assert_eq!(row["claim"], "development_only_not_dedicated_account");
    assert_eq!(
        (row["program"].as_str(), row["argc"].as_u64()),
        (Some("cargo"), Some(4))
    );
    assert_eq!(
        (row["integrity_rid"].as_u64(), row["elevation"].as_bool()),
        (Some(8192), Some(false))
    );
    assert_eq!(
        (
            row["observation_point"].as_str(),
            row["tree_boundary"].as_str()
        ),
        (Some("pre_exec"), Some("windows_job"))
    );
    assert!(row["cleanup_confirmed"] == true && row["identity_matches_pin"] == true);
    for key in ["observed_kind", "observed_id", "observed_gid", "rights"] {
        assert!(row[key].is_null(), "{key}");
    }
    let success_id = row["id"].as_i64().unwrap();

    // Argument negatives: rejected after `started` commits, nothing resolved or run.
    for bad in [
        json!({"tool":"cargo","mode":"development_current_user"}),
        json!({"tool":"cargo","executable":"C:\\Windows\\System32\\cmd.exe"}),
        json!({"tool":"cargo","identity":"x"}),
        json!({"tool":"cmd"}),
        json!({"tool":"cargo","cwd":"..\\x"}),
        json!({"tool":"cargo","timeout_s":0}),
        json!({"tool":"cargo","args":null}),
    ] {
        let error = call(&a, "run_command", bad).await.unwrap_err();
        assert!(error.contains("invalid execution arguments"));
        let list = rows(&a).await;
        let row = latest(&list, "run_command");
        assert_eq!(
            (row["outcome"].as_str(), row["reason"].as_str()),
            (Some("rejected"), Some("invalid_arguments"))
        );
        assert!(row["program"].is_null() && row["stop_reason"].is_null());
    }
    assert!(
        call(&a, "discover_tools", json!({"x":1}))
            .await
            .unwrap_err()
            .contains("invalid execution arguments")
    );

    // Busy, then SDK cancellation of the running request: the worker still
    // completes the row exactly once, kills the Job tree and does not quarantine.
    let mut options = PeerRequestOptions::no_options();
    options.timeout = Some(Duration::from_secs(3));
    let mut slow = run("tree");
    slow["timeout_s"] = json!(60);
    let handle = a
        .send_cancellable_request(
            ClientRequest::CallToolRequest(CallToolRequest::new(params("run_command", slow))),
            options,
        )
        .await
        .unwrap();
    let pending = tokio::spawn(handle.await_response());
    let probe = execution.clone();
    until(move || probe.is_busy()).await;
    assert!(
        call(&b, "run_command", run("echo"))
            .await
            .unwrap_err()
            .contains("busy")
    );
    assert!(
        pending.await.unwrap().is_err(),
        "client-side timeout sends cancellation"
    );
    let conn = db(state.path());
    let cancelled = || {
        conn.query_row(
            "SELECT outcome,stop_reason,cleanup_confirmed FROM activity WHERE principal='alpha' AND tool='run_command' ORDER BY id DESC LIMIT 1",
            [],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, Option<bool>>(2)?)),
        )
        .unwrap()
    };
    until(|| cancelled().0 != "started").await;
    assert_eq!(
        cancelled(),
        ("failed".into(), Some("cancelled".into()), Some(true))
    );
    let probe = execution.clone();
    until(move || !probe.is_busy()).await;
    assert_eq!(
        execution.capability()["available"],
        true,
        "cancel must not quarantine"
    );

    // Principal isolation: beta sees its own busy refusal, none of alpha's runs.
    let beta_rows = rows(&b).await;
    let busy = latest(&beta_rows, "run_command");
    assert_eq!(
        (busy["outcome"].as_str(), busy["reason"].as_str()),
        (Some("refused"), Some("busy"))
    );
    assert!(
        beta_rows
            .iter()
            .all(|r| r["id"].as_i64() != Some(success_id))
    );
    assert!(
        beta_rows
            .iter()
            .filter(|r| r["tool"] == "run_command")
            .count()
            == 1
    );

    // Start failure: nothing is resolved or run.
    let touched = workspace.join("touched");
    conn.execute_batch("CREATE TRIGGER fail_start BEFORE INSERT ON activity BEGIN SELECT RAISE(FAIL,'injected'); END;").unwrap();
    assert!(
        call(&a, "run_command", run("touch"))
            .await
            .unwrap_err()
            .contains("activity unavailable")
    );
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(!touched.exists(), "start failure must run nothing");
    // Completion failure: the command ran, but its result is withheld.
    conn.execute_batch("DROP TRIGGER fail_start; CREATE TRIGGER fail_finish BEFORE UPDATE ON activity BEGIN SELECT RAISE(FAIL,'injected'); END;").unwrap();
    let error = call(&a, "run_command", run("touch")).await.unwrap_err();
    assert!(error.contains("storage_failed") && !error.contains("ARX-TOUCH-MARKER"));
    assert!(touched.exists(), "positive control: the child ran");
    assert_eq!(
        cancelled().0,
        "started",
        "failed completion leaves the row unfinished"
    );
    conn.execute_batch("DROP TRIGGER fail_finish;").unwrap();
    std::fs::remove_file(&touched).unwrap();

    // Child TokenUser mismatch: refused before resume, then quarantined until restart.
    execution.tamper_identity(true);
    assert!(
        call(&a, "run_command", run("touch"))
            .await
            .unwrap_err()
            .contains("identity_mismatch")
    );
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(!touched.exists(), "suspended child must never run");
    execution.tamper_identity(false);
    let row_reason = |list: &[Value]| {
        latest(list, "run_command")["reason"]
            .as_str()
            .map(String::from)
    };
    assert_eq!(
        row_reason(&rows(&a).await).as_deref(),
        Some("identity_mismatch")
    );
    let hosts = call(&a, "list_hosts", json!({})).await.unwrap();
    assert_eq!(hosts["hosts"][0]["execution_available"], false);
    assert_eq!(hosts["hosts"][0]["execution"]["reason"], "quarantined");
    assert!(
        call(&a, "run_command", run("echo"))
            .await
            .unwrap_err()
            .contains("quarantined")
    );
    assert!(
        call(&a, "discover_tools", json!({}))
            .await
            .unwrap_err()
            .contains("quarantined")
    );
    assert_eq!(row_reason(&rows(&a).await).as_deref(), Some("quarantined"));
    let everything = conn
        .query_row(
            "SELECT group_concat(coalesce(observed_id,'')||coalesce(rights,''),'') FROM activity",
            [],
            |r| r.get::<_, Option<String>>(0),
        )
        .unwrap()
        .unwrap_or_default();
    assert!(everything.is_empty(), "no identity or rights persisted");

    a.cancel().await.unwrap();
    b.cancel().await.unwrap();
    server.abort();
    let _ = server.await;
}

/// The worker is spawned when `dispatch` is called: a future dropped before its
/// first poll still completes the audit row once, and starts no process.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dispatch_dropped_before_first_poll_still_completes_audit() {
    let state = private_dir();
    let (_fixture, settings) = fixture_settings("touch");
    let touched = settings.workspace.join("touched");
    let execution = Arc::new(Execution::policy_fixture(settings));
    let activity = Arc::new(crate::store::Activity::open(state.path()).unwrap());
    let id = activity.start_development("alpha", "run_command").unwrap();
    let request: crate::development::Request =
        serde_json::from_value(json!({"tool":"cargo","args":["--exact",CHILD,"--nocapture"]}))
            .unwrap();
    drop(execution.dispatch(Some(request), activity.clone(), id));
    let conn = db(state.path());
    let row = || {
        conn.query_row(
            "SELECT outcome,stop_reason,program FROM activity WHERE id=?1",
            [id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .unwrap()
    };
    until(|| row().0 != "started").await;
    assert_eq!(
        row(),
        (
            "failed".into(),
            Some("cancelled".into()),
            Some("cargo".into())
        )
    );
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(
        !touched.exists(),
        "a request cancelled before its worker ran starts nothing"
    );
    assert!(!execution.is_busy() && execution.capability()["available"] == true);
    // No spawn means no execution facts: a started-then-killed child would record them.
    let facts: (Option<bool>, Option<bool>) = conn
        .query_row(
            "SELECT cleanup_confirmed,identity_matches_pin FROM activity WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(facts, (None, None));
}

/// Dedicated fake is visibly synthetic; this proves IPC/audit integration only.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dedicated_fake_bridge_schema_audit_and_storage_boundaries() {
    use std::sync::atomic::Ordering;
    let state = private_dir();
    let (execution, calls) = crate::service::executor::tests::fake(false, true);
    let mut store = crate::store::Activity::open_service_fixture(state.path()).unwrap();
    store.dedicated = Some(execution.clone());
    let activity = Arc::new(store);
    let address = format!(r"\\.\pipe\arxcovo-{}", rand::random::<u64>());
    let endpoint = Endpoint {
        address: address.clone(),
        client_identity: platform::current_identity().unwrap(),
    };
    let agent = |principal: &str, token: &str| crate::config::Agent {
        principal: principal.into(),
        token_sha256: hex::encode(Sha256::digest(token.as_bytes())),
    };
    let alpha = "a".repeat(64);
    let beta = "b".repeat(64);
    let cfg = Arc::new(crate::config::Config {
        execution: None,
        development_execution: None,
        local: None,
        listen: None,
        state_dir: state.path().into(),
        agents: vec![agent("alpha", &alpha), agent("beta", &beta)],
    });
    let stop = crate::service::Shutdown::default();
    let listener = Listener::bind(&endpoint).unwrap();
    let server = tokio::spawn(listener.run_service(cfg, activity.clone(), stop.clone()));
    let a = connect(&address, &alpha).await;
    let b = connect(&address, &beta).await;
    let tools = a.list_tools(None).await.unwrap().tools;
    let schema = &tools
        .iter()
        .find(|t| t.name == "run_command")
        .unwrap()
        .input_schema;
    assert!(schema["properties"].get("executable").is_some());
    assert_eq!(schema["additionalProperties"], false);
    for key in ["mode", "principal", "identity", "executor", "config"] {
        let mut v = json!({"tool":"cargo"});
        v[key] = json!("forbidden");
        assert!(
            call(&a, "run_command", v)
                .await
                .unwrap_err()
                .contains("invalid execution arguments")
        );
    }
    for v in [
        json!({"tool":"cargo","executable":r"C:\synthetic\tool.exe"}),
        json!({"executable":"relative"}),
        json!({"tool":"cargo","cwd":"../escape"}),
        json!({"tool":"cargo","tool_typo":"cargo"}),
        json!({"tool":null}),
    ] {
        assert!(call(&a, "run_command", v).await.is_err());
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let conn = db(state.path());
    conn.execute_batch("PRAGMA journal_mode=TRUNCATE;").unwrap();
    conn.execute_batch("CREATE TRIGGER fail_start BEFORE INSERT ON activity BEGIN SELECT RAISE(FAIL,'synthetic'); END;").unwrap();
    assert!(
        call(&a, "run_command", json!({"tool":"cargo"}))
            .await
            .is_err()
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    conn.execute_batch("DROP TRIGGER fail_start").unwrap();
    let result = call(
        &a,
        "run_command",
        json!({"executable":r"C:\synthetic\tool.exe","args":["private-argv"],"cwd":"private-cwd"}),
    )
    .await
    .unwrap();
    assert_eq!(result["test_outcome"], "FakeForTest");
    assert_eq!(result["acceptance"], false);
    assert_eq!(result["metadata"]["observed_id"], Value::Null);
    let id = result["activity_id"].as_i64().unwrap();
    let alpha_rows = rows(&a).await;
    assert!(
        alpha_rows
            .iter()
            .any(|r| r["id"] == id && r["program"] == "path")
    );
    assert!(rows(&b).await.iter().all(|r| r["id"] != id));
    conn.execute_batch("CREATE TRIGGER fail_finish BEFORE UPDATE ON activity BEGIN SELECT RAISE(FAIL,'synthetic'); END;").unwrap();
    assert!(
        call(&a, "run_command", json!({"tool":"cargo"}))
            .await
            .unwrap_err()
            .contains("storage_failed")
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    conn.execute_batch("DROP TRIGGER fail_finish").unwrap();
    a.cancel().await.unwrap();
    b.cancel().await.unwrap();
    stop.cancel();
    server.await.unwrap().unwrap();
    assert!(execution.drain().await.is_err());
    drop(conn);
    drop(activity);
    let bytes = std::fs::read(state.path().join("activity.sqlite3")).unwrap();
    for marker in [
        "private-argv",
        "private-cwd",
        r"C:\synthetic\tool.exe",
        "FakeForTest",
    ] {
        assert!(!bytes.windows(marker.len()).any(|w| w == marker.as_bytes()));
    }
}

// Owned only by this fixture. Dropping the sender releases both preflight and
// the backend even when a cancellation assertion panics (including mutations).
struct CancellationProbe {
    release: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
    preflight_entered: tokio::sync::Notify,
    backend_entered: tokio::sync::Notify,
}
impl crate::service::executor::Executor for CancellationProbe {
    fn preflight(&self) -> Result<(), crate::service::preflight::ExecutionUnavailable> {
        self.preflight_entered.notify_one();
        let _ = self
            .release
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(3));
        Ok(())
    }
    fn run(
        &self,
        _: Option<&crate::service::executor::Request>,
        cancel: &crate::development::Cancellation,
        shutdown: &crate::development::Cancellation,
    ) -> crate::service::executor::Report {
        self.backend_entered.notify_one();
        while !cancel.is_cancelled() && !shutdown.is_cancelled() {
            // Test failure drops the sender; no worker survives a broken guard.
            if matches!(
                self.release
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_millis(2)),
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected)
            ) {
                break;
            }
        }
        crate::service::executor::Report::FakeForTest {
            stop: if shutdown.is_cancelled() {
                crate::store::StopReason::Shutdown
            } else if cancel.is_cancelled() {
                crate::store::StopReason::Cancelled
            } else {
                crate::store::StopReason::Exited
            },
            cleanup: true,
        }
    }
}
async fn cancellation_stage<T>(stage: &str, future: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(20), future)
        .await
        .unwrap_or_else(|_| panic!("cancellation stage timed out: {stage}"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dedicated_fake_sdk_cancellation_propagates_without_shutdown() {
    dedicated_sdk_cancellation(true).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dedicated_fake_sdk_cancellation_explicit_without_shutdown() {
    dedicated_sdk_cancellation(false).await;
}

async fn dedicated_sdk_cancellation(automatic_timeout: bool) {
    let state = private_dir();
    let (release, receive) = std::sync::mpsc::channel();
    let probe = Arc::new(CancellationProbe {
        release: std::sync::Mutex::new(receive),
        preflight_entered: tokio::sync::Notify::new(),
        backend_entered: tokio::sync::Notify::new(),
    });
    let execution = crate::service::executor::tests::backend_fixture(probe.clone());
    let mut store = crate::store::Activity::open_service_fixture(state.path()).unwrap();
    store.dedicated = Some(execution.clone());
    let activity = Arc::new(store);
    let address = format!(r"\\.\pipe\arxcovo-{}", rand::random::<u64>());
    let endpoint = Endpoint {
        address: address.clone(),
        client_identity: platform::current_identity().unwrap(),
    };
    let token = "a".repeat(64);
    let cfg = Arc::new(crate::config::Config {
        execution: None,
        development_execution: None,
        local: None,
        listen: None,
        state_dir: state.path().into(),
        agents: vec![crate::config::Agent {
            principal: "alpha".into(),
            token_sha256: hex::encode(Sha256::digest(token.as_bytes())),
        }],
    });
    let stop = crate::service::Shutdown::default();
    let listener = Listener::bind(&endpoint).unwrap();
    let server = tokio::spawn(listener.run_service(cfg, activity, stop.clone()));
    let a = connect(&address, &token).await;
    let mut options = PeerRequestOptions::no_options();
    options.timeout = Some(Duration::from_millis(200));
    let handle = a
        .send_cancellable_request(
            ClientRequest::CallToolRequest(CallToolRequest::new(params(
                "run_command",
                json!({"tool":"cargo"}),
            ))),
            options,
        )
        .await
        .unwrap();
    cancellation_stage("preflight entry", probe.preflight_entered.notified()).await;
    // Deliberately hold the real dispatch beyond the old 200ms startup budget.
    // rmcp 3.9 starts its response timer when await_response is polled, so keep
    // the handle unpolled until backend entry. This retains automatic timeout
    // coverage without requiring Windows to finish preflight inside 200ms.
    tokio::time::sleep(Duration::from_millis(250)).await;
    release.send(()).expect("preflight release");
    cancellation_stage("backend entry", probe.backend_entered.notified()).await;
    if automatic_timeout {
        let response = cancellation_stage("SDK automatic timeout", handle.await_response()).await;
        assert!(
            matches!(response, Err(rmcp::ServiceError::Timeout { .. })),
            "stage: expected SDK timeout"
        );
    } else {
        cancellation_stage(
            "SDK explicit cancellation",
            handle.cancel(Some("fixture cancellation".into())),
        )
        .await
        .unwrap();
    }
    let conn = db(state.path());
    let state = || {
        conn.query_row::<(String,Option<String>),_,_>("SELECT outcome,stop_reason FROM activity WHERE tool='run_command' ORDER BY id DESC LIMIT 1",[],|r|Ok((r.get(0)?,r.get(1)?))).unwrap()
    };
    cancellation_stage("durable audit completion without shutdown", async {
        while state().0 == "started" {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
    assert_eq!(
        state(),
        ("failed".into(), Some("cancelled".into())),
        "stage: durable cancellation before shutdown"
    );
    a.cancel().await.unwrap();
    execution.shutdown();
    stop.cancel();
    server.await.unwrap().unwrap();
    execution.drain().await.unwrap();
}
