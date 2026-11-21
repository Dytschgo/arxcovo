use super::*;
use std::{
    os::unix::{fs::symlink, process::ExitStatusExt},
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio::sync::Notify;

#[tokio::test]
async fn owner_protocol_refuses_oversize_extra_and_non_owner_operations_before_dispatch() {
    for request in [
        b"ARXO\x01\x22\x61".as_slice(), // 8801 bytes
        b"ARXO\x01\x00\x00x",
        b"ARXO\x02\x00\x02x",
        b"ARXO\x03\x00\x00",
        b"ARXO\x01\x00\x00ARXO\x01\x00\x00",
        b"\x16\x03\x01\x00\x03TLS", // No mTLS credential parsing on this endpoint.
    ] {
        let (server, mut client) = UnixStream::pair().unwrap();
        client.write_all(request).await.unwrap();
        client.shutdown().await.unwrap();
        connection(server, 1, Shutdown::default(), |_, _| async {
            panic!("malformed owner request dispatched")
        })
        .await;
        assert!(matches!(receive(&mut client).await.unwrap(), (13, _)));
    }
}

#[tokio::test]
async fn owner_protocol_lost_receipt_returns_unknown_without_retry() {
    let (mut server, client) = UnixStream::pair().unwrap();
    let work = tokio::spawn(async move {
        assert!(matches!(
            read_request(&mut server).await.unwrap(),
            Request::Mint
        ));
        // Possible effect before the single connection is lost.
    });
    assert!(matches!(
        request(client, Request::Mint).await,
        Err(Failure::Unknown)
    ));
    work.await.unwrap();
}

#[tokio::test]
async fn owner_protocol_admitted_work_survives_disconnect_and_stop_once() {
    let (server, mut client) = UnixStream::pair().unwrap();
    let count = Arc::new(AtomicUsize::new(0));
    let retained = count.clone();
    let entered = Arc::new(Notify::new());
    let observed = entered.clone();
    let release = Arc::new(Notify::new());
    let finish = release.clone();
    let stop = Shutdown::default();
    let work = tokio::spawn(connection(
        server,
        1,
        stop.clone(),
        move |_, request| async move {
            assert!(matches!(request, Request::Mint));
            observed.notify_one();
            finish.notified().await;
            retained.fetch_add(1, Ordering::SeqCst);
            Reply::Unknown
        },
    ));
    send(&mut client, 1, &[]).await.unwrap();
    timeout(HANDSHAKE, entered.notified()).await.unwrap();
    drop(client);
    stop.cancel();
    release.notify_one();
    timeout(HANDSHAKE, work).await.unwrap().unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn owner_route_rejects_replacement_symlink_and_changed_mode() {
    // SAFETY: the UID and GID queries take no pointers.
    let (uid, gid) = unsafe { (libc::geteuid(), libc::getegid()) };
    let base = tempfile::tempdir_in("/tmp").unwrap();
    let dir = base.path().join("owner");
    std::fs::create_dir(&dir).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o2710)).unwrap();
    let path = dir.join("control");
    let route_before = route(&path, 1, uid, Some(gid)).unwrap();
    let listener = UnixListener::bind(&path).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o660)).unwrap();
    let original = socket(&path, uid).unwrap();
    std::fs::rename(&path, dir.join("saved")).unwrap();
    symlink(dir.join("saved"), &path).unwrap();
    assert!(socket(&path, uid).is_err());
    std::fs::remove_file(&path).unwrap();
    let _replacement = UnixListener::bind(&path).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o660)).unwrap();
    assert!(socket(&path, uid).unwrap() != original);
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o2770)).unwrap();
    assert!(route(&path, 1, uid, Some(gid)).is_err());
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o2710)).unwrap();
    assert!(route(&path, 1, uid, Some(gid)).is_ok_and(|new| new == route_before));
    // A named ACL with only search permission preserves the 2710 mode.
    // Inspecting mode bits alone would accept this extra principal.
    let mut acl = 2_u32.to_le_bytes().to_vec();
    for (tag, perms, id) in [
        (1_u16, 7_u16, u32::MAX),
        (2, 1, uid ^ 1),
        (4, 1, u32::MAX),
        (16, 1, u32::MAX),
        (32, 0, u32::MAX),
    ] {
        acl.extend(tag.to_le_bytes());
        acl.extend(perms.to_le_bytes());
        acl.extend(id.to_le_bytes());
    }
    let dir_name = CString::new(dir.as_os_str().as_bytes()).unwrap();
    // SAFETY: both paths are NUL-terminated and acl owns the supplied range.
    assert_eq!(
        unsafe {
            libc::setxattr(
                dir_name.as_ptr(),
                c"system.posix_acl_access".as_ptr(),
                acl.as_ptr().cast(),
                acl.len(),
                0,
            )
        },
        0
    );
    assert_eq!(std::fs::metadata(&dir).unwrap().mode() & 0o7777, 0o2710);
    assert!(route(&path, 1, uid, Some(gid)).is_err());
    drop(listener);
}

#[tokio::test]
async fn owner_kernel_peer_rejects_wrong_daemon_even_with_connected_socket() {
    let (stream, _other) = UnixStream::pair().unwrap();
    // SAFETY: getuid takes no pointers.
    let uid = unsafe { libc::getuid() };
    assert!(peer(&stream, uid).is_ok());
    assert_eq!(peer(&stream, uid ^ 1), Err(Failure::Refused));
}

