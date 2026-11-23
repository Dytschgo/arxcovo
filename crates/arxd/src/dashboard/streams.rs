//! Same-origin owner WebSocket entry points and transport.
mod console;
mod terminal;
#[cfg(test)]
mod tests;

use super::{
    ApiError, Dashboard, json_response, one, read,
    session::{Bucket, Close, Connection, Stream, Target, Write},
};
use crate::development::Execution;
use axum::{
    body::to_bytes,
    extract::{
        FromRequestParts, Request, WebSocketUpgrade,
        ws::{CloseFrame, Message, WebSocket},
    },
    http::header,
    response::Response,
};
use futures_util::{Sink, SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    future::{pending, poll_fn},
    sync::Arc,
    time::Duration,
};
use tokio::time::{Instant, sleep_until, timeout, timeout_at};
use tokio_tungstenite::tungstenite::Error as WireError;
use zeroize::Zeroizing;

const TEXT_LIMIT: usize = 128 * 1024;
const AUTH_TIMEOUT: Duration = Duration::from_secs(5);
const HEARTBEAT: Duration = Duration::from_secs(30);
const INACTIVE: Duration = Duration::from_secs(60);
const PROTOCOL: Close = Close {
    code: 1002,
    reason: "invalid stream message",
};
const MESSAGE_SIZE: Close = Close {
    code: 1009,
    reason: "stream message too large",
};
const RATE: Close = Close {
    code: 1008,
    reason: "stream message rate exceeded",
};

#[derive(Clone)]
pub(super) struct Backends {
    terminal: Option<Arc<Execution>>,
    console: Option<Arc<dyn console::Sources>>,
}

impl Backends {
    pub(super) fn new(executor: Option<Arc<Execution>>) -> Self {
        Self {
            terminal: executor.filter(|executor| executor.owner_available()),
            // The VM runtime must explicitly supply qualified capture custody.
            console: None,
        }
    }
}

pub(super) async fn mint(
    state: &Dashboard,
    request: Request,
    key: Option<[u8; 32]>,
) -> Result<Response, ApiError> {
    let route = request.uri().path().split('/').collect::<Vec<_>>();
    let (id, terminal) = match route.as_slice() {
        ["", "api", "v1", "hosts", id, "terminal"] => (*id, true),
        ["", "api", "v1", "vms", id, "console"] => (*id, false),
        _ => return Err(ApiError::NOT_FOUND),
    };
    read::validate_id(id)?;
    if request.uri().query().is_some()
        || request.headers().contains_key(header::TRANSFER_ENCODING)
        || (request.headers().contains_key(header::CONTENT_LENGTH)
            && one(request.headers(), "content-length") != Some("0"))
    {
        return Err(ApiError::INVALID);
    }
    let id = id.to_owned();
    let csrf = one(request.headers(), "x-arx-csrf").map(|value| Zeroizing::new(value.to_owned()));
    if !timeout(AUTH_TIMEOUT, to_bytes(request.into_body(), 0))
        .await
        .is_ok_and(|body| body.is_ok_and(|body| body.is_empty()))
    {
        return Err(ApiError::INVALID);
    }
    let target = if terminal {
        let activity = state.activity.clone();
        let local = tokio::task::spawn_blocking(move || activity.host_id())
            .await
            .map_err(|_| ApiError::UNAVAILABLE)?
            .map_err(|_| ApiError::UNAVAILABLE)?;
        if id != local {
            return Err(ApiError::NOT_FOUND);
        }
        state
            .streams
            .terminal
            .as_ref()
            .filter(|e| e.owner_available())
            .ok_or(ApiError::UNAVAILABLE)?;
        Target::Terminal(local)
    } else {
        console::resolve(state, &id).await?
    };
    let (id, ticket) = state.sessions.mint(
        key,
        csrf.as_ref().map(|value| value.as_str()),
        &state.boundary.origin,
        target,
        state.clock.as_ref(),
    )?;
    json_response(json!({"streamPath":format!("/api/v1/streams/{id}"),"ticket":ticket}))
}

