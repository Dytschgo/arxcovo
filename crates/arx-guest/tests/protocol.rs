use std::io::{self, Cursor, Read};

use arx_guest::{
    MAX_FRAME, MAX_SCRIPT, MAX_TIMEOUT_S, MIN_TIMEOUT_S, PROTOCOL_VERSION, ProtocolError, Request,
    Response, ScriptExecution, ScriptExecutor, ScriptOutcome, ScriptStatus, read_frame, serve,
    write_frame,
};

const CHALLENGE: &str = "0123456789abcdef0123456789abcdef";
const HELLO: &[u8] =
    br#"{"type":"Hello","version":1,"challenge":"0123456789abcdef0123456789abcdef"}"#;
const WRONG_VERSION_HELLO: &[u8] =
    br#"{"type":"Hello","version":2,"challenge":"0123456789abcdef0123456789abcdef"}"#;
const PING: &[u8] = br#"{"type":"Ping","id":1}"#;

fn frame(payload: &[u8]) -> Vec<u8> {
    let mut bytes = (payload.len() as u32).to_be_bytes().to_vec();
    bytes.extend_from_slice(payload);
    bytes
}

fn run_script(id: u32, script: &str, timeout_s: u32) -> Request {
    Request::RunScript {
        id,
        script: script.to_owned(),
        timeout_s,
    }
}

struct NoScripts;

impl ScriptExecutor for NoScripts {
    fn script_execution(&self) -> ScriptExecution {
        ScriptExecution::Unavailable
    }

    fn run_script(&mut self, _script: &str, _timeout_s: u32) -> ScriptOutcome {
        panic!("the session dispatched an unexpected script");
    }
}

struct OneByte<R>(R);

impl<R: Read> Read for OneByte<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let len = buf.len().min(1);
        self.0.read(&mut buf[..len])
    }
}

#[test]
fn reads_fragmented_frames_and_clean_eof() {
    let mut bytes = frame(HELLO);
    bytes.extend(frame(PING));
    let mut reader = OneByte(bytes.as_slice());
    assert!(matches!(
        read_frame(&mut reader),
        Ok(Some(Request::Hello { version: PROTOCOL_VERSION, challenge })) if challenge == CHALLENGE
    ));
    assert!(matches!(
        read_frame(&mut reader),
        Ok(Some(Request::Ping { id: 1 }))
    ));
    assert!(matches!(read_frame::<Request>(&mut reader), Ok(None)));
}

#[test]
fn hello_and_ping_encode_to_golden_bytes() {
    for (request, expected) in [
        (
            Request::Hello {
                version: PROTOCOL_VERSION,
                challenge: CHALLENGE.to_owned(),
            },
            HELLO,
        ),
        (Request::Ping { id: 1 }, PING),
    ] {
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &request).unwrap();
        assert_eq!(bytes, frame(expected));
    }
}

#[test]
fn preserves_transport_reported_unexpected_eof() {
    struct UnexpectedEof;

    impl Read for UnexpectedEof {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::ErrorKind::UnexpectedEof.into())
        }
    }

    let ping = frame(PING);
    for received in [0, 2, 5] {
        let mut reader = (&ping[..received]).chain(UnexpectedEof);
        assert!(matches!(
            read_frame::<Request>(&mut reader),
            Err(ProtocolError::Io(io::ErrorKind::UnexpectedEof))
        ));
    }
}

#[test]
fn retries_an_interrupted_read_at_a_frame_boundary() {
    struct InterruptedOnce<'a> {
        remaining: &'a [u8],
        interrupted: bool,
    }

    impl Read for InterruptedOnce<'_> {
        fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
            if !self.interrupted {
                self.interrupted = true;
                return Err(io::ErrorKind::Interrupted.into());
            }
            self.remaining.read(bytes)
        }
    }

    let ping = frame(PING);
    let mut reader = InterruptedOnce {
        remaining: &ping,
        interrupted: false,
    };
    assert!(matches!(
        read_frame(&mut reader),
        Ok(Some(Request::Ping { id: 1 }))
    ));
}

#[test]
fn distinguishes_truncated_prefix_from_truncated_payload() {
    let bytes = frame(PING);
    for len in 1..bytes.len() {
        let error = if len < 4 {
            ProtocolError::TruncatedPrefix
        } else {
            ProtocolError::TruncatedPayload
        };
        assert!(matches!(
            read_frame::<Request>(&mut &bytes[..len]),
            Err(err) if err == error
        ));
    }
}

#[test]
fn rejects_invalid_lengths_before_reading_payload() {
    struct PrefixOnly<'a>(&'a [u8]);

    impl Read for PrefixOnly<'_> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            assert!(!self.0.is_empty(), "read attempted past the prefix");
            self.0.read(buf)
        }
    }

    assert_eq!(MAX_FRAME, 32_768);
    for len in [0, 32_769, u32::MAX] {
        let prefix = len.to_be_bytes();
        let mut reader = PrefixOnly(&prefix);
        assert!(matches!(
            read_frame::<Request>(&mut reader),
            Err(ProtocolError::InvalidFrameLength)
        ));
        assert!(reader.0.is_empty());
    }
}

