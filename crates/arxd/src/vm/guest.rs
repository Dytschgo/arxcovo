use super::{
    qemu_args::GUEST_PORT_ID,
    qmp::{ChannelError, PortWatch, Qmp, bounded},
};
use crate::service::Shutdown;
use arx_guest::protocol::{
    MAX_FRAME, Message, PROTOCOL_VERSION, Request, Response, ScriptExecution, ScriptStatus,
    read_frame, write_frame,
};
use std::time::Duration;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    time::{Instant, sleep_until, timeout_at},
};

/// Absolute budget, from the accepted startup `cont`, for the guest service
/// to answer a challenge issued in this boot. Fixed: no recipe, profile or
/// caller can extend it.
pub const GUEST_READY_BUDGET: Duration = Duration::from_secs(180);

/// Once the first byte of a response arrives, the rest of that frame must
/// arrive within this bound, inside any outer deadline.
pub const FRAME_BOUND: Duration = Duration::from_secs(10);

/// Challenges a boot may issue. Issuing one more refuses.
pub const MAX_CHALLENGES: usize = 32;

/// Retry schedule while the port is not known to be closed: 1 s, then
/// doubling to an 8 s cap. Each port open restarts it at 1 s.
pub const RETRY_FIRST: Duration = Duration::from_secs(1);
pub const RETRY_MAX: Duration = Duration::from_secs(8);

/// Protocol state for a single authenticated transport. The Windows adapter
/// must verify the held server and this exact pipe handle before constructing it.
pub struct GuestChannel<T> {
    transport: Option<T>,
    next_id: u32,
    /// Bytes already read past the accepted readiness reply, kept in order.
    inbound: Inbound,
    /// This boot's other challenges. A late ack for one is skipped once.
    stale: Vec<String>,
}

impl<T: AsyncRead + AsyncWrite + Unpin> GuestChannel<T> {
    /// Installed-boot readiness.
    ///
    /// The guest service answers every `Hello` with an ack echoing it and
    /// resets its session, so the host may send a fresh challenge whenever
    /// it might have been lost. Firmware can open the port first and drop a
    /// frame, and QEMU coalesces rapid `VSERPORT_CHANGE` close/open pairs.
    /// The host therefore sends a fresh challenge on every reported open of
    /// its port (including a repeated open) and on a 1/2/4/8 s retry schedule
    /// while the port is not known to be closed; a close only pauses the
    /// retries. Each challenge is a fresh nonce written only to this boot's
    /// verified pipe, so an echo of any of them, with scripts available,
    /// proves the guest service is live in this boot. A challenge not issued
    /// in this boot, or a malformed or oversized frame, refuses. At most
    /// `MAX_CHALLENGES` are issued, no write happens at or after the fixed
    /// `GUEST_READY_BUDGET`, and there is no serial reconnect.
    pub async fn ready<R, W>(
        mut transport: T,
        qmp: &mut Qmp<R, W>,
        cancel: &Shutdown,
    ) -> Result<Self, ChannelError>
    where
        R: AsyncRead + Unpin,
        W: AsyncWrite + Unpin,
    {
        let ready_by = Instant::now() + GUEST_READY_BUDGET;
        let mut inbound = Inbound::default();
        let mut issued: Vec<String> = Vec::new();
        let accepted = bounded(cancel, ready_by, async {
            let mut watch = PortWatch::default();
            let mut delay = RETRY_FIRST;
            // With no event yet the port state is unknown, so retries run.
            let mut next_retry = Some(Instant::now() + RETRY_FIRST);
            loop {
                let retry_at = next_retry;
                let retry = async move {
                    match retry_at {
                        Some(at) => sleep_until(at).await,
                        None => std::future::pending().await,
                    }
                };
                let send = tokio::select! {
                    biased;
                    open = qmp.port_change(GUEST_PORT_ID, &mut watch) => {
                        if open? {
                            delay = RETRY_FIRST;
                            true
                        } else {
                            // A close only pauses retries until the next open.
                            next_retry = None;
                            false
                        }
                    }
                    reply = inbound.next_frame(&mut transport) => match reply? {
                        Response::HelloAck {
                            challenge,
                            script_execution: ScriptExecution::Available,
                            ..
                        } if issued.contains(&challenge) => return Ok(challenge),
                        _ => return Err(ChannelError::Unexpected),
                    },
                    () = retry => {
                        // The next retry waits twice as long, up to the cap.
                        delay = (delay * 2).min(RETRY_MAX);
                        true
                    }
                };
                if send {
                    if issued.len() == MAX_CHALLENGES {
                        return Err(ChannelError::Refused);
                    }
                    // No write at or after the absolute cutoff.
                    if Instant::now() >= ready_by {
                        return Err(ChannelError::Timeout);
                    }
                    issued.push(send_hello(&mut transport).await?);
                    next_retry = Some(Instant::now() + delay);
                }
            }
        })
        .await?;
        issued.retain(|challenge| *challenge != accepted);
        Ok(Self {
            transport: Some(transport),
            next_id: 1,
            inbound,
            stale: issued,
        })
    }

