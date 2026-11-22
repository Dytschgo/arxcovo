use std::io::{Read, Write};
use std::process::{ChildStdin, ChildStdout, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const GUEST: &str = env!("CARGO_BIN_EXE_arx-guest");

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
    ] {
        let output = run_guest(args, Stdio::piped(), |_, _| {});
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert_eq!(output.stderr, b"Usage: arx-guest --version | serve-stdio\n");
    }
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