pub(super) async fn upgrade(
    state: Dashboard,
    request: Request,
    key: Option<[u8; 32]>,
    id: String,
) -> Result<Response, ApiError> {
    if one(request.headers(), "origin") != Some(state.boundary.origin.as_str()) {
        return Err(ApiError::FORBIDDEN);
    }
    if super::session::cookie_key(&id).is_none()
        || request.uri().query().is_some()
        || request
            .headers()
            .contains_key(header::SEC_WEBSOCKET_PROTOCOL)
    {
        return Err(ApiError::INVALID);
    }
    let (mut parts, _) = request.into_parts();
    let upgrade = WebSocketUpgrade::from_request_parts(&mut parts, &())
        .await
        .map_err(|_| ApiError::INVALID)?;
    let connection = state.sessions.register_stream(key, state.clock.clone())?;
    // Axum drops the callback (and registration) if the upgrade fails.
    Ok(upgrade
        .read_buffer_size(16 * 1024)
        .write_buffer_size(0)
        .max_write_buffer_size(crate::vm::console::MAX_PNG + 4096)
        .max_frame_size(TEXT_LIMIT)
        .max_message_size(TEXT_LIMIT)
        .on_upgrade(move |socket| serve(state, connection, id, socket)))
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Auth {
    Auth { ticket: String },
}

async fn serve(state: Dashboard, connection: Connection, id: String, socket: WebSocket) {
    let (sink, mut source) = socket.split();
    let stream = connection.stream();
    let reader = async {
        // One per-socket message budget covers authentication too.
        let mut rate = Bucket::new(16, 16);
        let auth = timeout(AUTH_TIMEOUT, async {
            loop {
                let next = source.next().await;
                if matches!(next, Some(Ok(_))) && !rate.take(Instant::now().into_std()) {
                    return Err(RATE);
                }
                match next {
                    Some(Ok(Message::Text(text))) if text.len() <= 256 => {
                        let Auth::Auth { ticket } =
                            serde_json::from_str(&text).map_err(|_| Close::TICKET)?;
                        let ticket = Zeroizing::new(ticket);
                        return connection.redeem(&id, &ticket, &state.boundary.origin);
                    }
                    Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
                    Some(Err(error)) => return Err(wire_close(&error).unwrap_or(Close::TICKET)),
                    _ => return Err(Close::TICKET),
                }
            }
        })
        .await
        .unwrap_or(Err(Close::TICKET));
        let target = match auth {
            Ok(target) => target,
            Err(close) => {
                stream.close(close, None);
                return pending::<()>().await;
            }
        };
        if !revalidate(&state, &stream, &target).await {
            return pending::<()>().await;
        }
        if matches!(target, Target::Terminal(_)) {
            stream.queue([message(json!({"type":"ready"}))]);
        }
        let mut heartbeat = tokio::time::interval_at(Instant::now() + HEARTBEAT, HEARTBEAT);
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut targets = tokio::time::interval_at(
            Instant::now() + Duration::from_secs(1),
            Duration::from_secs(1),
        );
        targets.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut active = Instant::now();
        loop {
            tokio::select! {
                biased;
                _ = heartbeat.tick() => {
                    if Instant::now().duration_since(active) >= INACTIVE {
                        break;
                    }
                    // Console pairs always remain adjacent application messages.
                    stream.control(Message::Ping(axum::body::Bytes::new()));
                }
                _ = targets.tick() => {
                    if !revalidate(&state, &stream, &target).await {
                        break;
                    }
                }
                next = source.next() => {
                    let next = match next {
                        Some(Ok(Message::Close(_))) | None => return,
                        Some(Ok(next)) => next,
                        Some(Err(error)) => {
                            if let Some(close) = wire_close(&error) {
                                stream.close(close, None);
                                break;
                            }
                            return;
                        }
                    };
                    active = Instant::now();
                    if !rate.take(active.into_std()) {
                        stream.close(RATE, None);
                        break;
                    }
                    match next {
                        Message::Ping(_) | Message::Pong(_) => {}
                        Message::Text(text) if matches!(target, Target::Terminal(_)) => {
                            if !terminal::receive(&state, &stream, &text).await {
                                stream.close(PROTOCOL, None);
                                break;
                            }
                        }
                        _ => {
                            stream.close(PROTOCOL, None);
                            break;
                        }
                    }
                }
            }
        }
        if !stream.closing() {
            stream.close(
                Close {
                    code: 1001,
                    reason: "stream inactive",
                },
                None,
            );
        }
        pending::<()>().await
    };
    tokio::select! {
        biased;
        _ = writer(&stream, sink) => {}
        _ = reader => {}
        _ = session_clock(&stream) => {}
    }
}

fn wire_close(error: &axum::Error) -> Option<Close> {
    match std::error::Error::source(error)?.downcast_ref::<WireError>()? {
        WireError::Capacity(_) => Some(MESSAGE_SIZE),
        WireError::Protocol(_) | WireError::Utf8(_) => Some(PROTOCOL),
        _ => None,
    }
}

async fn revalidate(state: &Dashboard, stream: &Stream, target: &Target) -> bool {
    if stream.closing() {
        return false;
    }
    match target {
        Target::Terminal(_)
            if state
                .streams
                .terminal
                .as_ref()
                .is_some_and(|e| e.owner_available()) =>
        {
            true
        }
        Target::Terminal(_) => {
            stream.close(terminal_unavailable(state), None);
            false
        }
        Target::Console { vm, generation } => console::check(state, stream, *vm, *generation),
    }
}

/// A terminal whose executor stopped because the daemon is shutting down
/// closes with 1001, not as a lost target.
fn terminal_unavailable(state: &Dashboard) -> Close {
    if state
        .streams
        .terminal
        .as_ref()
        .is_some_and(|e| e.shutting_down())
    {
        Close::STOP
    } else {
        Close::TARGET
    }
}

async fn session_clock(stream: &Stream) {
    let Some(mut revision) = stream.revision() else {
        return;
    };
    loop {
        let _ = revision.borrow_and_update();
        if let Some(deadline) = stream.deadline() {
            tokio::select! {
                _ = revision.changed() => {}
                _ = sleep_until(Instant::from_std(deadline)) => {}
            }
        } else {
            pending::<()>().await;
        }
    }
}

async fn writer<S>(stream: &Stream, mut sink: S)
where
    S: Sink<Message> + Unpin,
{
    let Some(mut revision) = stream.revision() else {
        return;
    };
    loop {
        let _ = revision.borrow_and_update();
        match stream.commit() {
            Some(Write::Message(message, deadline)) => {
                if !timeout_at(Instant::from_std(deadline), sink.send(message))
                    .await
                    .is_ok_and(|result| result.is_ok())
                {
                    return;
                }
            }
            Some(Write::Close(committed, deadline)) => {
                // The close frame becomes irreversible only when the sink
                // accepts it (`start_send`). Wait for the sink first, then
                // pick the code from the slot and hand it over in the same
                // session-locked step, so a revocation that lands while the
                // sink is not ready still turns the frame into 4401.
                let _ = timeout_at(Instant::from_std(deadline), async {
                    poll_fn(|cx| sink.poll_ready_unpin(cx)).await.ok()?;
                    stream
                        .submit_close(committed, |close| {
                            sink.start_send_unpin(Message::Close(Some(CloseFrame {
                                code: close.code,
                                reason: close.reason.into(),
                            })))
                        })?
                        .ok()?;
                    sink.flush().await.ok()
                })
                .await;
                return;
            }
            Some(Write::Wait) => {
                if revision.changed().await.is_err() {
                    return;
                }
            }
            None => return,
        }
    }
}

fn message(value: Value) -> Message {
    Message::Text(value.to_string().into())
}
