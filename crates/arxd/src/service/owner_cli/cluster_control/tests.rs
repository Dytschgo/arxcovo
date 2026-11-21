use super::*;
use sha2::{Digest, Sha256};
use std::{
    os::{
        fd::FromRawFd,
        unix::{
            fs::{MetadataExt, PermissionsExt},
            net::UnixListener,
            process::CommandExt,
        },
    },
    process::{Command, Stdio},
    time::Instant,
};
use tokio::io::AsyncReadExt;

const CASE: &str =
    "service::owner_cli::cluster_control::tests::owner_private_pty_full_paste_and_refusals";
const CHILD: &str = "ARX_TEST_OWNER_PTY_CHILD";
const CLI_CASE: &str = "service::owner_cli::cluster_control::tests::owner_cli_production_pty_dispatch_and_daemon_credentials";
const CLI_SOCKET: &str = "ARX_TEST_OWNER_PTY_SOCKET";
const CLI_DAEMON: &str = "ARX_TEST_OWNER_PTY_DAEMON";

#[test]
fn owner_control_parser_is_explicit_and_closed() {
    let parse_args = |verb: &str, path: &str, uid: &str| {
        parse(
            &[
                "service",
                "cluster-control",
                verb,
                "--socket",
                path,
                "--daemon-uid",
                uid,
            ]
            .map(OsString::from),
        )
    };
    assert!(matches!(
        parse_args("mint", "/run/arx/control", "123"),
        Some((Action::Mint, _, 123))
    ));
    assert!(matches!(
        parse_args("join", "/run/arx/control", "123"),
        Some((Action::Join, _, 123))
    ));
    assert!(parse_args("remove", "/run/arx/control", "123").is_none());
    for bad in [
        "arx/control",
        "/run//arx/control",
        "/run/../control",
        "/run/control\n",
    ] {
        assert!(parse_args("mint", bad, "123").is_none());
    }
    for bad in ["0", "4294967295", "4294967296", "+123", "owner"] {
        assert!(parse_args("mint", "/run/arx/control", bad).is_none());
    }
}

fn valid_paste() -> Paste {
    // Independent ADR0040 wire construction with a large root field. This is a
    // synthetic parser fixture kept only in memory.
    let root = [0xa0; 3000];
    let mut inner = [3; 132];
    inner[32..64].fill(6);
    inner[64..96].copy_from_slice(&Sha256::digest(root));
    let check = Sha256::new()
        .chain_update(b"arxcovo/join/check/v2")
        .chain_update(&inner[..128])
        .finalize();
    inner[128..].copy_from_slice(&check[..4]);
    let mut body = vec![4, 192, 168, 1, 42, 0x23, 0x45];
    body.extend((root.len() as u16).to_be_bytes());
    body.extend(root);
    body.extend(format!("arxj2-{}", hex::encode(inner)).bytes());
    let check = Sha256::new()
        .chain_update(b"arxcovo/join/transfer/v1")
        .chain_update(&body)
        .finalize();
    body.extend(&check[..4]);
    Paste::copy(format!("arxje1-{}", hex::encode(body)).as_bytes()).unwrap()
}

fn mode(file: &File) -> libc::termios {
    let mut mode = std::mem::MaybeUninit::uninit();
    // SAFETY: the descriptor is owned; tcgetattr writes one termios.
    assert_eq!(
        unsafe { libc::tcgetattr(file.as_raw_fd(), mode.as_mut_ptr()) },
        0
    );
    // SAFETY: tcgetattr succeeded.
    unsafe { mode.assume_init() }
}

fn noecho(file: &File) -> bool {
    mode(file).c_lflag & (libc::ECHO | libc::ICANON) == 0
}

fn child_case(case: &str) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        if case == "no-tty" {
            let result = control(Action::Mint, Path::new("/run/arx/control"), 20001).await;
            assert_eq!(result, Err(Failure::Refused));
            return;
        }
        let mut signals = Signals::new().unwrap();
        let tty = Terminal::open().unwrap();
        if case == "display" {
            tty.display(&valid_paste()).await.unwrap();
            return;
        }
        let outcome = tokio::select! {
            biased;
            _ = signals.interrupted() => None,
            result = timeout(Duration::from_secs(4), tty.read_paste()) => Some(result.unwrap()),
        };
        match case {
            "valid" => assert!(outcome.unwrap().unwrap().bytes() == valid_paste().bytes()),
            "signal" => assert!(outcome.is_none()),
            "overflow" | "extra" | "truncated" => assert!(outcome.unwrap().is_err()),
            _ => panic!("unknown fixture"),
        }
    });
}