#[tokio::test]
async fn owner_hosted_disposable_uids_enforce_group_and_kernel_credentials() {
    // Ordinary unprivileged runs exercise the other socket tests. This preflight
    // is scoped to the explicitly granted disposable hosted Ubuntu worker.
    if std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
        || std::env::var("RUNNER_OS").as_deref() != Ok("Linux")
        || std::env::var("CI").as_deref() != Ok("true")
        || !std::env::var("GITHUB_REPOSITORY")
            .is_ok_and(|repo| repo.eq_ignore_ascii_case("Dytschgo/arxcovo"))
    {
        return;
    }
    const OWNER: u32 = 65534; // nobody
    const AGENT: u32 = 33; // www-data
    const BUILD: u32 = 1; // daemon
    // SAFETY: geteuid takes no pointers.
    let daemon = unsafe { libc::geteuid() };
    let roles = Roles {
        owner: OWNER,
        daemon,
        agent: AGENT,
        build: BUILD,
        group: OWNER,
    };
    let base = tempfile::tempdir_in("/tmp").unwrap();
    std::fs::set_permissions(base.path(), std::fs::Permissions::from_mode(0o711)).unwrap();
    let dir = base.path().join("owner");
    std::fs::create_dir(&dir).unwrap();
    let mut group = tokio::process::Command::new("/usr/bin/sudo");
    group
        .args(["-n", "--", "/usr/bin/chgrp", "65534", "--"])
        .arg(&dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    assert!(
        timeout(HANDSHAKE, group.status())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
    let path = dir.join("control");
    // Replay the old fixture's successful chmod as the unprivileged runner.
    // The target GID belongs to the owner, so the kernel silently drops SGID.
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o2710)).unwrap();
    match Server::bind(path.clone(), roles) {
        Err(BindError::DirectoryMode(0o710)) => {
            eprintln!("old fixture bind refused at directory mode 0710 after chmod requested 2710");
        }
        Err(error) => panic!("unexpected old fixture bind rejection: {error:?}"),
        Ok(_) => panic!("old fixture unexpectedly retained setgid"),
    }
    assert!(!path.exists(), "directory policy must reject before bind");
    let mut mode = tokio::process::Command::new("/usr/bin/sudo");
    mode.args(["-n", "--", "/usr/bin/chmod", "--", "2710"])
        .arg(&dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    assert!(
        timeout(HANDSHAKE, mode.status())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
    let metadata = std::fs::symlink_metadata(&dir).unwrap();
    assert_eq!(
        (metadata.uid(), metadata.gid(), metadata.mode() & 0o7777),
        (daemon, OWNER, 0o2710)
    );
    let server = Server::bind(path.clone(), roles)
        .unwrap_or_else(|error| panic!("qualified fixture bind failed: {error:?}"));
    // A role configured to share the owner UID must fail qualification.
    assert_eq!(
        Roles {
            build: OWNER,
            ..roles
        }
        .qualify(),
        Err(Failure::Refused)
    );
    let count = Arc::new(AtomicUsize::new(0));
    let observed = count.clone();
    let mut running = server.start(move |operator, request| {
        let observed = observed.clone();
        async move {
            assert_eq!(operator.uid(), OWNER);
            assert!(matches!(request, Request::Mint));
            observed.fetch_add(1, Ordering::SeqCst);
            Reply::Unknown
        }
    });

    // No password prompt, account, membership or service changes. Each
    // subprocess switches only its own credentials on the disposable worker.
    // The fixed Python program sends only a public Mint opcode.
    const CLIENT: &str = "
import socket, sys
s = socket.socket(socket.AF_UNIX)
s.settimeout(2)
try:
    s.connect(sys.argv[1])
except PermissionError:
    sys.exit(20)
try:
    s.sendall(b'ARXO\\x01\\x00\\x00')
    s.shutdown(socket.SHUT_WR)
    reply = s.recv(32)
except ConnectionError:
    reply = b''
sys.exit(21 if reply == b'' else 0 if reply == b'ARXO\\x0e\\x00\\x00' else 22)
";
    for (uid, group, expected) in [
        (AGENT, AGENT, 20),
        (BUILD, BUILD, 20),
        (AGENT, OWNER, 21),
        (BUILD, OWNER, 21),
        (OWNER, OWNER, 0),
    ] {
        let mut client = tokio::process::Command::new("/usr/bin/sudo");
        client
            .args([
                "-n",
                "--",
                "/usr/bin/setpriv",
                "--reuid",
                &uid.to_string(),
                "--regid",
                &group.to_string(),
                "--clear-groups",
                "--no-new-privs",
                "--",
                "/usr/bin/python3",
                "-I",
                "-B",
                "-c",
                CLIENT,
            ])
            .arg(&path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let status = timeout(Duration::from_secs(4), client.status())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            status.code(),
            Some(expected),
            "disposable role exit, signal {:?}",
            status.signal()
        );
    }
    assert_eq!(running.close(HANDSHAKE).await, Ok(()));
    assert_eq!(count.load(Ordering::SeqCst), 1);
}
