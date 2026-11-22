use super::qmp::{ChannelError, bounded};
use crate::service::Shutdown;
use arx_guest::protocol::{
    MAX_FRAME, Message, PROTOCOL_VERSION, Request, Response, ScriptExecution, ScriptStatus,
    read_frame, write_frame,
};
use std::time::Duration;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    time::Instant,
};

/// Absolute budget, from the accepted startup `cont`, for the guest service
/// to start and answer the boot's one fresh challenge. Fixed: no recipe,
/// profile or caller can extend it.
pub const GUEST_READY_BUDGET: Duration = Duration::from_secs(180);

/// Once the first byte of a response arrives, the rest of that frame must
/// arrive within this bound, inside any outer deadline.
pub const FRAME_BOUND: Duration = Duration::from_secs(10);

/// Protocol state for a single authenticated transport. The Windows adapter
/// must verify the held server and this exact pipe handle before constructing it.
pub struct GuestChannel<T> {
    transport: Option<T>,
    next_id: u32,
}

impl<T: AsyncRead + AsyncWrite + Unpin> GuestChannel<T> {
    /// Boot readiness. The v1 protocol is host-initiated: the guest service
    /// reads exactly one `Hello` first and treats a second one as a protocol
    /// error. So the host sends one fresh challenge and tolerates silence
    /// until `GUEST_READY_BUDGET`. Until the guest opens its virtio-serial
    /// port, QEMU leaves the frame unread in the host pipe, so a guest service
    /// that starts late still receives this exact challenge. Only an ack that
    /// echoes it is accepted; there is no retry and no reconnect.
    pub async fn handshake(mut transport: T, cancel: &Shutdown) -> Result<Self, ChannelError> {
        let ready_by = Instant::now() + GUEST_READY_BUDGET;
        let mut random = [0; 16];
        getrandom::fill(&mut random).map_err(|_| ChannelError::Io)?;
        let challenge = hex::encode(random);
        let request = Request::Hello {
            version: PROTOCOL_VERSION,
            challenge: challenge.clone(),
        };
        let response = bounded(cancel, ready_by, exchange(&mut transport, &request)).await?;
        if !matches!(response, Response::HelloAck {
            version: PROTOCOL_VERSION,
            challenge: echo,
            script_execution: ScriptExecution::Available,
        } if echo == challenge)
        {
            return Err(ChannelError::Unexpected);
        }
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
    tokio::time::timeout_at(frame_by, rest)
        .await
        .map_err(|_| ChannelError::Timeout)??;
    read_frame(&mut std::io::Read::chain(
        prefix.as_slice(),
        frame.as_slice(),
    ))
    .map_err(|_| ChannelError::Invalid)?
    .ok_or(ChannelError::Eof)
}

fn read_error(error: std::io::Error) -> ChannelError {
    if error.kind() == std::io::ErrorKind::UnexpectedEof {
        ChannelError::Eof
    } else {
        ChannelError::Io
    }
}