#[test]
fn owner_private_pty_full_paste_and_refusals() {
    if let Ok(case) = std::env::var(CHILD) {
        child_case(&case);
        return;
    }
    for case in [
        "valid",
        "overflow",
        "extra",
        "truncated",
        "signal",
        "display",
        "no-tty",
    ] {
        run_pty(case, None);
    }
}

fn run_pty(case: &str, target: Option<(&Path, u32)>) {
    let mut master = -1;
    let mut slave = -1;
    // SAFETY: openpty returns two fresh descriptors; optional outputs are omitted.
    assert_eq!(
        unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
            )
        },
        0
    );
    // SAFETY: successful openpty transferred these distinct descriptors to us.
    let (mut master, slave) = unsafe { (File::from_raw_fd(master), File::from_raw_fd(slave)) };
    let original = mode(&slave);
    let mfd = master.as_raw_fd();
    let sfd = slave.as_raw_fd();
    nonblocking(&master);
    let attached = case != "no-tty";
    let mut command = Command::new(std::env::current_exe().unwrap());
    let selector = if let Some((path, daemon)) = target {
        command
            .env(CLI_SOCKET, path)
            .env(CLI_DAEMON, daemon.to_string());
        CLI_CASE
    } else {
        CASE
    };
    command
        .args(["--exact", selector])
        .env(CHILD, case)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // SAFETY: the post-fork closure calls only descriptor and session syscalls.
    unsafe {
        command.pre_exec(move || {
            libc::close(mfd);
            if libc::setsid() < 0 || (attached && libc::ioctl(sfd, libc::TIOCSCTTY, 0) < 0) {
                return Err(io::Error::last_os_error());
            }
            libc::close(sfd);
            Ok(())
        });
    }
    if case == "backpressure" {
        saturate_output(&slave);
    }
    let mut child = command.spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(6);
    let paste = valid_paste();
    assert!(paste.bytes().len() > 4096);
    if !matches!(case, "display" | "no-tty" | "wrong-daemon") {
        while !noecho(&slave) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        if !noecho(&slave) {
            expire(&mut child);
        }
        match case {
            "valid" => {
                for bytes in paste.bytes().chunks(1024) {
                    send_input(&master, bytes, deadline, &mut child);
                    std::thread::sleep(Duration::from_millis(10));
                }
                send_input(&master, b"\n", deadline, &mut child);
            }
            "overflow" => {
                for _ in 0..9 {
                    send_input(&master, &[b'a'; 1024], deadline, &mut child);
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
            "extra" | "truncated" => {
                let bytes = paste.bytes();
                let end = bytes.len() - 8;
                for part in bytes[..end].chunks(1024) {
                    send_input(&master, part, deadline, &mut child);
                    std::thread::sleep(Duration::from_millis(10));
                }
                if case == "extra" {
                    let mut last = [0; 10];
                    last[..8].copy_from_slice(&bytes[end..]);
                    last[8..].copy_from_slice(b"\nx");
                    send_input(&master, &last, deadline, &mut child);
                } else {
                    send_input(&master, b"\n", deadline, &mut child);
                }
            }
            "backpressure" => send_input(&master, b"invalid\n", deadline, &mut child),
            "signal" => {
                // SAFETY: the still-owned child PID names the exact fixture.
                assert_eq!(unsafe { libc::kill(child.id() as i32, libc::SIGTERM) }, 0);
            }
            _ => unreachable!(),
        }
    }
    let mut observed = Paste::empty();
    if case == "backpressure" {
        // Keep the master unread until the child has completed restoration.
        while child.try_wait().unwrap().is_none() {
            if Instant::now() >= deadline {
                expire(&mut child);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    } else {
        let mut chunk = InputChunk([0; 1024]);
        loop {
            match master.read(&mut chunk.0) {
                Ok(n) if n != 0 => assert!(observed.extend(&chunk.0[..n])),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    if child.try_wait().unwrap().is_some() {
                        break;
                    }
                }
                _ => panic!("invalid private terminal observation"),
            }
            if Instant::now() >= deadline {
                expire(&mut child);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "private terminal fixture {case} failed with exit {:?}",
        output.status.code()
    );
    assert!(!output.stdout.windows(7).any(|b| b == b"arxje1-"));
    assert!(!output.stderr.windows(7).any(|b| b == b"arxje1-"));
    if case == "display" {
        assert!(observed.bytes().starts_with(paste.bytes()));
    } else {
        assert!(observed.bytes().is_empty(), "terminal leaked bytes");
    }
    let restored = mode(&slave);
    assert_eq!(restored.c_iflag, original.c_iflag);
    assert_eq!(restored.c_oflag, original.c_oflag);
    assert_eq!(restored.c_cflag, original.c_cflag);
    assert_eq!(restored.c_lflag, original.c_lflag);
    assert_eq!(restored.c_cc, original.c_cc);
}

fn expire(child: &mut std::process::Child) -> ! {
    let _ = child.kill();
    let _ = child.wait();
    panic!("private terminal fixture deadline");
}

fn nonblocking(file: &File) {
    // SAFETY: the owned terminal descriptor accepts these flag operations.
    assert_eq!(
        unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK) },
        0
    );
}

fn send_input(
    mut master: &File,
    mut bytes: &[u8],
    deadline: Instant,
    child: &mut std::process::Child,
) {
    while !bytes.is_empty() {
        if Instant::now() >= deadline {
            expire(child);
        }
        match master.write(bytes) {
            Ok(size) if size != 0 => bytes = &bytes[size..],
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(5));
            }
            _ => expire(child),
        }
    }
}

