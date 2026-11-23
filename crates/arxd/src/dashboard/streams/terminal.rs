use super::{Close, Dashboard, Stream, TEXT_LIMIT, message};
use crate::{
    development::{self, Admission, Cancellation, Execution, Request, ResultData},
    store::{Activity, Outcome, Reason, StopReason},
};
use axum::extract::ws::Message;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

const OUTPUT_LIMIT: usize = 4096;
const OUTPUT_CHUNK: usize = 2048;
/// `started` reached the client but the terminal audit did not commit, so no
/// result may follow. The fixed reason carries no command content.
const AUDIT_FAILED: Close = Close {
    code: 1011,
    reason: "command audit failed",
};

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Input {
    Run {
        tool: String,
        args: Vec<String>,
        #[serde(rename = "timeoutS", default = "default_timeout")]
        timeout_s: u64,
    },
    Cancel {},
}

fn default_timeout() -> u64 {
    60
}

pub(super) async fn receive(state: &Dashboard, stream: &Stream, text: &str) -> bool {
    let input = match serde_json::from_str(text) {
        Ok(Input::Cancel {}) if text.len() <= 64 => {
            stream.cancel();
            return true;
        }
        Ok(Input::Run {
            tool,
            args,
            timeout_s,
        }) if !tool.is_empty()
            && tool.len() <= 64
            && args.len() <= 128
            && args.iter().all(|arg| arg.len() <= 8192)
            && (1..=3600).contains(&timeout_s) =>
        {
            Request {
                tool,
                args,
                timeout_s,
                cwd: String::new(),
                output_bytes: OUTPUT_LIMIT,
            }
        }
        _ => return false,
    };
    let Some(execution) = state
        .streams
        .terminal
        .as_ref()
        .filter(|e| e.owner_available())
    else {
        stream.close(super::Close::TARGET, None);
        return true;
    };
    if let Some((admission, cancel)) = stream.admit_terminal(|| execution.admit()) {
        let worker = Command {
            stream: stream.clone(),
            execution: execution.clone(),
            activity: state.activity.clone(),
            input,
            accepted: false,
        };
        // Admission tracks the worker through terminal audit even when the
        // socket or peer goes away. Refusals are serialized per reader.
        if let Some(cancel) = cancel {
            tokio::task::spawn_blocking(move || worker.run(admission, cancel));
        } else {
            let _ = tokio::task::spawn_blocking(move || worker.refuse_busy(admission)).await;
        }
    }
    true
}

struct Command {
    stream: Stream,
    execution: Arc<Execution>,
    activity: Arc<Activity>,
    input: Request,
    /// Set once `started` is queued to the client.
    accepted: bool,
}

impl Command {
    fn start(&self, admission: &mut Admission) -> Result<i64, Reason> {
        let metadata = self.execution.metadata(Some(&self.input), None, None);
        let id = self
            .activity
            .start_owner_command(&metadata)
            .map_err(|_| Reason::StorageFailed)?;
        admission.started();
        Ok(id)
    }

    fn refuse_busy(&self, mut admission: Admission) {
        let result = self.start(&mut admission).and_then(|id| {
            self.activity
                .finish_typed(
                    id,
                    Outcome::Refused,
                    &self
                        .execution
                        .metadata(Some(&self.input), None, Some(Reason::Busy)),
                )
                .map_err(|_| Reason::StorageFailed)?;
            admission.completed();
            Ok(())
        });
        self.stream
            .queue([error(result.err().unwrap_or(Reason::Busy))]);
    }

    fn run(mut self, mut admission: Admission, cancel: Cancellation) {
        match self.run_tracked(&mut admission, &cancel) {
            Ok(frames) => self.stream.complete(frames),
            Err(reason) if self.accepted => {
                // Only the terminal audit commit can fail after `started`.
                // Output stays withheld and the unconfirmed admission keeps
                // drain from reporting success. Close first so no new run is
                // admitted before the command slot is released.
                self.stream.close(AUDIT_FAILED, Some(error(reason)));
                self.stream.complete(std::iter::empty());
            }
            Err(reason) => self.stream.complete([error(reason)]),
        }
    }

