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

#[test]
fn runtime_rejects_overlapping_or_substituted_layouts() {
    let root = tempfile::tempdir().unwrap();
    let a = root.path().join("a");
    let b = root.path().join("b");
    let c = a.join("c");
    std::fs::create_dir_all(&c).unwrap();
    std::fs::create_dir(&b).unwrap();
    assert!(disjoint(&[&a, &b]).is_ok());
    assert!(disjoint(&[&a, &a]).is_err());
    assert!(disjoint(&[&a, &c]).is_err());
    let link = root.path().join("link");
    std::os::unix::fs::symlink(&b, &link).unwrap();
    assert!(disjoint(&[&a, &link]).is_err());
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
            &usize::from(uid != 0).to_string(),
        ])
        .arg(root.join("lock"));
    Child::spawn_worker(command, identity).unwrap()
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
    assert!(child.exited().await.is_err());
    assert_eq!(child.reaped.unwrap().code(), Some(41));
}

#[tokio::test]
async fn foreground_founder_attach_refusal_waits_for_admitted_owner_task() {
    let fence = Arc::new(Admission::default());
    let (running, mut cluster_closed) = activation::refused_founder_for_test(&fence);
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
            first: Some(activation::Error::Listener),
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
    assert_eq!(closing.await.unwrap(), Err(()));
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

struct FailingDrain(Child);

impl crate::service::Application for FailingDrain {
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
        Err(super::super::ExitCode::Store)
    }
    async fn finish_after_drain(&mut self, drained: bool) -> Result<(), super::super::ExitCode> {
        self.0.finish_after_drain(drained).await
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
    use crate::service::{ExitCode, Host, Reporter, State};
    use std::os::unix::fs::PermissionsExt;
    struct Silent;
    impl Reporter for Silent {
        fn report(&mut self, _: State, _: ExitCode) -> Result<(), ExitCode> {
            Ok(())
        }
    }
    let root = tempfile::tempdir().unwrap();
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = root.path().join("lock");
    std::fs::write(&path, "").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666)).unwrap();
    let mut application = FailingDrain(supervisor_worker(root.path(), "drain-refuse"));
    assert_eq!(
        Host::run(&mut application, Shutdown::default(), &mut Silent).await,
        ExitCode::Endpoint
    );
    assert_eq!(application.0.reaped.unwrap().code(), Some(43));
}
