//! The first request must be a Hello. Every valid Hello starts a fresh session
//! and receives a HelloAck echoing its challenge. A Hello buffered across a
//! virtio-serial port close can reach the guest before the host's fresh Hello,
//! so both are serviced on the same connection.
//!
//! Requests are processed serially. A Hello arriving during a script is read
//! only after the script finishes and its result is written. The host matches
//! the HelloAck to its challenge and discards frames before the matching ack.
//! The loop retains no challenge or request history across resets.

use std::io::{Read, Write};

use crate::protocol::{
    ProtocolError, Request, Response, ScriptExecution, ScriptStatus, read_frame, write_frame,
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
            } => {
                let (status, exit_code) = match executor.run_script(&script, timeout_s) {
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
        };
        write_frame(&mut writer, &response)?;
    }
    Ok(())
}
