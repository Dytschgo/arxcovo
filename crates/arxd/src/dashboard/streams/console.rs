use super::{ApiError, Close, Dashboard, Stream, Target, message};
use crate::{
    dashboard::{instant_string, session::Capture},
    vm::console::{CaptureError, Frame},
};
use axum::extract::ws::Message;
use serde_json::json;
use std::{future::Future, pin::Pin, sync::Arc, time::Duration};

pub(super) trait Sources: Send + Sync {
    fn lease(&self, vm: [u8; 16]) -> Option<Arc<dyn Lease>>;
}

pub(super) trait Lease: Send + Sync {
    fn generation(&self) -> u64;
    fn state(&self) -> Result<(), CaptureError>;
    fn capture(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<Option<Frame>, CaptureError>> + Send + '_>>;
}

fn source(state: &Dashboard, vm: [u8; 16]) -> Option<Arc<dyn Lease>> {
    state.streams.console.as_ref()?.lease(vm)
}

pub(super) async fn resolve(state: &Dashboard, id: &str) -> Result<Target, ApiError> {
    let mut vm = [0; 16];
    hex::decode_to_slice(id, &mut vm).map_err(|_| ApiError::NOT_FOUND)?;
    if hex::encode(vm) != id {
        return Err(ApiError::NOT_FOUND);
    }
    let catalog = state.vm_catalog.clone().ok_or(ApiError::NOT_FOUND)?;
    let now = state.clock.now().utc_ms;
    let found = tokio::task::spawn_blocking(move || catalog.list(Some(vm), now))
        .await
        .map_err(|_| ApiError::UNAVAILABLE)?
        .map_err(|_| ApiError::UNAVAILABLE)?;
    if found.is_empty() {
        return Err(ApiError::NOT_FOUND);
    }
    let lease = source(state, vm).ok_or(ApiError::UNAVAILABLE)?;
    lease.state().map_err(|_| ApiError::UNAVAILABLE)?;
    Ok(Target::Console {
        vm,
        generation: lease.generation(),
    })
}

fn live(state: &Dashboard, vm: [u8; 16], generation: u64) -> Result<Arc<dyn Lease>, CaptureError> {
    let lease = source(state, vm)
        .filter(|lease| lease.generation() == generation)
        .ok_or(CaptureError::Replaced)?;
    lease.state()?;
    Ok(lease)
}

fn close(stream: &Stream, error: CaptureError) {
    match error {
        CaptureError::Replaced => stream.close(Close::TARGET, None),
        CaptureError::Stopped => stream.close(
            Close {
                code: 1000,
                reason: "VM stopped",
            },
            Some(message(json!({"type":"closed","reason":"VM stopped"}))),
        ),
        CaptureError::Fault => stream.close(
            Close {
                code: 1011,
                reason: "console capture failed",
            },
            Some(message(
                json!({"type":"closed","reason":"Console capture failed"}),
            )),
        ),
    }
}

pub(super) fn check(state: &Dashboard, stream: &Stream, vm: [u8; 16], generation: u64) -> bool {
    match live(state, vm, generation) {
        Ok(_) => {
            let target = Target::Console { vm, generation };
            if let Some(job) = state.sessions.capture(target, state.clock.clone()) {
                let state = state.clone();
                tokio::spawn(capture(state, job, vm, generation));
            }
            true
        }
        Err(error) => {
            close(stream, error);
            false
        }
    }
}

async fn capture(state: Dashboard, job: Capture, vm: [u8; 16], generation: u64) {
    let mut ticks = tokio::time::interval(Duration::from_secs(1));
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut seq = 0u64;
    loop {
        ticks.tick().await;
        let viewers = job.viewers();
        if viewers.is_empty() {
            return;
        }
        let result = match live(&state, vm, generation) {
            Ok(lease) if viewers.iter().any(Stream::console_space) => lease.capture().await,
            Ok(_) => continue,
            Err(error) => Err(error),
        };
        let viewers = job.viewers();
        // Revalidate after every capture, successful or not. A lease from a
        // replaced generation may report its own stop or fault first; the
        // authoritative replacement must win over that ordinary close.
        let result = match (result, live(&state, vm, generation)) {
            (_, Err(CaptureError::Replaced)) => Err(CaptureError::Replaced),
            (Err(error), _) | (Ok(_), Err(error)) => Err(error),
            (Ok(frame), Ok(_)) => Ok(frame),
        };
        match result {
            Ok(Some(frame)) => {
                seq += 1;
                let Ok(time) = instant_string(state.clock.now().utc_ms) else {
                    for viewer in viewers {
                        close(&viewer, CaptureError::Fault);
                    }
                    return;
                };
                let (width, height, bytes) = frame.image();
                let meta = message(json!({"type":"frame","seq":seq,"mime":"image/png",
                    "width":width,"height":height,"capturedAt":time}));
                for viewer in viewers {
                    viewer.queue_console([meta.clone(), Message::Binary(bytes.clone())]);
                }
            }
            Ok(None) => {}
            Err(error) => {
                for viewer in viewers {
                    close(&viewer, error);
                }
                return;
            }
        }
    }
}
