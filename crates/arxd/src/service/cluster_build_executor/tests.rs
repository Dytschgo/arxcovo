use super::*;
use std::sync::atomic::{AtomicU8, Ordering};

const PIN: Pin = Pin {
    uid: 21072,
    gid: 21072,
    slots: 1,
};

fn request() -> Request {
    Request {
        tool: Some("cargo".into()),
        executable: None,
        args: vec!["--version".into()],
        cwd: String::new(),
        timeout_s: 5,
        output_bytes: 4096,
    }
}

#[test]
fn wire_bounds_and_shape() {
    assert!(frame_len((MAX_FRAME as u32 + 1).to_be_bytes()).is_err());
    assert!(frame_len([0; 4]).is_err());
    let frame = frame(&Operation::Execute(request())).unwrap();
    let Operation::Execute(decoded) = serde_json::from_slice(&frame[4..]).unwrap() else {
        panic!("expected execute")
    };
    assert!(decoded.validate());
    assert!(
        serde_json::from_slice::<Operation>(br#"{"Execute":{"tool":"cargo","uid":0}}"#).is_err()
    );
    assert!(!canonical_path(Path::new("/var/lib//stage")));
    assert!(!canonical_path(Path::new("/var/lib/stage/../control")));
}

#[test]
fn actual_peer_identity_is_observed() {
    let (a, b) = UnixStream::pair().unwrap();
    // SAFETY: these identity getters take no pointers.
    let expected = unsafe { (libc::geteuid(), libc::getegid()) };
    assert_eq!(credentials(&a).unwrap(), expected);
    assert_eq!(credentials(&b).unwrap(), expected);
}

#[test]
fn socket_connect_refuses_a_full_backlog_without_a_blocking_wait() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("executor.sock");
    let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
    // SAFETY: listen only changes the backlog on this owned socket.
    assert_eq!(unsafe { libc::listen(listener.as_raw_fd(), 0) }, 0);
    let _queued = open_socket(&path).unwrap();
    assert_eq!(
        open_socket(&path).unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
}

#[test]
fn nonroot_cannot_enter_broker() {
    // SAFETY: identity getter takes no pointers.
    if unsafe { libc::geteuid() } != 0 {
        assert_eq!(
            entry(&["cluster-build-broker".into(), "/unused".into()]),
            Some(1)
        );
    }
}

struct Observed(AtomicU8);
impl Executor for Observed {
    fn preflight(&self) -> Result<(), Unavailable> {
        Ok(())
    }

    fn run(&self, _: Option<&Request>, cancel: &Cancellation, _: &Cancellation) -> Report {
        self.0.fetch_add(1, Ordering::AcqRel);
        let deadline = Instant::now() + Duration::from_secs(2);
        while !cancel.is_cancelled() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(cancel.is_cancelled());
        self.0.fetch_add(1, Ordering::AcqRel);
        Report::CancelledBeforeSpawn
    }
}

#[tokio::test]
async fn status_and_invalid_descriptor_do_not_dispatch() {
    let bridge = Arc::new(Observed(AtomicU8::new(0)));
    for (operation, ready) in [
        (Operation::Status, true),
        (
            Operation::Execute(Request {
                cwd: "../other".into(),
                ..request()
            }),
            false,
        ),
    ] {
        let (mut client, server) = ServerStream::pair().unwrap();
        let server = tokio::spawn(serve(
            server,
            bridge.clone(),
            PIN,
            Cancellation::default(),
            Arc::new(Semaphore::new(1)),
        ));
        client.write_all(&frame(&operation).unwrap()).await.unwrap();
        let mut len = [0; 4];
        client.read_exact(&mut len).await.unwrap();
        let mut body = vec![0; frame_len(len).unwrap()];
        client.read_exact(&mut body).await.unwrap();
        let reply: Reply = serde_json::from_slice(&body).unwrap();
        assert!(if ready {
            matches!(reply, Reply::Ready(PIN))
        } else {
            matches!(reply, Reply::Failed(Creation::NotInvoked))
        });
        server.await.unwrap();
    }
    assert_eq!(bridge.0.load(Ordering::Acquire), 0);
}

#[tokio::test]
async fn observer_loss_and_cancel_are_joined() {
    for explicit in [false, true] {
        let bridge = Arc::new(Observed(AtomicU8::new(0)));
        let (mut client, server) = ServerStream::pair().unwrap();
        let server = tokio::spawn(serve(
            server,
            bridge.clone(),
            PIN,
            Cancellation::default(),
            Arc::new(Semaphore::new(1)),
        ));
        client
            .write_all(&frame(&Operation::Execute(request())).unwrap())
            .await
            .unwrap();
        timeout(HANDSHAKE, async {
            while bridge.0.load(Ordering::Acquire) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        if explicit {
            client.write_u8(CANCEL).await.unwrap();
        } else {
            drop(client);
        }
        timeout(HANDSHAKE, server).await.unwrap().unwrap();
        assert_eq!(bridge.0.load(Ordering::Acquire), 2);
    }
}

#[test]
#[ignore = "root-only refusal in the isolated QEMU validation guest"]
fn root_refuses_unqualified_runner_profile() {
    // SAFETY: this identity getter takes no pointers.
    assert_eq!(unsafe { libc::geteuid() }, 0);
    let config = crate::service::config::tests::parse(
        crate::service::config::tests::fixture(),
        &crate::service::config::tests::FakeResolver::default(),
    )
    .unwrap();
    let result = crate::service::unix_bridge::broker(
        ("/unopened-binary".into(), "/unopened-root".into()),
        &config,
        Box::new(|| panic!("unqualified membership reached configuration check")),
    );
    assert!(matches!(result, Err("membership")));
}

#[tokio::test]
#[ignore = "A10 hosted transient broker, one grant per job"]
async fn hosted_standard_build() {
    use crate::store::{Activity, ListActivity, Outcome};
    assert_eq!(std::env::var("ARX_EXEC_HOSTED").as_deref(), Ok("1"));
    let base = PathBuf::from(std::env::var_os("ARX_EXEC_BASE").unwrap());
    assert!(base.starts_with("/var/lib"));
    // SAFETY: these identity getters take no pointers.
    assert_eq!(unsafe { (libc::getuid(), libc::geteuid()) }, (21071, 21071));
    let socket = base.join("control/executor.sock");
    let _retained_fd = File::open(base.join("daemon/wrap")).unwrap();
    let pid = std::process::id();
    timeout(HANDSHAKE, async {
        while fs::read_to_string(base.join("daemon.pid")).as_deref().ok()
            != Some(pid.to_string().as_str())
        {
            tokio::time::sleep(POLL).await;
        }
    })
    .await
    .unwrap();
    let mut request = request();
    request.timeout_s = 8;
    let execution = connect(socket, PIN.uid).unwrap();
    let activity = Arc::new(Activity::open_service_fixture(&base.join("daemon/history")).unwrap());
    let id = activity
        .start_dedicated("qualification", "run_command")
        .unwrap();
    let result = execution
        .dispatch(Some(request), activity.clone(), id)
        .await
        .unwrap_or_else(|_| panic!("dispatch refused"));
    execution.drain().await.unwrap();
    assert_eq!(result["outcome"], "succeeded");
    assert_eq!(result["exit_code"], 0);
    assert_eq!(result["cleanup_confirmed"], true);
    assert_eq!(result["tree_boundary"], "linux_cgroup");
    let read_id = activity.start("qualification", "list_activity").unwrap();
    let rows = activity
        .list_filtered(
            "qualification",
            read_id,
            &ListActivity {
                limit: 2,
                before_id: None,
                include_reads: false,
            },
            Some(id),
        )
        .unwrap()
        .activities;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].outcome, Outcome::Succeeded);
    let output = format!(
        "{}{}",
        result["stdout"]["head"].as_str().unwrap(),
        result["stdout"]["tail"].as_str().unwrap()
    );
    assert_eq!(result["stdout"]["dropped_bytes"], 0);
    let mut lines = output.lines();
    assert_eq!(
        lines.next(),
        Some("A10 child migration=1 limits=1 identity=1 denial=1 workspace=1")
    );
    assert!(lines.next().is_some_and(|line| line.starts_with("cargo ")));
    assert!(lines.next().is_none());
    println!(
        "ARX_EXEC_BUILD dispatches=1 migration=1 limits=1 identity=1 denial=1 workspace=1 terminal=1 cleanup=1"
    );
}
