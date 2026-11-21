use super::*;
use sha2::{Digest, Sha256};
use std::{
    os::{fd::FromRawFd, unix::process::CommandExt},
    process::{Command, Stdio},
    time::Instant,
};

const CASE: &str =
    "service::owner_cli::cluster_control::tests::owner_private_pty_full_paste_and_refusals";
const CHILD: &str = "ARX_TEST_OWNER_PTY_CHILD";

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
        run_pty(case);
    }
}

fn run_pty(case: &str) {
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
    let attached = case != "no-tty";
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", CASE])
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
    let mut child = command.spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(6);
    let paste = valid_paste();
    assert!(paste.bytes().len() > 4096);
    if !matches!(case, "display" | "no-tty") {
        while !noecho(&slave) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(noecho(&slave), "terminal read mode unavailable");
        match case {
            "valid" => {
                for bytes in paste.bytes().chunks(1024) {
                    master.write_all(bytes).unwrap();
                    std::thread::sleep(Duration::from_millis(10));
                }
                master.write_all(b"\n").unwrap();
            }
            "overflow" => {
                for _ in 0..9 {
                    master.write_all(&[b'a'; 1024]).unwrap();
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
            "extra" | "truncated" => {
                let bytes = paste.bytes();
                let end = bytes.len() - 8;
                for part in bytes[..end].chunks(1024) {
                    master.write_all(part).unwrap();
                    std::thread::sleep(Duration::from_millis(10));
                }
                if case == "extra" {
                    let mut last = [0; 10];
                    last[..8].copy_from_slice(&bytes[end..]);
                    last[8..].copy_from_slice(b"\nx");
                    master.write_all(&last).unwrap();
                } else {
                    master.write_all(b"\n").unwrap();
                }
            }
            "signal" => {
                // SAFETY: the still-owned child PID names the exact fixture.
                assert_eq!(unsafe { libc::kill(child.id() as i32, libc::SIGTERM) }, 0);
            }
            _ => unreachable!(),
        }
    }
    // SAFETY: set the test master nonblocking for bounded, in-memory observation.
    assert_eq!(
        unsafe { libc::fcntl(mfd, libc::F_SETFL, libc::O_NONBLOCK) },
        0
    );
    let mut observed = Paste::empty();
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
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("private terminal fixture deadline");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "private terminal fixture failed");
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
