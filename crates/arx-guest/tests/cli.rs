use std::io::{Read, Write};
use std::process::{ChildStdin, ChildStdout, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const GUEST: &str = env!("CARGO_BIN_EXE_arx-guest");
#[cfg(target_os = "linux")]
const USAGE: &[u8] = b"Usage: arx-guest --version | serve-stdio | serve-virtio\n";
#[cfg(windows)]
const USAGE: &[u8] = b"Usage: arx-guest --version | serve-stdio | serve-service\n";
#[cfg(not(any(target_os = "linux", windows)))]
const USAGE: &[u8] = b"Usage: arx-guest --version | serve-stdio\n";

fn run_guest(
    args: &[&str],
    stderr: Stdio,
    exchange: impl FnOnce(ChildStdin, &mut ChildStdout) + Send + 'static,
) -> Output {
    fn drain(stream: impl Read) -> Vec<u8> {
        let mut bytes = Vec::new();
        stream.take(1024).read_to_end(&mut bytes).unwrap();
        assert!(bytes.len() < 1024, "unexpectedly large CLI output");
        bytes
    }

    let mut child = Command::new(GUEST)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(stderr)
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let input = child.stdin.take().unwrap();
    let mut output = child.stdout.take().unwrap();
    let errors = child.stderr.take();
    let stdout = thread::spawn(move || {
        exchange(input, &mut output);
        drain(output)
    });
    let stderr = thread::spawn(move || errors.map(drain).unwrap_or_default());

    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            child.wait().unwrap();
            let _ = stdout.join();
            let _ = stderr.join();
            panic!("guest process did not exit before the deadline");
        }
        thread::sleep(Duration::from_millis(10));
    };
    Output {
        status,
        stdout: stdout.join().unwrap(),
        stderr: stderr.join().unwrap(),
    }
}

