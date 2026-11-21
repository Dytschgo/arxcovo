use super::*;

#[test]
fn private_prefixes_reject_bad_masks_and_ipv4_boundary_hosts() {
    let v4 = |s: &str| s.parse::<IpAddr>().unwrap();
    assert!(prefix(v4("192.168.3.9"), v4("255.0.255.0")).is_none());
    assert!(prefix(v4("192.168.3.9"), v4("0.0.0.0")).is_none());
    let route = prefix(v4("192.168.3.9"), v4("255.255.255.0")).unwrap();
    assert!(covers(route, v4("192.168.3.17")));
    assert!(!covers(route, v4("192.168.3.0")));
    assert!(!covers(route, v4("192.168.3.255")));
    assert!(!covers(route, v4("192.168.4.17")));
    let route = prefix(v4("fd00:123::2"), v4("ffff:ffff:ffff:ffff::")).unwrap();
    assert!(covers(route, v4("fd00:123::9")));
    assert!(!covers(route, v4("fd00:124::9")));
}

fn supervisor_worker(root: &Path, behavior: &str) -> Child {
    use std::os::fd::{AsRawFd, FromRawFd};

    const PROBE: &str = r#"
import fcntl, os, socket, sys
uid, gid, inherited = map(int, sys.argv[1:4])
assert os.getresuid() == (uid,) * 3
assert os.getresgid() == (gid,) * 3
assert os.readlink('/proc/self/fd/0') == '/dev/null'
assert os.readlink('/proc/self/fd/1') == '/dev/null'
assert int(sys.argv[5]) == 0 or os.getgroups() == []
try:
    os.fstat(inherited)
except OSError as e:
    assert e.errno == 9
else:
    sys.exit(40)
with socket.socket(fileno=3) as control:
    control.settimeout(3)
    behavior = sys.argv[4]
    if behavior == 'refuse':
        control.sendall(b'X')
        sys.exit(41)
    with open(sys.argv[6], 'r+') as lock, socket.socket() as listener:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        listener.bind(('127.0.0.1', 0))
        listener.listen()
        lock.write(str(listener.getsockname()[1]))
        lock.flush()
        if behavior != 'startup-stop':
            control.sendall(b'R')
        assert control.recv(1) == b'S'
        control.sendall(b'J')
        if behavior == 'drain-refuse':
            assert control.recv(1) == b''
            sys.exit(43)
        assert control.recv(1) == b'C'
        if behavior == 'no-close':
            sys.exit(42)
    control.sendall(b'X')
"#;
    // SAFETY: fixed identity queries and duplication of a live local file.
    let (uid, gid) = unsafe { (libc::geteuid(), libc::getegid()) };
    let (expected_uid, expected_gid, identity) = if uid == 0 {
        (65534, 65534, WorkerIdentity::Dedicated(65534, 65534))
    } else {
        (uid, gid, WorkerIdentity::Inherited(uid, gid))
    };
    let file = std::fs::File::open("/dev/null").unwrap();
    // SAFETY: fcntl duplicates a live descriptor without CLOEXEC.
    let fd = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_DUPFD, 50) };
    assert!(fd >= 50);
    // SAFETY: fcntl created a new owned descriptor.
    let inherited = unsafe { std::os::fd::OwnedFd::from_raw_fd(fd) };
    let mut command = std::process::Command::new("/usr/bin/python3");
    command
        .args([
            "-I",
            "-B",
            "-c",
            PROBE,
            &expected_uid.to_string(),
            &expected_gid.to_string(),
            &inherited.as_raw_fd().to_string(),
            behavior,
            // Only the dedicated identity transition clears supplementary groups.
            &usize::from(uid == 0).to_string(),
        ])
        .arg(root.join("lock"));
    Child::spawn_worker(command, identity, None).unwrap()
}