#[test]
fn accepts_a_frame_at_the_exact_size_limit() {
    let mut payload = PING.to_vec();
    payload.resize(MAX_FRAME, b' ');
    assert!(matches!(
        read_frame(&mut frame(&payload).as_slice()),
        Ok(Some(Request::Ping { id: 1 }))
    ));
}

#[test]
fn rejects_invalid_utf8() {
    let bytes = frame(b"{\"type\":\"RunScript\",\"id\":1,\"script\":\"\xff\",\"timeout_s\":1}");
    assert!(matches!(
        read_frame::<Request>(&mut bytes.as_slice()),
        Err(ProtocolError::InvalidUtf8)
    ));
}

#[test]
fn rejects_invalid_json_and_non_object_messages() {
    for payload in [
        &b"{"[..],
        b"[]",
        b"null",
        b"{\"type\":\"Ping\",\"id\":1}{\"type\":\"Ping\",\"id\":2}",
    ] {
        assert!(matches!(
            read_frame::<Request>(&mut frame(payload).as_slice()),
            Err(ProtocolError::InvalidJson)
        ));
    }
}

#[test]
fn rejects_unknown_missing_and_duplicate_fields_and_tags() {
    for payload in [
        &br#"{"type":"Ping","id":1,"extra":true}"#[..],
        br#"{"type":"Ping","id":1,"id":2}"#,
        br#"{"type":"Ping","type":"Ping","id":1}"#,
        br#"{"type":"Ping"}"#,
        br#"{"type":"ping","id":1}"#,
        br#"{"type":1,"id":1}"#,
        br#"{"type":"RunScript","id":1,"script":"hello","timeout_s":1.0}"#,
        br#"{"Ping":{"id":1}}"#,
        br#"{"type":"Ping","id":"1"}"#,
    ] {
        assert!(matches!(
            read_frame::<Request>(&mut frame(payload).as_slice()),
            Err(ProtocolError::InvalidJson)
        ));
    }
    for payload in [
        &br#"{"type":"Pong","id":1,"extra":true}"#[..],
        br#"{"type":"ScriptResult","id":1,"status":"Unknown"}"#,
        br#"{"type":"ScriptResult","id":1,"status":{"Unavailable":null}}"#,
        br#"{"type":"ScriptResult","id":1,"status":"Unavailable","stdout":"hello"}"#,
        br#"{"type":"HelloAck","version":1,"challenge":"0123456789abcdef0123456789abcdef"}"#,
        br#"{"type":"HelloAck","version":1,"challenge":"0123456789abcdef0123456789abcdef","script_execution":"maybe"}"#,
        br#"{"type":"HelloAck","version":1,"challenge":"0123456789abcdef0123456789abcdef","script_execution":{"available":null}}"#,
    ] {
        assert!(matches!(
            read_frame::<Response>(&mut frame(payload).as_slice()),
            Err(ProtocolError::InvalidJson)
        ));
    }
}

#[test]
fn both_codecs_enforce_the_exact_wire_version() {
    let request = Request::Hello {
        version: 2,
        challenge: CHALLENGE.to_owned(),
    };
    let response = Response::HelloAck {
        version: 2,
        challenge: CHALLENGE.to_owned(),
        script_execution: ScriptExecution::Unavailable,
    };
    let mut bytes = Vec::new();
    assert_eq!(
        write_frame(&mut bytes, &request),
        Err(ProtocolError::UnsupportedVersion)
    );
    assert_eq!(
        write_frame(&mut bytes, &response),
        Err(ProtocolError::UnsupportedVersion)
    );
    assert!(bytes.is_empty());
    let ack = br#"{"type":"HelloAck","version":2,"challenge":"0123456789abcdef0123456789abcdef","script_execution":"unavailable"}"#;
    assert!(matches!(
        read_frame::<Response>(&mut frame(ack).as_slice()),
        Err(ProtocolError::UnsupportedVersion)
    ));
}