    pub async fn ping(&mut self, cancel: &Shutdown) -> Result<(), ChannelError> {
        let id = self.next_id;
        let (transport, response) = self.request(cancel, Request::Ping { id }, 5).await?;
        if !matches!(response, Response::Pong { id: echo } if echo == id) {
            return Err(ChannelError::Unexpected);
        }
        self.transport = Some(transport);
        Ok(())
    }

    pub async fn run_script(
        &mut self,
        cancel: &Shutdown,
        script: String,
        timeout_s: u32,
    ) -> Result<(ScriptStatus, Option<u32>), ChannelError> {
        let id = self.next_id;
        let request = Request::RunScript {
            id,
            script,
            timeout_s,
        };
        request.validate().map_err(|_| ChannelError::Invalid)?;
        let (transport, response) = self
            .request(cancel, request, u64::from(timeout_s) + 5)
            .await?;
        let Response::ScriptResult {
            id: echo,
            status,
            exit_code,
        } = response
        else {
            return Err(ChannelError::Unexpected);
        };
        if echo != id {
            return Err(ChannelError::Unexpected);
        }
        self.transport = Some(transport);
        Ok((status, exit_code))
    }

    async fn request(
        &mut self,
        cancel: &Shutdown,
        request: Request,
        seconds: u64,
    ) -> Result<(T, Response), ChannelError> {
        let mut transport = self.transport.take().ok_or(ChannelError::Closed)?;
        self.next_id = self.next_id.checked_add(1).ok_or(ChannelError::Closed)?;
        let inbound = &mut self.inbound;
        let stale = &mut self.stale;
        let response = bounded(
            cancel,
            Instant::now() + Duration::from_secs(seconds),
            async {
                send_frame(&mut transport, &request).await?;
                loop {
                    match inbound.next_frame(&mut transport).await? {
                        // A late ack for another of this boot's challenges, each
                        // skipped at most once.
                        Response::HelloAck { challenge, .. } if stale.contains(&challenge) => {
                            stale.retain(|issued| *issued != challenge);
                        }
                        response => return Ok(response),
                    }
                }
            },
        )
        .await?;
        Ok((transport, response))
    }
}

async fn send_frame(
    transport: &mut (impl AsyncWrite + Unpin),
    request: &Request,
) -> Result<(), ChannelError> {
    let mut frame = Vec::new();
    write_frame(&mut frame, request).map_err(|_| ChannelError::Invalid)?;
    transport
        .write_all(&frame)
        .await
        .map_err(|_| ChannelError::Io)?;
    transport.flush().await.map_err(|_| ChannelError::Io)
}

/// Sends one fresh 128-bit challenge and returns it.
async fn send_hello(transport: &mut (impl AsyncWrite + Unpin)) -> Result<String, ChannelError> {
    let mut random = [0; 16];
    getrandom::fill(&mut random).map_err(|_| ChannelError::Io)?;
    let challenge = hex::encode(random);
    send_frame(
        transport,
        &Request::Hello {
            version: PROTOCOL_VERSION,
            challenge: challenge.clone(),
        },
    )
    .await?;
    Ok(challenge)
}

/// Cancel-safe response framing for use inside `select!`. Bytes read so far
/// and the time the current frame started persist across dropped calls, and
/// bytes past one frame stay buffered for the next call.
#[derive(Default)]
struct Inbound {
    bytes: Vec<u8>,
    started: Option<Instant>,
}

impl Inbound {
    async fn next_frame(
        &mut self,
        transport: &mut (impl AsyncRead + Unpin),
    ) -> Result<Response, ChannelError> {
        loop {
            if self.bytes.len() >= 4 {
                let size = u32::from_be_bytes(self.bytes[..4].try_into().unwrap()) as usize;
                if !(1..=MAX_FRAME).contains(&size) {
                    return Err(ChannelError::Oversize);
                }
                if self.bytes.len() >= 4 + size {
                    let frame: Vec<u8> = self.bytes.drain(..4 + size).collect();
                    self.started = (!self.bytes.is_empty()).then(Instant::now);
                    return read_frame(&mut frame.as_slice())
                        .map_err(|_| ChannelError::Invalid)?
                        .ok_or(ChannelError::Eof);
                }
            }
            let mut chunk = [0; 4096];
            let count = match self.started {
                // Silence before a frame starts is bounded by the caller.
                None => transport.read(&mut chunk).await.map_err(read_error)?,
                Some(started) => {
                    let frame_by = started + FRAME_BOUND;
                    let count = timeout_at(frame_by, transport.read(&mut chunk))
                        .await
                        .map_err(|_| ChannelError::Timeout)?
                        .map_err(read_error)?;
                    if Instant::now() >= frame_by {
                        return Err(ChannelError::Timeout);
                    }
                    count
                }
            };
            if count == 0 {
                return Err(ChannelError::Eof);
            }
            self.started.get_or_insert_with(Instant::now);
            self.bytes.extend_from_slice(&chunk[..count]);
        }
    }
}

fn read_error(error: std::io::Error) -> ChannelError {
    if error.kind() == std::io::ErrorKind::UnexpectedEof {
        ChannelError::Eof
    } else {
        ChannelError::Io
    }
}