#[tokio::test]
async fn supervisor_observes_real_child_identity_listener_and_lock_disposition() {
    use std::os::unix::fs::PermissionsExt;
    for behavior in ["clean", "no-close", "startup-stop"] {
        let root = tempfile::tempdir().unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        let path = root.path().join("lock");
        std::fs::write(&path, "").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666)).unwrap();
        let mut child = supervisor_worker(root.path(), behavior);
        if behavior == "startup-stop" {
            assert!(
                tokio::time::timeout(Duration::from_millis(30), child.wait_ready())
                    .await
                    .is_err()
            );
            child.quiesce().await.unwrap();
        } else {
            child.wait_ready().await.unwrap();
            child.quiesce().await.unwrap();
        }
        let lock = std::fs::File::options()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        let port: u16 = std::fs::read_to_string(&path).unwrap().parse().unwrap();
        assert!(lock.try_lock().is_err());
        assert!(std::net::TcpListener::bind(("127.0.0.1", port)).is_err());
        assert_eq!(child.finish().await.is_ok(), behavior != "no-close");
        assert!(child.reaped.is_some());
        assert!(lock.try_lock().is_ok());
        assert!(std::net::TcpListener::bind(("127.0.0.1", port)).is_ok());
    }
}

#[tokio::test]
async fn supervisor_does_not_accept_close_frame_as_readiness() {
    let root = tempfile::tempdir().unwrap();
    let mut child = supervisor_worker(root.path(), "refuse");
    assert!(child.wait_ready().await.is_err());
    assert!(child.failed_or_exited().await.is_err());
    assert!(!child.observe_reap().await);
    assert_eq!(child.reaped.unwrap().code(), Some(41));
}

#[tokio::test]
async fn foreground_founder_attach_refusal_waits_for_admitted_owner_task() {
    let fence = Arc::new(Admission::default());
    let (running, mut cluster_closed) = activation::refused_founder_for_test(&fence, true);
    let (finish_owner, owner_work) = tokio::sync::oneshot::channel();
    let (admitted, admission) = tokio::sync::oneshot::channel();
    let owner = tokio::spawn(async move {
        admitted.send(()).unwrap();
        owner_work.await.unwrap();
        Ok(())
    });
    admission.await.unwrap();
    let (_status, receiver) = watch::channel(Status::Refused);
    let mut lifecycle = Lifecycle {
        work: Arc::new(Mutex::new(Work {
            waiting: None,
            running: Some(running),
            first: None,
        })),
        fence: fence.clone(),
        joined: false,
        status: receiver,
        control: cluster_owner::Control::observe_task(owner),
        supervisor: tokio::spawn(async {}),
    };
    let closing = tokio::spawn(async move {
        lifecycle.quiesce().await.unwrap();
        lifecycle.close().await
    });
    fence.signal().cancelled().await;
    assert!(matches!(
        cluster_closed.try_recv(),
        Err(tokio::sync::oneshot::error::TryRecvError::Empty)
    ));
    finish_owner.send(()).unwrap();
    cluster_closed.await.unwrap();
    assert_eq!(closing.await.unwrap(), Ok(true));
}

#[test]
fn lifecycle_records_retained_learner_lock_separately_from_unconfirmed_close() {
    let retained = LifecycleFailure::from(activation::Error::LearnerLockRetained);
    let unconfirmed = LifecycleFailure::from(activation::Error::Close);
    assert_eq!(retained, LifecycleFailure::LearnerLockRetained);
    assert_eq!(unconfirmed, LifecycleFailure::RefusedOrUnconfirmed);
    assert_eq!(retained.category(), "cluster-learner-outer-lock-retained");
    assert_eq!(
        unconfirmed.category(),
        "cluster-lifecycle-refused-or-unconfirmed"
    );
}

#[test]
fn inherited_listener_refuses_wrong_address_and_non_listening_descriptors() {
    use std::os::fd::AsRawFd;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    assert!(validate_listener(&listener, address, true).is_ok());
    assert!(validate_listener(&listener, "127.0.0.1:0".parse().unwrap(), true).is_err());
    assert!(
        qualify_listener(listener.as_raw_fd(), Some("[::1]:0".parse().unwrap()), true).is_err()
    );
    let unbound = tokio::net::TcpSocket::new_v4().unwrap();
    assert!(qualify_listener(unbound.as_raw_fd(), None, true).is_err());
    let root = tempfile::tempdir().unwrap();
    let unix = std::os::unix::net::UnixListener::bind(root.path().join("socket")).unwrap();
    assert!(qualify_listener(unix.as_raw_fd(), None, true).is_err());
}

struct DrainProbe(Child, bool, Option<Result<(), super::super::ExitCode>>);

