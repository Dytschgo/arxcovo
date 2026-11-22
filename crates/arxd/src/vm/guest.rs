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
    time::{Instant, timeout_at},
};

/// Absolute budget, from the accepted startup `cont`, for the guest service
/// to open its port and answer a fresh challenge. Fixed: no recipe, profile
/// or caller can extend it.
pub const GUEST_READY_BUDGET: Duration = Duration::from_secs(180);

/// Once the first byte of a response arrives, the rest of that frame must
/// arrive within this bound, inside any outer deadline.
pub const FRAME_BOUND: Duration = Duration::from_secs(10);

/// Port opens accepted during readiness: firmware, the guest service and a
/// few service restarts. One more refuses.
pub const MAX_OPEN_CYCLES: u8 = 8;

/// Protocol state for a single authenticated transport. The Windows adapter
/// must verify the held server and this exact pipe handle before constructing it.
pub struct GuestChannel<T> {
    transport: Option<T>,
    next_id: u32,
}

impl<T: AsyncRead + AsyncWrite + Unpin> GuestChannel<T> {
    /// Installed-boot readiness, driven by QEMU's port-state events.
    ///
    /// The v1 protocol is host-initiated: the guest service reads exactly one
    /// `Hello` first and treats a second one as a protocol error. Firmware can
    /// open the virtio-serial port before Linux (OVMF's VirtioSerialDxe does),
    /// take a frame into its queue and reset that queue at ExitBootServices.
    /// So the host writes nothing until `VSERPORT_CHANGE` reports our fixed
    /// port open, and sends one fresh challenge per open. A close voids the
    /// outstanding challenge: an echo of any voided challenge is discarded and
    /// never accepted, and only an echo of the latest challenge, with scripts
    /// available, completes readiness. Everything stays inside the fixed
    /// `GUEST_READY_BUDGET` from the accepted `cont`, with at most
    /// `MAX_OPEN_CYCLES` opens. There is no serial reconnect.
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
        bounded(cancel, ready_by, async {
            let mut watch = PortWatch::default();
            let mut inbound = Inbound::default();
            let mut voided = Vec::new();
            let mut current: Option<String> = None;
            let mut opens = 0;
            loop {
                let Some(challenge) = &current else {
                    // Closed: the host never writes before the port opens.
                    if qmp.port_change(GUEST_PORT_ID, &mut watch).await? {
                        opens += 1;
                        if opens > MAX_OPEN_CYCLES {
                            return Err(ChannelError::Refused);
                        }
                        current = Some(send_hello(&mut transport).await?);
                    }
                    continue;
                };
                tokio::select! {
                    // A close observed together with a reply wins: refuse
                    // rather than accept an echo that may predate the close.
                    biased;
                    open = qmp.port_change(GUEST_PORT_ID, &mut watch) => {
                        if open? {
                            // QEMU reports changes only; open while open is not one.
                            return Err(ChannelError::Unexpected);
                        }
                        voided.push(current.take().expect("an open port has a challenge"));
                    }
                    reply = inbound.next_frame(&mut transport) => match reply? {
                        Response::HelloAck {
                            challenge: echo,
                            script_execution: ScriptExecution::Available,
                            ..
                        } if echo == *challenge => break,
                        // A late echo of a voided challenge, for example from a
                        // guest that read a stale buffered Hello, is discarded.
                        Response::HelloAck { challenge: echo, .. } if voided.contains(&echo) => {}
                        _ => return Err(ChannelError::Unexpected),
                    },
                }
            }
            Ok(())
        })
        .await?;
        Ok(Self {
            transport: Some(transport),
            next_id: 1,
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
        let response = bounded(
            cancel,
            Instant::now() + Duration::from_secs(seconds),
            exchange(&mut transport, &request),
        )
        .await?;
        Ok((transport, response))
    }
}

async fn exchange(
    transport: &mut (impl AsyncRead + AsyncWrite + Unpin),
    request: &Request,
) -> Result<Response, ChannelError> {
    let mut frame = Vec::new();
    write_frame(&mut frame, request).map_err(|_| ChannelError::Invalid)?;
    transport
        .write_all(&frame)
        .await
        .map_err(|_| ChannelError::Io)?;
    transport.flush().await.map_err(|_| ChannelError::Io)?;
    // Silence before the first byte is bounded only by the caller's deadline.
    let mut prefix = [0; 4];
    transport
        .read_exact(&mut prefix[..1])
        .await
        .map_err(read_error)?;
    let frame_by = Instant::now() + FRAME_BOUND;
    let rest = async {
        transport
            .read_exact(&mut prefix[1..])
            .await
            .map_err(read_error)?;
        let size = u32::from_be_bytes(prefix) as usize;
        if !(1..=MAX_FRAME).contains(&size) {
            return Err(ChannelError::Oversize);
        }
        frame.resize(size, 0);
        transport.read_exact(&mut frame).await.map_err(read_error)
    };
    timeout_at(frame_by, rest)
        .await
        .map_err(|_| ChannelError::Timeout)??;
    if Instant::now() >= frame_by {
        return Err(ChannelError::Timeout);
    }
    read_frame(&mut std::io::Read::chain(
        prefix.as_slice(),
        frame.as_slice(),
    ))
    .map_err(|_| ChannelError::Invalid)?
    .ok_or(ChannelError::Eof)
}

/// Sends one fresh 128-bit challenge and returns it.
async fn send_hello(transport: &mut (impl AsyncWrite + Unpin)) -> Result<String, ChannelError> {
    let mut random = [0; 16];
    getrandom::fill(&mut random).map_err(|_| ChannelError::Io)?;
    let challenge = hex::encode(random);
    let mut frame = Vec::new();
    write_frame(
        &mut frame,
        &Request::Hello {
            version: PROTOCOL_VERSION,
            challenge: challenge.clone(),
        },
    )
    .map_err(|_| ChannelError::Invalid)?;
    transport
        .write_all(&frame)
        .await
        .map_err(|_| ChannelError::Io)?;
    transport.flush().await.map_err(|_| ChannelError::Io)?;
    Ok(challenge)
}

/// Cancel-safe response framing for use inside `select!`. Bytes read so far
/// and the time the current frame started persist across dropped calls.
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