#[test]
fn version_exits_successfully() {
    let output = run_guest(&["--version"], Stdio::piped(), |_, _| {});
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        concat!("arx-guest ", env!("CARGO_PKG_VERSION"), "\n")
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn bare_unknown_and_extra_arguments_fail_with_fixed_usage() {
    for args in [
        &[][..],
        &["unknown"],
        &["--version", "unexpected"],
        &["serve-stdio", "unexpected"],
        &["serve-virtio", "/dev/null"],
        &["serve-service", "unexpected"],
        #[cfg(not(windows))]
        &["serve-service"],
        #[cfg(not(target_os = "linux"))]
        &["serve-virtio"],
    ] {
        let output = run_guest(args, Stdio::piped(), |_, _| {});
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert_eq!(output.stderr, USAGE);
    }
}

#[cfg(windows)]
#[test]
fn service_dispatch_refuses_to_run_outside_scm() {
    let output = run_guest(&["serve-service"], Stdio::piped(), |_, _| {});
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(
        output.stderr,
        b"arx-guest: service dispatcher unavailable\n"
    );
}

#[test]
fn stdio_round_trip_flushes_each_response_and_exits_on_eof() {
    let output = run_guest(&["serve-stdio"], Stdio::piped(), |mut input, output| {
        for (request, expected) in [
            (
                r#"{"type":"Hello","version":1,"challenge":"0123456789abcdef0123456789abcdef"}"#,
                r#"{"type":"HelloAck","version":1,"challenge":"0123456789abcdef0123456789abcdef","script_execution":"unavailable"}"#,
            ),
            (r#"{"type":"Ping","id":1}"#, r#"{"type":"Pong","id":1}"#),
            (
                r#"{"type":"RunScript","id":2,"script":"echo hello","timeout_s":1}"#,
                r#"{"type":"ScriptResult","id":2,"status":"Unavailable"}"#,
            ),
        ] {
            input
                .write_all(&(request.len() as u32).to_be_bytes())
                .unwrap();
            input.write_all(request.as_bytes()).unwrap();
            input.flush().unwrap();

            let mut prefix = [0; 4];
            output.read_exact(&mut prefix).unwrap();
            assert_eq!(prefix, (expected.len() as u32).to_be_bytes());
            let mut response = vec![0; expected.len()];
            output.read_exact(&mut response).unwrap();
            assert_eq!(response, expected.as_bytes());
        }
    });
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}

#[test]
fn stdio_reports_protocol_error_and_exits() {
    let output = run_guest(&["serve-stdio"], Stdio::piped(), |mut input, _| {
        input.write_all(&[0, 0]).unwrap();
    });
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(output.stderr, b"arx-guest: truncated guest frame prefix\n");
}

#[test]
fn stdio_protocol_error_redacts_script_bytes() {
    let output = run_guest(&["serve-stdio"], Stdio::piped(), |mut input, _| {
        let request =
            br#"{"type":"RunScript","id":1,"script":"arx-private-script-sentinel","timeout_s":1}"#;
        input
            .write_all(&(request.len() as u32).to_be_bytes())
            .unwrap();
        input.write_all(request).unwrap();
    });
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty());
    assert!(
        !String::from_utf8(output.stderr)
            .unwrap()
            .contains("arx-private-script-sentinel")
    );
}

#[test]
fn stdio_protocol_error_keeps_exit_code_when_stderr_is_closed() {
    let (reader, writer) = std::io::pipe().unwrap();
    drop(reader);
    let output = run_guest(&["serve-stdio"], Stdio::from(writer), |mut input, _| {
        input.write_all(&[0, 0]).unwrap();
    });
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
}

#[test]
fn stdio_exits_on_clean_eof_before_hello() {
    let output = run_guest(&["serve-stdio"], Stdio::piped(), |_, _| {});
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}

#[test]
fn ci_test_profile_keeps_runtime_checks_when_debuginfo_is_off() {
    assert!(
        std::hint::black_box(cfg!(debug_assertions)),
        "debug assertions must stay enabled in test builds"
    );
    let overflowed =
        std::panic::catch_unwind(|| std::hint::black_box(u8::MAX) + std::hint::black_box(1u8));
    assert!(
        overflowed.is_err(),
        "overflow checks and unwinding panics must stay enabled"
    );
    assert!(
        std::panic::Location::caller().file() == file!(),
        "compiled caller location smoke check"
    );
}

#[test]
fn stdio_guest_refuses_the_compose_handshake_like_a_legacy_guest() {
    let output = run_guest(&["serve-stdio"], Stdio::piped(), |mut input, _| {
        let request = br#"{"type":"ComposeHello","version":3,"challenge":"0123456789abcdef0123456789abcdef","generation":7,"capabilities":["backup","lifecycle","logs","manifest","secrets","status"]}"#;
        input
            .write_all(&(request.len() as u32).to_be_bytes())
            .unwrap();
        input.write_all(request).unwrap();
    });
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(output.stderr, b"arx-guest: invalid guest frame JSON\n");
}

#[test]
fn enrolled_host_and_range_headers_reach_each_fixed_upstream() {
    use arx_guest::compose::routing::{RouteTarget, RoutingTable, SLIRP_FORWARD_PEER};

    let table = RoutingTable::new(
        "arx-fedcba9876543210fedcba9876543210.invalid",
        Some("example.invalid".into()),
        SLIRP_FORWARD_PEER,
        vec![
            RouteTarget {
                hostname: "a.example.invalid".into(),
                project_id: "calm-life".into(),
                service: "web".into(),
                project_slot: 0,
                service_index: 0,
                route_slot: 0,
            },
            RouteTarget {
                hostname: "b.example.invalid".into(),
                project_id: "second-app".into(),
                service: "web".into(),
                project_slot: 1,
                service_index: 0,
                route_slot: 1,
            },
        ],
    )
    .unwrap();
    assert_eq!(table.routes()[0].loopback_port(), 20000);
    assert_eq!(table.routes()[1].loopback_port(), 20004);
    let config = table.render_nginx_config();
    // Evaluate the emitted map rule. Python's re supports this PCRE subset.
    let map = config
        .split_once("map $http_cf_connecting_ip $arx_cf_ip {\n")
        .unwrap()
        .1
        .split_once("\n  }")
        .unwrap()
        .0;
    assert!(map.contains("default \"\";"));
    let rule = map
        .lines()
        .find_map(|line| {
            line.trim()
                .strip_prefix("\"~")
                .and_then(|pattern| pattern.strip_suffix("\" $remote_addr;"))
        })
        .unwrap();
    let python = if cfg!(windows) { "python" } else { "python3" };
    let checked = Command::new(python)
        .args([
            "-I",
            "-B",
            "-c",
            r#"
import ipaddress, re, sys
pattern = re.compile(sys.argv[1])
cases = ["", " 1.2.3.4", "1.2.3.4 ", "1.2.3.4,::1", "1.2.3.4:80",
         "001.2.3.4", "::1%lo", "[::1]", "a:::b", "10000::", "::ffff:256.0.0.1"]
cases += [f"192.0.2.{n}" for n in range(258)]
for left in range(9):
    for right in range(9):
        cases.append(":".join(["abcd"] * left) + "::" + ":".join(["1"] * right))
cases += ["::ffff:192.0.2.1", "2001:db8:0:0:0:0:0:1", "255.255.255.255"]
for value in cases:
    try:
        ipaddress.ip_address(value)
        valid = "%" not in value
    except ValueError:
        valid = False
    assert bool(pattern.search(value)) == valid, value
"#,
            rule,
        ])
        .status()
        .expect("Python 3 for nginx IP map test");
    assert!(checked.success());
    for route in table.routes() {
        let server = config
            .split(&format!("server_name {};", route.hostname))
            .nth(1)
            .unwrap()
            .split("  server {")
            .next()
            .unwrap();
        assert!(server.contains(&format!("if ($http_host != {})", route.hostname)));
        assert!(server.contains(&format!(
            "proxy_pass http://127.0.0.1:{};",
            route.loopback_port()
        )));
        for header in [
            "CF-Connecting-IP",
            "CF-Connecting-IPv6",
            "True-Client-IP",
            "X-Client-IP",
            "X-Original-Forwarded-For",
            "X-Forwarded-Port",
            "X-Cluster-Client-IP",
            "Forwarded",
        ] {
            assert_eq!(
                server
                    .matches(&format!("proxy_set_header {header} \"\";"))
                    .count(),
                3
            );
        }
        assert_eq!(
            server
                .matches("proxy_set_header Range $http_range;")
                .count(),
            3
        );
        assert_eq!(
            server
                .matches("proxy_set_header If-Range $http_if_range;")
                .count(),
            3
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn backup_sqlite_helper_subcommand_refuses_bare_invocation_and_runs_isolated_copy() {
    use std::fs::{self, File, OpenOptions};
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::os::unix::fs::OpenOptionsExt;
    use std::os::unix::process::CommandExt;

    use arx_guest::compose_backup::{
        LINUX_HELPER_CONFIG_FD, LINUX_HELPER_DST_STAT_FD, LINUX_HELPER_RESULT_FD,
        LINUX_HELPER_VOL_FD, LinuxBackupHelperConfig, LinuxBackupHelperReply, MAX_BACKUP_BYTES,
        verify_portable_sqlite_file,
    };

    // Bare invocation without inherited descriptors exits non-zero cleanly.
    let bare = Command::new(GUEST)
        .arg("backup-sqlite-helper")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    assert_eq!(bare.status.code(), Some(1));
    assert!(bare.stdout.is_empty());

    // End-to-end invocation of the real binary subcommand with inherited fds.
    let raw = std::env::temp_dir().join(format!("arx-guest-cli-helper-{}", std::process::id()));
    let _ = fs::remove_dir_all(&raw);
    fs::create_dir_all(&raw).unwrap();
    let root = fs::canonicalize(&raw).unwrap();
    let vol_data = root.join("vol_data");
    let staging = root.join("staging");
    fs::create_dir_all(&vol_data).unwrap();
    fs::create_dir_all(&staging).unwrap();

    let db_path = vol_data.join("app.sqlite3");
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA wal_autocheckpoint=0;
         CREATE TABLE t(id INTEGER PRIMARY KEY, v TEXT NOT NULL);
         INSERT INTO t(v) VALUES ('cli_helper_wal');",
    )
    .unwrap();

    let dst_path = staging.join("out.sqlite3");
    let dst_file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&dst_path)
        .unwrap();
    let vol_dir_file = File::open(&vol_data).unwrap();

    let uid = unsafe { libc::geteuid() };
    let gid = unsafe { libc::getegid() };
    let cfg = LinuxBackupHelperConfig {
        database: "app.sqlite3".to_owned(),
        dst_path: dst_path.to_str().unwrap().to_owned(),
        vol_path: Some(vol_data.to_str().unwrap().to_owned()),
        max_bytes: MAX_BACKUP_BYTES,
        deadline_ms: 15_000,
        uid,
        gid,
        isolate_root: false,
    };
    let (cfg_read, mut cfg_write) = std::io::pipe().unwrap();
    let (mut res_read, res_write) = std::io::pipe().unwrap();
    cfg_write
        .write_all(&serde_json::to_vec(&cfg).unwrap())
        .unwrap();
    drop(cfg_write);

    let dup_high = |fd: i32| -> OwnedFd {
        let d = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 10) };
        assert!(d >= 10);
        unsafe { OwnedFd::from_raw_fd(d) }
    };
    let vol_h = dup_high(vol_dir_file.as_raw_fd());
    let dst_h = dup_high(dst_file.as_raw_fd());
    let cfg_h = dup_high(cfg_read.as_raw_fd());
    let res_h = dup_high(res_write.as_raw_fd());
    let (v_raw, d_raw, c_raw, r_raw) = (
        vol_h.as_raw_fd(),
        dst_h.as_raw_fd(),
        cfg_h.as_raw_fd(),
        res_h.as_raw_fd(),
    );

    let mut cmd = Command::new(GUEST);
    cmd.arg("backup-sqlite-helper")
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    unsafe {
        cmd.pre_exec(move || {
            for (src, dst) in [
                (v_raw, LINUX_HELPER_VOL_FD),
                (d_raw, LINUX_HELPER_DST_STAT_FD),
                (c_raw, LINUX_HELPER_CONFIG_FD),
                (r_raw, LINUX_HELPER_RESULT_FD),
            ] {
                if libc::dup2(src, dst) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            Ok(())
        });
    }
    let mut child = cmd.spawn().unwrap();
    drop(vol_h);
    drop(dst_h);
    drop(cfg_h);
    drop(res_h);
    drop(cfg_read);
    drop(res_write);
    let status = child.wait().unwrap();
    assert!(status.success());

    let mut reply_buf = Vec::new();
    res_read.read_to_end(&mut reply_buf).unwrap();
    let reply: LinuxBackupHelperReply = serde_json::from_slice(&reply_buf).unwrap();
    assert!(reply.ok, "{reply:?}");

    // Normalize journal_mode=DELETE as parent does and verify portable SQLite output.
    {
        let c = rusqlite::Connection::open(&dst_path).unwrap();
        c.execute_batch("PRAGMA journal_mode=DELETE;").unwrap();
    }
    let meta = verify_portable_sqlite_file(&dst_path, MAX_BACKUP_BYTES).unwrap();
    assert!(meta.total_bytes >= 4096);
    let check_conn = rusqlite::Connection::open(&dst_path).unwrap();
    let v: String = check_conn
        .query_row("SELECT v FROM t WHERE id=1", [], |r| r.get(0))
        .unwrap();
    assert_eq!(v, "cli_helper_wal");
    drop(check_conn);
    drop(conn);
    let _ = fs::remove_dir_all(&root);
}