impl crate::service::Application for DrainProbe {
    async fn load_config(&mut self) -> Result<(), super::super::ExitCode> {
        Ok(())
    }
    async fn verify_layout(&mut self) -> Result<(), super::super::ExitCode> {
        Ok(())
    }
    async fn open_store(&mut self) -> Result<(), super::super::ExitCode> {
        Ok(())
    }
    async fn service_start(&mut self) -> Result<(), super::super::ExitCode> {
        Ok(())
    }
    async fn bind(&mut self, _: Shutdown) -> Result<(), super::super::ExitCode> {
        self.0.wait_ready().await?;
        // Enter production cleanup without keeping an ordinary endpoint alive.
        Err(super::super::ExitCode::Endpoint)
    }
    async fn preflight(&mut self) {}
    fn start_discovery(&mut self) {}
    async fn serve(&mut self) -> Result<(), super::super::ExitCode> {
        unreachable!()
    }
    fn stop_accepting(&mut self) -> Result<(), super::super::ExitCode> {
        Ok(())
    }
    async fn drain(&mut self) -> Result<(), super::super::ExitCode> {
        self.0.quiesce().await?;
        assert!(self.0.process.try_wait().unwrap().is_none());
        if self.1 {
            Err(super::super::ExitCode::Store)
        } else {
            Ok(())
        }
    }
    async fn finish_after_drain(&mut self, drained: bool) -> Result<(), super::super::ExitCode> {
        let result = self.0.finish_after_drain(drained).await;
        self.2 = Some(result);
        result
    }
    async fn service_stop(&mut self) -> Result<(), super::super::ExitCode> {
        Ok(())
    }
    fn release(&mut self) -> Result<(), super::super::ExitCode> {
        Ok(())
    }
}

#[tokio::test]
async fn service_drain_error_denies_close_now_and_still_reaps_child() {
    use crate::service::{ExitCode, Host};
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = root.path().join("lock");
    std::fs::write(&path, "").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666)).unwrap();
    let mut application = DrainProbe(supervisor_worker(root.path(), "drain-refuse"), true, None);
    assert_eq!(
        Host::run(&mut application, Shutdown::default(), &mut Silent).await,
        ExitCode::Endpoint
    );
    assert_eq!(application.0.reaped.unwrap().code(), Some(43));
}

#[tokio::test]
async fn worker_refusal_reports_before_stop_and_waits_for_close_authorization() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    for (initial, confirmed) in [
        (Status::Refused, true),
        (Status::Pending, true),
        (Status::Pending, false),
    ] {
        let fence = Arc::new(Admission::default());
        let (running, mut cluster_closed) = activation::refused_founder_for_test(&fence, confirmed);
        let (status, receiver) = watch::channel(initial);
        let lifecycle = Lifecycle {
            work: Arc::new(Mutex::new(Work {
                waiting: None,
                running: Some(running),
                first: Some(activation::Error::Listener),
            })),
            fence: fence.clone(),
            joined: false,
            status: receiver,
            control: cluster_owner::Control::observe_task(tokio::spawn(async { Ok(()) })),
            supervisor: tokio::spawn(async {}),
        };
        let (mut parent, worker) = tokio::net::UnixStream::pair().unwrap();
        let stop = fence.signal();
        let worker = ParentControl::new(worker, fence);
        let task = tokio::spawn(run_lifecycle(lifecycle, stop, Some(worker)));
        tokio::time::timeout(Duration::from_secs(2), async {
            if initial == Status::Pending {
                assert_eq!(parent.read_u8().await.unwrap(), READY);
                status.send_replace(Status::Refused);
            }
            assert_eq!(parent.read_u8().await.unwrap(), FAILED);
            assert!(!task.is_finished());
            parent.write_u8(STOP).await.unwrap();
            assert_eq!(parent.read_u8().await.unwrap(), JOINED);
            assert!(matches!(
                cluster_closed.try_recv(),
                Err(tokio::sync::oneshot::error::TryRecvError::Empty)
            ));
            assert!(!task.is_finished());
            parent.write_u8(CLOSE_NOW).await.unwrap();
            cluster_closed.await.unwrap();
            if confirmed {
                assert_eq!(parent.read_u8().await.unwrap(), CLOSED);
            } else {
                assert_eq!(parent.read(&mut [0; 1]).await.unwrap(), 0);
            }
            assert_eq!(
                task.await.unwrap(),
                Err(LifecycleFailure::RefusedOrUnconfirmed)
            );
        })
        .await
        .unwrap();
    }
}

struct Silent;

impl crate::service::Reporter for Silent {
    fn report(
        &mut self,
        _: crate::service::State,
        _: crate::service::ExitCode,
    ) -> Result<(), crate::service::ExitCode> {
        Ok(())
    }
}