fn saturate_output(mut slave: &File) {
    nonblocking(slave);
    for _ in 0..256 {
        match slave.write(&[b'#'; 512]) {
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return,
            Ok(size) if size != 0 => {}
            _ => panic!("unexpected PTY output state"),
        }
    }
    panic!("PTY output queue did not saturate");
}

fn hosted_ubuntu() -> bool {
    std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true")
        && std::env::var("RUNNER_OS").as_deref() == Ok("Linux")
        && std::env::var("CI").as_deref() == Ok("true")
        && std::env::var("GITHUB_REPOSITORY")
            .is_ok_and(|repo| repo.eq_ignore_ascii_case("Dytschgo/arxcovo"))
}

fn cli_child(case: &str) {
    let path = std::env::var_os(CLI_SOCKET).unwrap();
    let uid = std::env::var_os(CLI_DAEMON).unwrap();
    let verb = if matches!(case, "wrong-daemon" | "no-tty") {
        "mint"
    } else {
        "join"
    };
    let args = [
        "service".into(),
        "cluster-control".into(),
        verb.into(),
        "--socket".into(),
        path,
        "--daemon-uid".into(),
        uid,
    ];
    let actual = super::super::main(&args);
    assert_eq!(actual, if case == "valid" { 0 } else { 6 });
}

async fn provision(program: &str, args: &[&str], paths: &[&Path]) {
    let mut command = tokio::process::Command::new("/usr/bin/sudo");
    command
        .args(["-n", "--", program])
        .args(args)
        .args(paths)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    assert!(
        timeout(Duration::from_secs(2), command.status())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
}

// This separate UID owns the checked endpoint. Only one actual Join frame
// may reach its synthetic dispatcher, and it verifies the exact fixture bytes.
const SYNTHETIC_DAEMON: &str = r"
import hashlib
import os
import select
import socket
import struct
import sys

def fixture():
    def check(label, data):
        return hashlib.sha256(label + data).digest()[:4]
    root = bytes([160]) * 3000
    inner = bytes([3]) * 32 + bytes([6]) * 32 + hashlib.sha256(root).digest() + bytes([3]) * 32
    inner += check(b'arxcovo/join/check/v2', inner)
    body = bytes.fromhex('04c0a8012a2345') + len(root).to_bytes(2, 'big') + root
    body += b'arxj2-' + inner.hex().encode('ascii')
    body += check(b'arxcovo/join/transfer/v1', body)
    return b'arxje1-' + body.hex().encode('ascii')

path, wrong, owner = sys.argv[1:]
try:
    with socket.socket(socket.AF_UNIX) as server:
        server.bind(path)
        os.chmod(path, 0o660)
        server.listen(4)
        sys.stdout.buffer.write(b'R')
        sys.stdout.buffer.flush()
        calls = 0
        while True:
            ready, _, _ = select.select([server, sys.stdin], [], [], 20)
            assert ready
            if server in ready:
                conn, _ = server.accept()
                with conn:
                    conn.settimeout(2)
                    assert struct.unpack('3i', conn.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12))[1] == int(owner)
                    with conn.makefile('rb') as source:
                        header = source.read(7)
                        assert header[:5] == b'ARXO\x02'
                        payload = fixture()
                        assert int.from_bytes(header[5:], 'big') == len(payload)
                        assert source.read(len(payload)) == payload and source.read(1) == b''
                    calls += 1
                    conn.sendall(b'ARXO\x0c\x00\x00')
                    conn.shutdown(socket.SHUT_WR)
            if sys.stdin in ready:
                assert sys.stdin.buffer.read(1) == b''
                assert calls == 1
                break
