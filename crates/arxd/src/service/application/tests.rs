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

fn private_temp() -> tempfile::TempDir {
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
    fn preflight(&self, _: &Layout, _: &ServiceConfig) -> &'static str {
        self.event("preflight").unwrap();
        "preflight_failed"
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
