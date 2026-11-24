//! The first request must be a Hello. Every valid Hello starts a fresh session
//! and receives a HelloAck echoing its challenge. A Hello buffered across a
//! virtio-serial port close can reach the guest before the host's fresh Hello,
//! so both are serviced on the same connection.
//!
//! Requests are processed serially. A Hello arriving during a script is read
//! only after the script finishes and its result is written. The host matches
//! the HelloAck to its challenge and discards frames before the matching ack.
//! The loop retains no challenge or request history across resets.
//!
//! A guest that serves Compose also accepts a pinned ComposeHello, which
//! starts a Compose session in the same way. Frames whose tag does not start
//! with `Compose` are decoded exactly as on a legacy guest, so legacy sessions
//! keep their framing and wire format. Per settled design §3, a Compose-qualified
//! guest disables legacy script execution (reporting `unavailable` on legacy
//! `Hello` and `Unavailable` on legacy `RunScript`, and closing a Compose
//! session with `UnexpectedRequest` on `RunScript`) because the script runner
//! shares the guest network namespace and has no ingress-isolation witness.
//! Non-Compose guests (`serve` and `serve_with_compose(..., None)`) keep TEST
//! and Windows behavior byte-for-byte.

use std::io::{Read, Write};

use crate::compose::{ComposeRequest, ComposeService, is_compose_tag};
use crate::protocol::{
    ProtocolError, Request, Response, ScriptExecution, ScriptStatus, decode, peek_tag, read_frame,
    read_payload, write_frame,
};

pub trait ScriptExecutor {
    fn script_execution(&self) -> ScriptExecution;

    /// Runs a script with the supplied guest execution budget. Guest runners
    /// discard script stdout and stderr.
    fn run_script(&mut self, script: &str, timeout_s: u32) -> ScriptOutcome;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptOutcome {
    Exited { exit_code: u32 },
    Unavailable,
    TimedOut,
    LaunchFailed,
    Terminated,
}

/// Serves a single connection until clean EOF or the first protocol error.
pub fn serve(
    mut reader: impl Read,
    mut writer: impl Write,
    executor: &mut impl ScriptExecutor,
) -> Result<(), ProtocolError> {
    let mut handshake_complete = false;
    while let Some(request) = read_frame(&mut reader)? {
        if !handshake_complete && !matches!(request, Request::Hello { .. }) {
            return Err(ProtocolError::ExpectedHello);
        }
        let response = match request {
            Request::Hello { version, challenge } => {
                handshake_complete = true;
                Response::HelloAck {
                    version,
                    challenge,
                    script_execution: executor.script_execution(),
                }
            }
            Request::Ping { id } => Response::Pong { id },
            Request::RunScript {
                id,
                script,
                timeout_s,
            } => run_script(executor, id, &script, timeout_s),
        };
        write_frame(&mut writer, &response)?;
    }
    Ok(())
}

fn run_script(
    executor: &mut impl ScriptExecutor,
    id: u32,
    script: &str,
    timeout_s: u32,
) -> Response {
    let (status, exit_code) = match executor.run_script(script, timeout_s) {
        ScriptOutcome::Exited { exit_code } => (ScriptStatus::Exited, Some(exit_code)),
        ScriptOutcome::Unavailable => (ScriptStatus::Unavailable, None),
        ScriptOutcome::TimedOut => (ScriptStatus::TimedOut, None),
        ScriptOutcome::LaunchFailed => (ScriptStatus::LaunchFailed, None),
        ScriptOutcome::Terminated => (ScriptStatus::Terminated, None),
    };
    Response::ScriptResult {
        id,
        status,
        exit_code,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Unbound,
    Legacy,
    Compose,
}

/// Serves a connection on a guest that may serve Compose. With `None` this is
/// exactly [`serve`].
pub fn serve_with_compose(
    mut reader: impl Read,
    mut writer: impl Write,
    executor: &mut impl ScriptExecutor,
    compose: Option<&ComposeService>,
) -> Result<(), ProtocolError> {
    let Some(compose) = compose else {
        return serve(reader, writer, executor);
    };
    let mut mode = Mode::Unbound;
    while let Some(payload) = read_payload(&mut reader)? {
        if peek_tag(payload.json()).is_some_and(|tag| is_compose_tag(&tag)) {
            let request: ComposeRequest = decode(payload.json())?;
            // Wipe the frame text before any engine or storage work.
            drop(payload);
            let hello = matches!(request, ComposeRequest::ComposeHello { .. });
            match (mode, hello) {
                (_, true) => mode = Mode::Compose,
                (Mode::Compose, false) => {}
                (Mode::Unbound, false) => return Err(ProtocolError::ExpectedHello),
                (Mode::Legacy, false) => return Err(ProtocolError::ExpectedComposeHello),
            }
            let response = compose.handle(request);
            write_frame(&mut writer, &response)?;
            continue;
        }
        let request: Request = decode(payload.json())?;
        drop(payload);
        if mode == Mode::Unbound && !matches!(request, Request::Hello { .. }) {
            return Err(ProtocolError::ExpectedHello);
        }
        let response = match request {
            Request::Hello { version, challenge } => {
                mode = Mode::Legacy;
                // Design §3: scripts are disabled on Compose-qualified guests
                // because the script runner cannot qualify ingress isolation.
                Response::HelloAck {
                    version,
                    challenge,
                    script_execution: ScriptExecution::Unavailable,
                }
            }
            Request::Ping { id } => Response::Pong { id },
            Request::RunScript { .. } if mode == Mode::Compose => {
                return Err(ProtocolError::UnexpectedRequest);
            }
            Request::RunScript { id, .. } => Response::ScriptResult {
                id,
                status: ScriptStatus::Unavailable,
                exit_code: None,
            },
        };
        write_frame(&mut writer, &response)?;
    }
    Ok(())
}