finally:
    for entry in (path, wrong):
        try:
            os.unlink(entry)
        except FileNotFoundError:
            pass
";

#[test]
fn owner_cli_production_pty_dispatch_and_daemon_credentials() {
    if let Ok(case) = std::env::var(CHILD) {
        cli_child(&case);
        return;
    }
    if hosted_ubuntu() {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(production_pty());
    }
}

async fn production_pty() {
    const DAEMON: u32 = 65534; // nobody
    // SAFETY: identity queries take no pointers.
    let (owner, group) = unsafe { (libc::geteuid(), libc::getegid()) };
    assert!(owner != 0 && owner != DAEMON && group != 0);
    let base = tempfile::tempdir_in("/tmp").unwrap();
    std::fs::set_permissions(base.path(), std::fs::Permissions::from_mode(0o711)).unwrap();
    let dir = base.path().join("owner");
    std::fs::create_dir(&dir).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = dir.join("control");
    let wrong = dir.join("wrong");
    let impostor = UnixListener::bind(&wrong).unwrap();
    impostor.set_nonblocking(true).unwrap();
    std::fs::set_permissions(&wrong, std::fs::Permissions::from_mode(0o660)).unwrap();

    // Match trusted filesystem metadata on both paths. The impostor's
    // listening descriptor keeps the runner UID recorded at listen time.
    provision(
        "/usr/bin/chown",
        &["--", &format!("{DAEMON}:{group}")],
        &[&dir, &wrong],
    )
    .await;
    provision("/usr/bin/chmod", &["--", "2710"], &[&dir]).await;
    let metadata = std::fs::symlink_metadata(&dir).unwrap();
    assert_eq!(
        (metadata.uid(), metadata.gid(), metadata.mode() & 0o7777),
        (DAEMON, group, 0o2710)
    );
    let metadata = std::fs::symlink_metadata(&wrong).unwrap();
    assert_eq!(
        (metadata.uid(), metadata.gid(), metadata.mode() & 0o7777),
        (DAEMON, group, 0o660)
    );

    let mut daemon = tokio::process::Command::new("/usr/bin/sudo")
        .args([
            "-n",
            "--",
            "/usr/bin/setpriv",
            "--reuid",
            "65534",
            "--regid",
            &group.to_string(),
            "--clear-groups",
            "--no-new-privs",
            "--",
            "/usr/bin/python3",
            "-I",
            "-B",
            "-c",
            SYNTHETIC_DAEMON,
        ])
        .arg(&path)
        .arg(&wrong)
        .arg(owner.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut ready = [0];
    timeout(
        Duration::from_secs(4),
        daemon.stdout.as_mut().unwrap().read_exact(&mut ready),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(ready, *b"R");
    for case in [
        "valid",
        "overflow",
        "extra",
        "truncated",
        "signal",
        "backpressure",
        "no-tty",
    ] {
        run_pty(case, Some((&path, DAEMON)));
    }
    run_pty("wrong-daemon", Some((&wrong, DAEMON)));
    let (mut accepted, _) = impostor
        .accept()
        .expect("the metadata-qualified socket was reachable");
    accepted
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    let mut received = [0; 1];
    assert_eq!(
        accepted.read(&mut received).unwrap(),
        0,
        "unexpected request to wrong daemon"
    );

    drop(daemon.stdin.take());
    assert!(
        timeout(Duration::from_secs(4), daemon.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
    std::fs::remove_dir(&dir).unwrap();
}