fn fixture_worker(selector: &str, cwd: &Path, stamp: Option<ConfigStamp>) -> Child {
    // SAFETY: scalar identity getters take no pointers.
    let (uid, gid) = unsafe { (libc::geteuid(), libc::getegid()) };
    let identity = if uid == 0 {
        WorkerIdentity::Dedicated(65534, 65534)
    } else {
        WorkerIdentity::Inherited(uid, gid)
    };
    let mut command = std::process::Command::new("/proc/self/exe");
    command
        .current_dir(cwd)
        .args([selector, "--exact", "--ignored", "--nocapture"]);
    Child::spawn_worker(command, identity, stamp).unwrap()
}

const REFUSAL_FIXTURE: &str = "service::cluster_runtime::tests::refused_worker_fixture";

#[tokio::test]
#[ignore = "spawned by the parent shutdown control"]
async fn refused_worker_fixture() {
    use std::os::fd::FromRawFd;
    // SAFETY: spawn_worker transferred ownership of the fixed control FD.
    let stream = unsafe { std::os::unix::net::UnixStream::from_raw_fd(CONTROL_FD) };
    stream.set_nonblocking(true).unwrap();
    let fence = Arc::new(Admission::default());
    let (running, closed) = activation::refused_founder_for_test(&fence, true);
    let (_sender, status) = watch::channel(Status::Refused);
    let lifecycle = Lifecycle {
        work: Arc::new(Mutex::new(Work {
            waiting: None,
            running: Some(running),
            first: None,
        })),
        fence: fence.clone(),
        joined: false,
        status,
        control: cluster_owner::Control::observe_task(tokio::spawn(async { Ok(()) })),
        supervisor: tokio::spawn(async {}),
    };
    let parent = ParentControl::new(
        tokio::net::UnixStream::from_std(stream).unwrap(),
        fence.clone(),
    );
    assert_eq!(
        run_lifecycle(lifecycle, fence.signal(), Some(parent)).await,
        Err(LifecycleFailure::RefusedOrUnconfirmed)
    );
    closed.await.unwrap();
    std::process::exit(41);
}

#[tokio::test]
async fn service_parent_drains_and_reaps_refused_worker_after_confirmed_close() {
    use crate::service::{ExitCode, Host};
    let child = fixture_worker(REFUSAL_FIXTURE, Path::new("/"), None);
    let mut application = DrainProbe(child, false, None);
    assert_eq!(
        Host::run(&mut application, Shutdown::default(), &mut Silent).await,
        ExitCode::Endpoint
    );
    assert_eq!(application.2, Some(Err(ExitCode::Endpoint)));
    assert_eq!(application.0.reaped.unwrap().code(), Some(41));
}

fn private_fixture() -> (tempfile::TempDir, u32, PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    // SAFETY: scalar identity getters take no pointers.
    let (effective_uid, effective_gid) = unsafe { (libc::geteuid(), libc::getegid()) };
    let (uid, gid) = if effective_uid == 0 {
        (65534, 65534)
    } else {
        (effective_uid, effective_gid)
    };
    let root = tempfile::tempdir().unwrap();
    let qualify = |path: &Path, mode| {
        if effective_uid == 0 {
            std::os::unix::fs::chown(path, Some(uid), Some(gid)).unwrap();
        }
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    };
    let path = |name| root.path().join(name);
    for (name, mode) in [
        ("workspace", 0o700),
        ("state", 0o700),
        ("credentials", 0o700),
        ("wrap", 0o700),
        ("pki", 0o700),
        ("pki/custody", 0o700),
        ("history", 0o700),
        ("control", 0o2710),
    ] {
        let dir = path(name);
        std::fs::create_dir(&dir).unwrap();
        qualify(&dir, mode);
    }
    let config = path("cluster.json");
    std::fs::write(
        &config,
        serde_json::to_vec(&serde_json::json!({
            "workspace": path("workspace"), "state": path("state"),
            "credentials": path("credentials"), "wrap": path("wrap"),
            "custody": path("pki/custody"), "history": path("history"),
            "owner_socket": path("control/owner"), "peer": "192.168.1.9:2000",
            "enrollment": "192.168.1.9:2001", "owner_uid": 1, "daemon_uid": uid,
            "daemon_gid": gid, "agent_uid": 3, "build_uid": 4, "owner_gid": gid,
        }))
        .unwrap(),
    )
    .unwrap();
    qualify(&config, 0o600);
    qualify(root.path(), 0o700);
    (root, uid, config)
}