#[test]
fn requires_a_valid_hello_before_any_other_request() {
    for (request, expected) in [
        (frame(PING), ProtocolError::ExpectedHello),
        (
            frame(br#"{"type":"RunScript","id":1,"script":"echo hello","timeout_s":1}"#),
            ProtocolError::ExpectedHello,
        ),
        (
            frame(WRONG_VERSION_HELLO),
            ProtocolError::UnsupportedVersion,
        ),
    ] {
        let mut responses = Vec::new();
        assert_eq!(
            serve(request.as_slice(), &mut responses, &mut NoScripts),
            Err(expected)
        );
        assert!(responses.is_empty());
    }
}

#[test]
fn rejects_bad_challenges_without_acknowledging() {
    for challenge in [
        "0123456789ABCDEF0123456789ABCDEF",
        "0123456789abcdef0123456789abcde",
        "0123456789abcdef0123456789abcdeg",
    ] {
        let payload = format!(r#"{{"type":"Hello","version":1,"challenge":"{challenge}"}}"#);
        let mut responses = Vec::new();
        assert_eq!(
            serve(
                frame(payload.as_bytes()).as_slice(),
                &mut responses,
                &mut NoScripts
            ),
            Err(ProtocolError::InvalidChallenge)
        );
        assert!(responses.is_empty());
        let hello = Request::Hello {
            version: PROTOCOL_VERSION,
            challenge: challenge.to_owned(),
        };
        assert_eq!(
            write_frame(&mut responses, &hello),
            Err(ProtocolError::InvalidChallenge)
        );
        assert!(responses.is_empty());
    }
}

fn assert_closes_after_hello(next_frame: &[u8], expected: ProtocolError) {
    let mut bytes = frame(HELLO);
    bytes.extend_from_slice(next_frame);
    let closed_at = bytes.len() as u64;
    bytes.extend(frame(PING));
    let mut reader = Cursor::new(bytes);
    let mut responses = Vec::new();
    assert_eq!(
        serve(&mut reader, &mut responses, &mut NoScripts),
        Err(expected)
    );
    assert_eq!(reader.position(), closed_at);
    let mut output = responses.as_slice();
    assert!(matches!(
        read_frame(&mut output),
        Ok(Some(Response::HelloAck {
            version: PROTOCOL_VERSION,
            challenge,
            script_execution: ScriptExecution::Unavailable,
        })) if challenge == CHALLENGE
    ));
    assert!(output.is_empty());
}

#[test]
fn rejects_an_unsupported_version_after_hello() {
    assert_closes_after_hello(
        &frame(WRONG_VERSION_HELLO),
        ProtocolError::UnsupportedVersion,
    );
}

#[test]
fn enforces_the_frame_limit_after_hello() {
    assert_closes_after_hello(
        &(MAX_FRAME as u32 + 1).to_be_bytes(),
        ProtocolError::InvalidFrameLength,
    );
}

#[test]
fn rejects_an_unsolicited_response_after_hello() {
    assert_closes_after_hello(
        &frame(br#"{"type":"Pong","id":1}"#),
        ProtocolError::InvalidJson,
    );
}

#[test]
fn session_propagates_truncated_frames_after_the_handshake() {
    let ping = frame(PING);
    for (tail, error) in [
        (&ping[..2], ProtocolError::TruncatedPrefix),
        (&ping[..ping.len() - 1], ProtocolError::TruncatedPayload),
    ] {
        let mut requests = frame(HELLO);
        requests.extend_from_slice(tail);
        assert_eq!(
            serve(requests.as_slice(), io::sink(), &mut NoScripts),
            Err(error)
        );
    }
}

#[test]
fn rejects_zero_ids() {
    assert_closes_after_hello(
        &frame(br#"{"type":"Ping","id":0}"#),
        ProtocolError::InvalidId,
    );
    assert_closes_after_hello(
        &frame(br#"{"type":"RunScript","id":0,"script":"echo hello","timeout_s":1}"#),
        ProtocolError::InvalidId,
    );
    let mut bytes = Vec::new();
    assert_eq!(
        write_frame(&mut bytes, &Request::Ping { id: 0 }),
        Err(ProtocolError::InvalidId)
    );
    assert_eq!(
        write_frame(&mut bytes, &Response::Pong { id: 0 }),
        Err(ProtocolError::InvalidId)
    );
    assert!(bytes.is_empty());
}

#[test]
fn rejects_invalid_scripts_before_dispatch() {
    assert_eq!(MAX_SCRIPT, 4096);
    let too_long = format!("{}x", "é".repeat(2048));
    for (request, expected) in [
        (
            run_script(1, "", MIN_TIMEOUT_S),
            ProtocolError::InvalidScriptLength,
        ),
        (
            run_script(1, &too_long, MIN_TIMEOUT_S),
            ProtocolError::InvalidScriptLength,
        ),
        (
            run_script(1, "echo hello", 0),
            ProtocolError::InvalidTimeout,
        ),
        (
            run_script(1, "echo hello", 121),
            ProtocolError::InvalidTimeout,
        ),
        (run_script(1, "echo\0hello", 1), ProtocolError::EmbeddedNul),
    ] {
        let payload = serde_json::to_vec(&request).unwrap();
        assert_closes_after_hello(&frame(&payload), expected);
        let mut bytes = Vec::new();
        assert_eq!(write_frame(&mut bytes, &request), Err(expected));
        assert!(bytes.is_empty());
    }
}

#[test]
fn accepts_the_maximum_decoded_script_even_with_worst_case_json_escaping() {
    let script = "\x01".repeat(4096);
    let request = run_script(u32::MAX, &script, 120);
    let payload = format!(
        r#"{{"type":"RunScript","id":4294967295,"script":"{}","timeout_s":120}}"#,
        r"\u0001".repeat(4096)
    );
    let mut bytes = Vec::new();
    write_frame(&mut bytes, &request).unwrap();
    assert_eq!(bytes, frame(payload.as_bytes()));
    assert!(matches!(
        read_frame::<Request>(&mut bytes.as_slice()),
        Ok(Some(decoded)) if decoded == request
    ));
}

#[test]
fn validates_the_exit_code_invariant_on_read_and_write() {
    for (status, exit_code) in [
        (ScriptStatus::Exited, None),
        (ScriptStatus::Unavailable, Some(7)),
        (ScriptStatus::TimedOut, Some(7)),
        (ScriptStatus::LaunchFailed, Some(7)),
        (ScriptStatus::Terminated, Some(7)),
    ] {
        let mut bytes = Vec::new();
        let invalid = Response::ScriptResult {
            id: 1,
            status,
            exit_code,
        };
        assert_eq!(
            write_frame(&mut bytes, &invalid),
            Err(ProtocolError::InvalidExitCode)
        );
        assert!(bytes.is_empty());
        let payload = serde_json::to_vec(&invalid).unwrap();
        assert!(matches!(
            read_frame::<Response>(&mut frame(&payload).as_slice()),
            Err(ProtocolError::InvalidExitCode)
        ));
    }
    for (status, code) in [
        ("Exited", "null"),
        ("Exited", "-1"),
        ("Exited", "4294967296"),
        ("Exited", "\"0\""),
        ("Unavailable", "null"),
    ] {
        let payload =
            format!(r#"{{"type":"ScriptResult","id":1,"status":"{status}","exit_code":{code}}}"#);
        assert!(matches!(
            read_frame::<Response>(&mut frame(payload.as_bytes()).as_slice()),
            Err(ProtocolError::InvalidJson)
        ));
    }
}

#[test]
fn reports_errors_without_input_or_io_details() {
    struct BrokenReader;
    impl Read for BrokenReader {
        fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("private I/O detail"))
        }
    }

    let payload = br#"{"type":"Ping","private field":"private script text"}"#;
    let message_error = match read_frame::<Request>(&mut frame(payload).as_slice()) {
        Err(error) => error,
        Ok(_) => panic!("invalid request accepted"),
    };
    let io_error = match read_frame::<Request>(&mut BrokenReader) {
        Err(error) => error,
        Ok(_) => panic!("read unexpectedly succeeded"),
    };

    assert_eq!(message_error, ProtocolError::InvalidJson);
    assert_eq!(io_error, ProtocolError::Io(io::ErrorKind::Other));
    for error in [message_error, io_error] {
        assert!(!format!("{error:?} {error}").contains("private"));
    }
}

struct Executor<'a> {
    script: &'a str,
    calls: std::slice::Iter<'a, (u32, ScriptOutcome)>,
}

impl ScriptExecutor for Executor<'_> {
    fn script_execution(&self) -> ScriptExecution {
        ScriptExecution::Available
    }

    fn run_script(&mut self, script: &str, timeout_s: u32) -> ScriptOutcome {
        assert_eq!(script, self.script);
        let &(expected_timeout, outcome) = self.calls.next().expect("unexpected script");
        assert_eq!(timeout_s, expected_timeout);
        outcome
    }
}

#[test]
fn resets_on_hello_before_and_after_a_script() {
    let next_challenge = "11111111111111111111111111111111";
    let final_challenge = "22222222222222222222222222222222";
    let hello = |challenge: &str| Request::Hello {
        version: PROTOCOL_VERSION,
        challenge: challenge.to_owned(),
    };
    let script = "echo hello";
    let calls = [
        (MIN_TIMEOUT_S, ScriptOutcome::Exited { exit_code: 7 }),
        (MAX_TIMEOUT_S, ScriptOutcome::Exited { exit_code: 0 }),
    ];
    let mut executor = Executor {
        script,
        calls: calls.iter(),
    };
    let mut requests = Vec::new();
    for request in [
        hello(CHALLENGE),
        hello(next_challenge),
        run_script(1, script, MIN_TIMEOUT_S),
        hello(final_challenge),
        run_script(1, script, MAX_TIMEOUT_S),
    ] {
        write_frame(&mut requests, &request).unwrap();
    }
    let mut responses = Vec::new();
    serve(requests.as_slice(), &mut responses, &mut executor).unwrap();
    assert_eq!(executor.calls.len(), 0);

    let ack = |challenge: &str| Response::HelloAck {
        version: PROTOCOL_VERSION,
        challenge: challenge.to_owned(),
        script_execution: ScriptExecution::Available,
    };
    let script_result = |exit_code| Response::ScriptResult {
        id: 1,
        status: ScriptStatus::Exited,
        exit_code: Some(exit_code),
    };
    let mut output = responses.as_slice();
    for expected in [
        ack(CHALLENGE),
        ack(next_challenge),
        script_result(7),
        ack(final_challenge),
        script_result(0),
    ] {
        assert!(
            matches!(read_frame::<Response>(&mut output), Ok(Some(actual)) if actual == expected)
        );
    }
    assert!(output.is_empty());
}

#[test]
fn keeps_serving_after_many_hellos() {
    let script = "echo hello";
    let calls = [(MIN_TIMEOUT_S, ScriptOutcome::Exited { exit_code: 0 })];
    let mut executor = Executor {
        script,
        calls: calls.iter(),
    };
    let mut requests = Vec::new();
    for nonce in 0..1024 {
        write_frame(
            &mut requests,
            &Request::Hello {
                version: PROTOCOL_VERSION,
                challenge: format!("{nonce:032x}"),
            },
        )
        .unwrap();
    }
    write_frame(&mut requests, &run_script(1, script, MIN_TIMEOUT_S)).unwrap();
    serve(requests.as_slice(), io::sink(), &mut executor).unwrap();
    assert_eq!(executor.calls.len(), 0);
}

#[test]
fn dispatches_valid_scripts_and_returns_all_executor_statuses_in_order() {
    let calls = [
        (
            MIN_TIMEOUT_S,
            ScriptOutcome::Exited {
                exit_code: u32::MAX,
            },
        ),
        (MAX_TIMEOUT_S, ScriptOutcome::Unavailable),
        (MAX_TIMEOUT_S, ScriptOutcome::TimedOut),
        (MAX_TIMEOUT_S, ScriptOutcome::LaunchFailed),
        (MAX_TIMEOUT_S, ScriptOutcome::Terminated),
    ];
    let script = "é".repeat(2048);
    let mut executor = Executor {
        script: &script,
        calls: calls.iter(),
    };
    let mut requests = frame(HELLO);
    for (id, (timeout, _)) in (1..).zip(calls) {
        write_frame(&mut requests, &run_script(id, &script, timeout)).unwrap();
    }
    let mut responses = Vec::new();
    serve(requests.as_slice(), &mut responses, &mut executor).unwrap();
    assert_eq!(executor.calls.len(), 0);

    let fixtures = [
        r#"{"type":"HelloAck","version":1,"challenge":"0123456789abcdef0123456789abcdef","script_execution":"available"}"#,
        r#"{"type":"ScriptResult","id":1,"status":"Exited","exit_code":4294967295}"#,
        r#"{"type":"ScriptResult","id":2,"status":"Unavailable"}"#,
        r#"{"type":"ScriptResult","id":3,"status":"TimedOut"}"#,
        r#"{"type":"ScriptResult","id":4,"status":"LaunchFailed"}"#,
        r#"{"type":"ScriptResult","id":5,"status":"Terminated"}"#,
    ];
    let mut reader = responses.as_slice();
    let mut golden = Vec::new();
    for fixture in fixtures {
        let bytes = frame(fixture.as_bytes());
        let expected = read_frame::<Response>(&mut bytes.as_slice())
            .unwrap()
            .unwrap();
        assert!(
            matches!(read_frame::<Response>(&mut reader), Ok(Some(decoded)) if decoded == expected)
        );
        golden.extend(bytes);
    }
    assert!(matches!(read_frame::<Response>(&mut reader), Ok(None)));
    assert_eq!(responses, golden);
}

mod compose_session {
    use std::time::Duration;

    use arx_guest::compose::{
        COMPOSE_CAPABILITIES, COMPOSE_PROTOCOL_VERSION, Clock, CommandRunner, ComposeRequest,
        ComposeResponse, ComposeService, ComposeStore, Invocation, RejectCode, RunResult,
        STAGE_CHUNK, SecretFileWitness, SecretMatch, SecretText, StoreError, encode_base64,
    };
    use arx_guest::serve_with_compose;

    use super::*;

    const COMPOSE_HELLO: &[u8] = br#"{"type":"ComposeHello","version":2,"challenge":"0123456789abcdef0123456789abcdef","generation":7,"capabilities":["lifecycle","logs","manifest","secrets","status"]}"#;
    const COMPOSE_HELLO_ACK: &[u8] = br#"{"type":"ComposeHelloAck","version":2,"challenge":"0123456789abcdef0123456789abcdef","generation":7,"capabilities":["lifecycle","logs","manifest","secrets","status"],"cleared_staging":0,"ambiguous_operations":0}"#;
    const STATUS: &[u8] = br#"{"type":"ComposeStatus","id":3,"project_id":"calm-life"}"#;
    const SCRIPT: &[u8] = br#"{"type":"RunScript","id":2,"script":"echo hello","timeout_s":1}"#;
    const DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    struct NoEngine;

    impl CommandRunner for NoEngine {
        fn run(&mut self, _: &Invocation) -> RunResult {
            panic!("the session reached the engine");
        }

        fn probe_http(&mut self, _: u16, _: &str) -> bool {
            false
        }

        fn pause(&mut self, _: Duration) {}
    }

    struct NoStore;

    impl ComposeStore for NoStore {
        fn load_state(&mut self) -> Result<Option<Vec<u8>>, StoreError> {
            Ok(None)
        }

        fn save_state(&mut self, _: &[u8]) -> Result<(), StoreError> {
            Ok(())
        }

        fn write_revision(&mut self, _: &str, _: u64, _: &str) -> Result<(), StoreError> {
            Ok(())
        }

        fn write_rotation(&mut self, _: &str, _: u64, _: &str, _: &str) -> Result<(), StoreError> {
            Ok(())
        }

        fn write_secret(
            &mut self,
            _: &str,
            _: &str,
            _: (u32, u32),
            _: &[u8],
        ) -> Result<SecretFileWitness, StoreError> {
            panic!("an unknown project reached secret storage");
        }

        fn stat_secret(&mut self, _: &str, _: &str, _: (u32, u32)) -> Option<SecretFileWitness> {
            None
        }

        fn compare_secret(&mut self, _: &str, _: &str, _: (u32, u32), _: &[u8]) -> SecretMatch {
            SecretMatch::Missing
        }

        fn prepare_volume(&mut self, _: &str, _: (u32, u32)) -> Result<(), StoreError> {
            Ok(())
        }
    }

    struct ZeroClock;

    impl Clock for ZeroClock {
        fn now_unix(&self) -> u64 {
            0
        }
    }

    struct Fixed;

    impl ScriptExecutor for Fixed {
        fn script_execution(&self) -> ScriptExecution {
            ScriptExecution::Available
        }

        fn run_script(&mut self, _: &str, _: u32) -> ScriptOutcome {
            ScriptOutcome::Exited { exit_code: 3 }
        }
    }

    struct ScriptsUnavailable;

    impl ScriptExecutor for ScriptsUnavailable {
        fn script_execution(&self) -> ScriptExecution {
            ScriptExecution::Unavailable
        }

        fn run_script(&mut self, _: &str, _: u32) -> ScriptOutcome {
            ScriptOutcome::Unavailable
        }
    }

    struct PanicOnScript;

    impl ScriptExecutor for PanicOnScript {
        fn script_execution(&self) -> ScriptExecution {
            ScriptExecution::Available
        }

        fn run_script(&mut self, _: &str, _: u32) -> ScriptOutcome {
            panic!("compose-qualified guest dispatched a legacy script");
        }
    }

    fn service() -> ComposeService {
        ComposeService::new(Box::new(NoEngine), Box::new(NoStore), Box::new(ZeroClock)).unwrap()
    }

    fn frames(payloads: &[&[u8]]) -> Vec<u8> {
        payloads.iter().flat_map(|payload| frame(payload)).collect()
    }

    fn compose(input: &[u8]) -> (Result<(), ProtocolError>, Vec<u8>) {
        let mut output = Vec::new();
        let result = serve_with_compose(input, &mut output, &mut PanicOnScript, Some(&service()));
        (result, output)
    }

    fn hello_with(edit: impl FnOnce(&mut serde_json::Value)) -> Vec<u8> {
        let mut value: serde_json::Value = serde_json::from_slice(COMPOSE_HELLO).unwrap();
        edit(&mut value);
        serde_json::to_vec(&value).unwrap()
    }

    #[test]
    fn legacy_sessions_keep_their_bytes_on_a_compose_guest() {
        let streams: Vec<Vec<u8>> = vec![
            frames(&[HELLO, PING, SCRIPT, HELLO, PING]),
            frames(&[PING]),
            frames(&[HELLO, br#"{"type":"Nope","id":1}"#]),
            frames(&[HELLO, br#"{"type":"Ping","type":"ComposeHello","id":1}"#]),
            frames(&[WRONG_VERSION_HELLO]),
            frames(&[HELLO, b"[1]"]),
            frames(&[HELLO, br#"{"type":"Ping","id":0}"#]),
            frame(HELLO)[..9].to_vec(),
        ];
        for input in &streams {
            // Non-Compose guests keep TEST/Windows bytes and scripts identically.
            let mut legacy = Vec::new();
            let legacy_result = serve(input.as_slice(), &mut legacy, &mut Fixed);
            let mut non_compose = Vec::new();
            let non_compose_result =
                serve_with_compose(input.as_slice(), &mut non_compose, &mut Fixed, None);
            assert_eq!(non_compose_result, legacy_result);
            assert_eq!(non_compose, legacy);

            // A Compose-qualified guest keeps exact legacy framing and disables
            // scripts even if a script-capable executor is supplied (design §3).
            let mut no_scripts = Vec::new();
            let no_scripts_result =
                serve(input.as_slice(), &mut no_scripts, &mut ScriptsUnavailable);
            let (compose_result, compose_bytes) = compose(input);
            assert_eq!(compose_result, no_scripts_result);
            assert_eq!(compose_bytes, no_scripts);
        }
        // A legacy guest still refuses the Compose handshake as unknown JSON.
        let mut output = Vec::new();
        assert_eq!(
            serve(frame(COMPOSE_HELLO).as_slice(), &mut output, &mut Fixed),
            Err(ProtocolError::InvalidJson)
        );
        assert!(output.is_empty());
        // Without a Compose service the dispatcher is the legacy one.
        let mut output = Vec::new();
        assert_eq!(
            serve_with_compose(
                frame(COMPOSE_HELLO).as_slice(),
                &mut output,
                &mut Fixed,
                None
            ),
            Err(ProtocolError::InvalidJson)
        );
        assert!(output.is_empty());
    }

    #[test]
    fn compose_hello_and_ack_have_golden_bytes() {
        let request = ComposeRequest::ComposeHello {
            version: COMPOSE_PROTOCOL_VERSION,
            challenge: CHALLENGE.to_owned(),
            generation: 7,
            capabilities: COMPOSE_CAPABILITIES.map(str::to_owned).to_vec(),
        };
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &request).unwrap();
        assert_eq!(bytes, frame(COMPOSE_HELLO));
        assert_eq!(
            compose(&frame(COMPOSE_HELLO)),
            (Ok(()), frame(COMPOSE_HELLO_ACK))
        );
    }

    #[test]
    fn compose_handshake_is_closed_and_sessions_stay_separate() {
        let refused: Vec<(Vec<u8>, ProtocolError)> = vec![
            (
                hello_with(|v| {
                    v["capabilities"] =
                        serde_json::json!(["lifecycle", "logs", "manifest", "secrets"])
                }),
                ProtocolError::UnsupportedCapability,
            ),
            (
                hello_with(|v| {
                    v["capabilities"]
                        .as_array_mut()
                        .unwrap()
                        .push("shell".into())
                }),
                ProtocolError::UnsupportedCapability,
            ),
            (
                hello_with(|v| v["capabilities"].as_array_mut().unwrap().reverse()),
                ProtocolError::UnsupportedCapability,
            ),
            // v1 ComposeHello is fenced off after protocol bump to v2.
            (
                hello_with(|v| v["version"] = 1.into()),
                ProtocolError::UnsupportedVersion,
            ),
            (
                hello_with(|v| v["version"] = 3.into()),
                ProtocolError::UnsupportedVersion,
            ),
            (
                hello_with(|v| v["generation"] = 0.into()),
                ProtocolError::InvalidComposeField,
            ),
            (
                hello_with(|v| v["challenge"] = "ABC".into()),
                ProtocolError::InvalidChallenge,
            ),
            (
                hello_with(|v| v["debug"] = true.into()),
                ProtocolError::InvalidJson,
            ),
            (STATUS.to_vec(), ProtocolError::ExpectedHello),
        ];
        for (payload, error) in refused {
            assert_eq!(compose(&frame(&payload)), (Err(error), Vec::new()));
        }

        // Compose requests need the Compose handshake.
        let (result, bytes) = compose(&frames(&[HELLO, STATUS]));
        assert_eq!(result, Err(ProtocolError::ExpectedComposeHello));
        let mut bytes = bytes.as_slice();
        assert!(matches!(
            read_frame::<Response>(&mut bytes),
            Ok(Some(Response::HelloAck { .. }))
        ));
        assert!(bytes.is_empty());

        // A Compose session answers Ping and refuses scripts; switching to a
        // legacy Hello on a Compose-qualified guest still keeps scripts
        // unavailable (design §3) and never dispatches to the script executor.
        let (result, bytes) = compose(&frames(&[COMPOSE_HELLO, PING, HELLO, SCRIPT]));
        assert_eq!(result, Ok(()));
        let mut expected = frame(COMPOSE_HELLO_ACK);
        expected.extend(frame(br#"{"type":"Pong","id":1}"#));
        expected.extend(frame(
            br#"{"type":"HelloAck","version":1,"challenge":"0123456789abcdef0123456789abcdef","script_execution":"unavailable"}"#,
        ));
        expected.extend(frame(
            br#"{"type":"ScriptResult","id":2,"status":"Unavailable"}"#,
        ));
        assert_eq!(bytes, expected);
        let (result, bytes) = compose(&frames(&[COMPOSE_HELLO, SCRIPT]));
        assert_eq!(result, Err(ProtocolError::UnexpectedRequest));
        assert_eq!(bytes, frame(COMPOSE_HELLO_ACK));

        // Field bounds close the connection before dispatch.
        for (payload, error) in [
            (
                &br#"{"type":"ComposeLogs","id":3,"project_id":"calm-life","cursor":0,"limit":33}"#
                    [..],
                ProtocolError::InvalidComposeField,
            ),
            (
                br#"{"type":"ComposeStatus","id":3,"project_id":"Calm_Life"}"#,
                ProtocolError::InvalidComposeField,
            ),
            (
                br#"{"type":"ComposeStatus","id":0,"project_id":"calm-life"}"#,
                ProtocolError::InvalidId,
            ),
            (
                br#"{"type":"ComposeOperation","id":3,"operation_id":"xyz"}"#,
                ProtocolError::InvalidComposeField,
            ),
        ] {
            let (result, bytes) = compose(&frames(&[COMPOSE_HELLO, payload]));
            assert_eq!(result, Err(error));
            assert_eq!(bytes, frame(COMPOSE_HELLO_ACK));
        }

        // Typed refusals keep the session open.
        let (result, bytes) = compose(&frames(&[COMPOSE_HELLO, STATUS, PING]));
        assert_eq!(result, Ok(()));
        let mut bytes = &bytes[frame(COMPOSE_HELLO_ACK).len()..];
        assert_eq!(
            read_frame::<ComposeResponse>(&mut bytes),
            Ok(Some(ComposeResponse::ComposeRejected {
                id: 3,
                code: RejectCode::UnknownProject,
                rule: None,
            }))
        );
        assert!(matches!(
            read_frame::<Response>(&mut bytes),
            Ok(Some(Response::Pong { id: 1 }))
        ));
    }

    #[test]
    fn manifest_chunks_fit_one_frame_at_the_bound() {
        let at_bound = ComposeRequest::ComposeStageChunk {
            id: 1,
            digest: "a".repeat(64),
            index: 0,
            data: encode_base64(&[0x41; STAGE_CHUNK]),
        };
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &at_bound).unwrap();
        assert!(bytes.len() <= MAX_FRAME + 4);
        assert!(matches!(
            read_frame::<ComposeRequest>(&mut bytes.as_slice()),
            Ok(Some(decoded)) if decoded == at_bound
        ));
        let over = ComposeRequest::ComposeStageChunk {
            id: 1,
            digest: "a".repeat(64),
            index: 0,
            data: encode_base64(&[0x41; STAGE_CHUNK + 3]),
        };
        assert_eq!(
            write_frame(&mut Vec::new(), &over),
            Err(ProtocolError::InvalidComposeField)
        );
    }

    #[test]
    fn secret_frames_never_echo_their_bytes() {
        let canary = format!("arx-canary-{:016x}", 0x0dd_ba11_c0ff_ee00_u64);
        let encoded = encode_base64(canary.as_bytes());
        let put = |slot: &str, digest: &str| {
            serde_json::to_vec(&ComposeRequest::ComposeSecretPut {
                id: 4,
                operation_id: "0".repeat(32),
                generation: 7,
                project_id: "calm-life".to_owned(),
                digest: digest.to_owned(),
                slot: slot.to_owned(),
                epoch: 1,
                data: SecretText::new(encoded.clone()),
            })
            .unwrap()
        };
        let (result, bytes) = compose(&frames(&[COMPOSE_HELLO, &put("admin_password", DIGEST)]));
        assert_eq!(result, Ok(()));
        let text = String::from_utf8_lossy(&bytes).into_owned();
        assert!(text.contains("unknown_project"));
        let (result, _) = compose(&frames(&[COMPOSE_HELLO, &put("Bad-Slot", DIGEST)]));
        let error = result.unwrap_err();
        assert_eq!(error, ProtocolError::InvalidComposeField);
        let (bad_digest, _) = compose(&frames(&[COMPOSE_HELLO, &put("admin_password", "not-hex")]));
        assert_eq!(bad_digest, Err(ProtocolError::InvalidComposeField));
        for haystack in [text, format!("{error:?} {error}")] {
            assert!(!haystack.contains(&canary) && !haystack.contains(&encoded));
        }
    }

    #[test]
    fn escaped_secret_json_and_malformed_compose_frames_are_rejected() {
        let escaped_cases: &[&[u8]] = &[
            // Missing explicit release digest on v2 ComposeSecretPut.
            br#"{"type":"ComposeSecretPut","id":4,"operation_id":"00000000000000000000000000000000","generation":7,"project_id":"calm-life","slot":"admin_password","epoch":1,"data":"QUFBQQ=="}"#,
            // Unicode escape in Base64 secret field.
            br#"{"type":"ComposeSecretPut","id":4,"operation_id":"00000000000000000000000000000000","generation":7,"project_id":"calm-life","digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","slot":"admin_password","epoch":1,"data":"\u0041\u0041\u0041\u0041"}"#,
            // Forward-slash escape in Base64 secret field.
            br#"{"type":"ComposeSecretPut","id":4,"operation_id":"00000000000000000000000000000000","generation":7,"project_id":"calm-life","digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","slot":"admin_password","epoch":1,"data":"QUJ\/"}"#,
            // Escaped type tag on a secret frame.
            br#"{"type":"\u0043omposeSecretPut","id":4,"operation_id":"00000000000000000000000000000000","generation":7,"project_id":"calm-life","digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","slot":"admin_password","epoch":1,"data":"QUFBQQ=="}"#,
            // Secret field before type, followed by an unknown field.
            br#"{"data":"QUFBQQ==","type":"ComposeSecretPut","id":4,"operation_id":"00000000000000000000000000000000","generation":7,"project_id":"calm-life","digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","slot":"admin_password","epoch":1,"extra":true}"#,
            // Cross-variant extra field on a Compose request.
            br#"{"type":"ComposeStatus","id":3,"project_id":"calm-life","revision":1}"#,
            // Duplicate field on a Compose request.
            br#"{"type":"ComposeStatus","id":3,"id":4,"project_id":"calm-life"}"#,
        ];
        for payload in escaped_cases {
            let (result, bytes) = compose(&frames(&[COMPOSE_HELLO, payload]));
            assert_eq!(
                result,
                Err(ProtocolError::InvalidJson),
                "session accepted: {}",
                String::from_utf8_lossy(payload)
            );
            assert_eq!(bytes, frame(COMPOSE_HELLO_ACK));
            assert!(matches!(
                read_frame::<ComposeRequest>(&mut frame(payload).as_slice()),
                Err(ProtocolError::InvalidJson)
            ));
        }
        // Even direct serde_json deserialization refuses escaped SecretText.
        let escaped = r#"{"type":"ComposeSecretPut","id":4,"operation_id":"00000000000000000000000000000000","generation":7,"project_id":"calm-life","digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","slot":"admin_password","epoch":1,"data":"\u0041\u0041\u0041\u0041"}"#;
        assert!(serde_json::from_str::<ComposeRequest>(escaped).is_err());
        // Unescaped SecretText in any key order still decodes cleanly.
        let reordered = br#"{"data":"QUFBQQ==","type":"ComposeSecretPut","id":4,"operation_id":"00000000000000000000000000000000","generation":7,"project_id":"calm-life","digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","slot":"admin_password","epoch":1}"#;
        assert!(matches!(
            read_frame::<ComposeRequest>(&mut frame(reordered).as_slice()),
            Ok(Some(ComposeRequest::ComposeSecretPut { epoch: 1, .. }))
        ));
    }
}