    fn run_tracked(
        &mut self,
        admission: &mut Admission,
        cancel: &Cancellation,
    ) -> Result<Vec<Message>, Reason> {
        let id = self.start(admission)?;
        let command_id = format!("c{id}");
        let allowed = self.input.validate()
            && self.activity.tool_cache().is_some_and(|cache| {
                cache.current_user
                    && cache.tools.as_array().is_some_and(|tools| {
                        tools
                            .iter()
                            .any(|tool| tool["name"] == self.input.tool && tool["found"] == true)
                    })
            });
        let started = message(json!({"type":"started","commandId":command_id,
            "tool":self.input.tool,"args":self.input.args}));
        let fits = matches!(&started, Message::Text(text) if text.len() <= TEXT_LIMIT);
        let result = if allowed && fits {
            if let Some(deadline) = self.stream.deadline() {
                let remaining =
                    deadline.saturating_duration_since(tokio::time::Instant::now().into_std());
                self.input.timeout_s = self.input.timeout_s.min(remaining.as_secs().max(1));
            }
            self.execution.run_owner_command(&self.input, cancel, || {
                self.accepted = true;
                self.stream.queue([started]);
            })
        } else {
            Err(Reason::InvalidArguments)
        };
        let cancelled = self.stream.claim_completion(cancel);
        let output = result.as_ref().ok();
        let reason = result.as_ref().err().copied();
        let mut metadata = self.execution.metadata(Some(&self.input), output, reason);
        let outcome = if cancelled
            || output.is_some_and(|output| {
                matches!(
                    output.stop_reason,
                    StopReason::Cancelled | StopReason::Shutdown
                )
            }) {
            if metadata.stop_reason.is_none() {
                metadata.stop_reason = Some(StopReason::Cancelled);
            }
            Outcome::Interrupted
        } else if let Some(output) = output {
            development::outcome(output)
        } else {
            Outcome::Refused
        };
        self.activity
            .finish_typed(id, outcome, &metadata)
            .map_err(|_| Reason::StorageFailed)?;
        admission.completed();
        if !self.accepted
            && let Some(reason) = reason
        {
            return Err(reason);
        }
        Ok(output_frames(&command_id, outcome, output))
    }
}

fn output_frames(id: &str, outcome: Outcome, output: Option<&ResultData>) -> Vec<Message> {
    let mut frames = Vec::with_capacity(4);
    let mut remaining = OUTPUT_LIMIT;
    let mut truncated = false;
    let mut exit_code = None;
    if let Some(output) =
        output.filter(|output| output.cleanup_confirmed && output.matches_current_user)
    {
        exit_code = output.exit_code;
        truncated = output.stdout_dropped_bytes != 0 || output.stderr_dropped_bytes != 0;
        for (stream, mut data) in [
            ("stdout", output.stdout.as_str()),
            ("stderr", output.stderr.as_str()),
        ] {
            while !data.is_empty() && remaining > 0 {
                let cut = data.floor_char_boundary(data.len().min(remaining).min(OUTPUT_CHUNK));
                if cut == 0 {
                    break;
                }
                let (prefix, rest) = data.split_at(cut);
                frames.push(message(
                    json!({"type":"output","commandId":id,"stream":stream,"data":prefix}),
                ));
                remaining -= cut;
                data = rest;
            }
            truncated |= !data.is_empty();
        }
    }
    frames.push(message(
        json!({"type":"exit","commandId":id,"exitCode":exit_code,
        "outcome":outcome.as_str(),"truncated":truncated}),
    ));
    frames
}

fn error(reason: Reason) -> Message {
    let summary = match reason {
        Reason::Busy => "A command is already running.",
        Reason::InvalidArguments => "The command is outside the allowed tool or argument policy.",
        _ => "The dashboard could not run the command.",
    };
    message(json!({"type":"error","code":reason.as_str(),"message":summary}))
}