#[test]
fn worker_config_stamp_rejects_in_place_edit_and_object_replacement() {
    let (root, uid, config) = private_fixture();
    let (_, stamp) = Settings::vetted(&config, uid, &[], None).unwrap();
    assert!(Settings::vetted(&config, uid, &[], Some(stamp)).is_ok());
    assert!(Settings::vetted(&config, uid, &[&root.path().join("state/ordinary")], None).is_err());
    let alias = root.path().join("ordinary-link");
    std::os::unix::fs::symlink(root.path().join("state"), &alias).unwrap();
    assert!(Settings::vetted(&config, uid, &[&alias], None).is_err());

    let contents = std::fs::read(&config).unwrap();
    let edited = [contents.as_slice(), b"\n"].concat();
    std::fs::write(&config, edited).unwrap();
    assert!(Settings::vetted(&config, uid, &[], None).is_ok());
    assert!(Settings::vetted(&config, uid, &[], Some(stamp)).is_err());
    std::fs::write(&config, contents).unwrap();
    let original = root.path().join("original.json");
    std::fs::rename(&config, &original).unwrap();
    std::fs::copy(&original, &config).unwrap();
    // SAFETY: geteuid takes no pointers.
    if unsafe { libc::geteuid() } == 0 {
        let metadata = std::fs::metadata(original).unwrap();
        std::os::unix::fs::chown(&config, Some(uid), Some(metadata.gid())).unwrap();
    }
    assert!(Settings::vetted(&config, uid, &[], None).is_ok());
    assert!(Settings::vetted(&config, uid, &[], Some(stamp)).is_err());
}

const CONFIG_FIXTURE: &str = "service::cluster_runtime::tests::config_worker_fixture";

#[tokio::test]
#[ignore = "spawned by the parent layout substitution control"]
async fn config_worker_fixture() {
    use std::os::fd::FromRawFd;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    // SAFETY: spawn_worker transferred ownership of the fixed control FD.
    let stream = unsafe { std::os::unix::net::UnixStream::from_raw_fd(CONTROL_FD) };
    stream.set_nonblocking(true).unwrap();
    let mut stream = tokio::net::UnixStream::from_std(stream).unwrap();
    let mut stamp = ConfigStamp([0; 32]);
    stream.read_exact(&mut stamp.0).await.unwrap();
    // SAFETY: geteuid takes no pointers.
    let uid = unsafe { libc::geteuid() };
    let path = std::env::current_dir().unwrap().join("cluster.json");
    assert!(Settings::vetted(&path, uid, &[], None).is_ok());
    if Settings::vetted(&path, uid, &[], Some(stamp)).is_err() {
        std::process::exit(42);
    }
    stream.write_u8(READY).await.unwrap();
}

#[tokio::test]
async fn worker_config_stamp_rejects_replaced_layout_ancestor() {
    use std::os::unix::fs::PermissionsExt;
    let (root, uid, config) = private_fixture();
    let ordinary = root.path().join("absent-ordinary");
    let (_, stamp) = Settings::vetted(&config, uid, &[&ordinary], None).unwrap();
    let check = |stamp| fixture_worker(CONFIG_FIXTURE, root.path(), Some(stamp));
    let mut original = check(stamp);
    assert!(original.wait_ready().await.is_ok());
    assert!(original.observe_reap().await);
    let ancestor = root.path().join("pki");
    let moved = root.path().join("pki-original");
    std::fs::rename(&ancestor, &moved).unwrap();
    std::fs::create_dir(&ancestor).unwrap();
    let metadata = std::fs::metadata(&moved).unwrap();
    // SAFETY: geteuid takes no pointers.
    if unsafe { libc::geteuid() } == 0 {
        std::os::unix::fs::chown(&ancestor, Some(uid), Some(metadata.gid())).unwrap();
    }
    std::fs::set_permissions(&ancestor, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::rename(moved.join("custody"), ancestor.join("custody")).unwrap();
    let mut replaced = check(stamp);
    assert!(replaced.wait_ready().await.is_err());
    assert!(!replaced.observe_reap().await);
    assert_eq!(replaced.reaped.unwrap().code(), Some(42));
    assert!(!root.path().join("control/owner").exists());
}
