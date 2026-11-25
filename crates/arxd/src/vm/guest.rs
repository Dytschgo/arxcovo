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
    async fn next_frame<M: Message>(
        &mut self,
        transport: &mut (impl AsyncRead + Unpin),
    ) -> Result<M, ChannelError> {
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

/// Compose-only channel over a pipe already verified by the boot supervisor.
/// A timeout, invalid frame or mismatched reply permanently closes the channel.
///
/// The inner state is held behind an `Arc<AsyncMutex<_>>` so lifecycle and
/// protected-backup workers serialize cleanly over the same physical serial
/// transport without opening a second guest pipe.
pub struct ComposeChannel {
    inner: std::sync::Arc<tokio::sync::Mutex<ComposeChannelInner>>,
    open: std::sync::Arc<std::sync::atomic::AtomicBool>,
    generation: u64,
}

struct ComposeChannelInner {
    transport: Option<Box<dyn ComposeTransport>>,
    inbound: Inbound,
    next_id: u32,
}

trait ComposeTransport: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> ComposeTransport for T {}

impl ComposeChannel {
    pub async fn negotiate(
        mut transport: impl AsyncRead + AsyncWrite + Unpin + Send + 'static,
        generation: u64,
        cancel: &Shutdown,
    ) -> Result<Self, ChannelError> {
        use arx_guest::compose::{
            COMPOSE_CAPABILITIES, COMPOSE_PROTOCOL_VERSION, ComposeRequest, ComposeResponse,
        };
        let mut random = [0; 16];
        getrandom::fill(&mut random).map_err(|_| ChannelError::Io)?;
        let challenge = hex::encode(random);
        let capabilities = COMPOSE_CAPABILITIES.map(str::to_owned).to_vec();
        let mut inbound = Inbound::default();
        let ack: ComposeResponse = bounded(cancel, Instant::now() + FRAME_BOUND, async {
            send_compose(
                &mut transport,
                ComposeRequest::ComposeHello {
                    version: COMPOSE_PROTOCOL_VERSION,
                    challenge: challenge.clone(),
                    generation,
                    capabilities: capabilities.clone(),
                },
            )
            .await?;
            inbound.next_frame(&mut transport).await
        })
        .await?;
        if !matches!(ack, ComposeResponse::ComposeHelloAck {
            version:COMPOSE_PROTOCOL_VERSION, challenge:echo, generation:boot, capabilities:offered, ..
        } if echo == challenge && boot == generation && offered == capabilities)
        {
            return Err(ChannelError::Unexpected);
        }
        Ok(Self {
            inner: std::sync::Arc::new(tokio::sync::Mutex::new(ComposeChannelInner {
                transport: Some(Box::new(transport)),
                inbound,
                next_id: 1,
            })),
            open: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)),
            generation,
        })
    }

    /// Returns a second handle referencing the same serialized physical pipe.
    #[allow(
        dead_code,
        reason = "reserved for qualified Compose lifecycle/backup transport sharing"
    )]
    pub(crate) fn shared(&self) -> Self {
        Self {
            inner: std::sync::Arc::clone(&self.inner),
            open: std::sync::Arc::clone(&self.open),
            generation: self.generation,
        }
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) fn is_open(&self) -> bool {
        self.open.load(std::sync::atomic::Ordering::Acquire)
    }

    pub(crate) async fn exchange(
        &mut self,
        cancel: &Shutdown,
        request: impl FnOnce(u32) -> arx_guest::compose::ComposeRequest,
    ) -> anyhow::Result<arx_guest::compose::ComposeResponse> {
        self.exchange_until(cancel, Instant::now() + FRAME_BOUND, request)
            .await
            .map_err(|_| anyhow::anyhow!("Compose guest exchange unavailable"))
    }

    pub(crate) async fn exchange_until(
        &mut self,
        cancel: &Shutdown,
        deadline: Instant,
        request: impl FnOnce(u32) -> arx_guest::compose::ComposeRequest,
    ) -> Result<arx_guest::compose::ComposeResponse, ChannelError> {
        use arx_guest::compose::ComposeResponse::*;
        use std::sync::atomic::Ordering;

        struct CloseOnDrop<'a>(&'a std::sync::atomic::AtomicBool, bool);
        impl Drop for CloseOnDrop<'_> {
            fn drop(&mut self) {
                if self.1 {
                    self.0.store(false, Ordering::Release);
                }
            }
        }

        let mut state = bounded(cancel, deadline, async { Ok(self.inner.lock().await) }).await?;
        let Some(mut transport) = state.transport.take() else {
            self.open.store(false, Ordering::Release);
            return Err(ChannelError::Closed);
        };
        let mut guard = CloseOnDrop(&self.open, true);
        let id = state.next_id;
        state.next_id = id.checked_add(1).ok_or(ChannelError::Closed)?;
        let response = bounded(cancel, deadline, async {
            send_compose(&mut transport, request(id)).await?;
            state.inbound.next_frame(&mut transport).await
        })
        .await?;
        let echo = match &response {
            ComposeStageProgress { id, .. }
            | ComposeReleaseStaged { id, .. }
            | ComposeOperationState { id, .. }
            | ComposeProjectStatus { id, .. }
            | ComposeLogPage { id, .. }
            | ComposeRoutesInstalled { id, .. }
            | ComposeSecretAccepted { id, .. }
            | ComposeBackupReady { id, .. }
            | ComposeBackupData { id, .. }
            | ComposeBackupOccupied { id, .. }
            | ComposeBackupReleased { id, .. }
            | ComposeRejected { id, .. } => *id,
            ComposeHelloAck { .. } => return Err(ChannelError::Unexpected),
        };
        if echo != id {
            return Err(ChannelError::Unexpected);
        }
        state.transport = Some(transport);
        guard.1 = false;
        Ok(response)
    }
}

async fn send_compose(
    transport: &mut (impl AsyncWrite + Unpin),
    request: arx_guest::compose::ComposeRequest,
) -> Result<(), ChannelError> {
    use zeroize::Zeroizing;

    request.validate().map_err(|_| ChannelError::Invalid)?;
    // The generic guest write_frame uses a non-wiping stack buffer. This
    // writer owns a fixed, wiping frame, including after partial writes.
    let mut frame = Zeroizing::new(vec![0u8; 4 + MAX_FRAME]);
    let mut writer = std::io::Cursor::new(&mut frame[4..]);
    serde_json::to_writer(&mut writer, &request).map_err(|_| ChannelError::Invalid)?;
    let len = writer.position() as usize;
    drop(request);
    frame[..4].copy_from_slice(&(len as u32).to_be_bytes());
    transport
        .write_all(&frame[..4 + len])
        .await
        .map_err(|_| ChannelError::Io)?;
    drop(frame);
    transport.flush().await.map_err(|_| ChannelError::Io)
}
#[cfg(any(test, all(windows, target_arch = "x86_64")))]
pub(super) mod network_probe {
    #[cfg(test)]
    use serde::Deserialize;
    #[cfg(test)]
    use std::{
        collections::BTreeSet,
        net::{IpAddr, Ipv4Addr, SocketAddr},
    };
    use tokio::io::{AsyncRead, AsyncReadExt};

    use super::ChannelError;
    use crate::vm::qualification_record::{NetworkProbeReport, ProbeOutcome};

    #[cfg(any(test, all(windows, target_arch = "x86_64")))]
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(in crate::vm) struct WitnessReport {
        pub diagnostic: NetworkProbeReport,
        pub gateway_reachable: bool,
        pub validated_attempts: u8,
    }

    #[cfg(any(test, all(windows, target_arch = "x86_64")))]
    pub(in crate::vm) async fn witness_response(
        mut source: impl AsyncRead + Unpin,
        nonce: &[u8; 16],
    ) -> Result<WitnessReport, ChannelError> {
        let mut bytes = [0; 57];
        source
            .read_exact(&mut bytes)
            .await
            .map_err(|_| ChannelError::Eof)?;
        let net = std::str::from_utf8(&bytes).map_err(|_| ChannelError::Invalid)?;
        let net = net
            .strip_prefix("ARXNET3 ")
            .and_then(|value| value.strip_suffix('\n'))
            .ok_or(ChannelError::Invalid)?;
        let mut parts = net.split(' ');
        let (Some(received), Some(flags), Some(control), Some(outcomes), None) = (
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
        ) else {
            return Err(ChannelError::Invalid);
        };
        let flags: [u8; 4] = flags
            .as_bytes()
            .try_into()
            .map_err(|_| ChannelError::Invalid)?;
        let codes: [u8; 8] = outcomes
            .as_bytes()
            .try_into()
            .map_err(|_| ChannelError::Invalid)?;
        if received != hex::encode(nonce)
            || !flags.iter().all(|flag| matches!(flag, b'0' | b'1'))
            || !matches!(control, "0" | "1")
            || (flags != *b"1111" && (control != "0" || codes != *b"EEEEEEEE"))
            || !codes
                .iter()
                .all(|code| matches!(code, b'R' | b'F' | b'T' | b'S' | b'L' | b'E' | b'M'))
            || codes[..6].contains(&b'M')
        {
            return Err(ChannelError::Unexpected);
        }
        let validated_attempts = codes
            .into_iter()
            .enumerate()
            .filter(|(index, code)| match index {
                0 | 2 => matches!(code, b'F' | b'T'),
                1 | 4 => *code == b'L',
                _ => *code == b'S',
            })
            .count() as u8;
        let [nic_ready, ipv4_lease, default_route, no_ipv6_egress_route] = flags.map(|b| b == b'1');
        let outcomes = codes.map(|code| match code {
            b'R' => ProbeOutcome::Reachable,
            b'M' => ProbeOutcome::MalformedReply,
            b'S' => ProbeOutcome::DatagramSent,
            b'L' => ProbeOutcome::Ipv6PathAbsent,
            b'T' => ProbeOutcome::TimedOut,
            b'F' => ProbeOutcome::RefusedOrReset,
            _ if flags != *b"1111" => ProbeOutcome::NotAttempted,
            _ => ProbeOutcome::SocketError,
        });
        Ok(WitnessReport {
            diagnostic: NetworkProbeReport {
                nic_ready,
                ipv4_lease,
                default_route,
                no_ipv6_egress_route,
                outcomes,
            },
            gateway_reachable: control == "1",
            validated_attempts,
        })
    }

    /// Enrolled witness sockets. The alias control is on the host loopback
    /// and is addressed by its slirp alias only from the probe guest.
    #[cfg(test)]
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(in crate::vm) struct Targets {
        pub ipv4: SocketAddr,
        pub ipv6: SocketAddr,
        pub host: SocketAddr,
        pub host_alias_port: u16,
        pub management: SocketAddr,
        pub public: SocketAddr,
        pub dns: SocketAddr,
    }

    #[cfg(test)]
    impl Targets {
        pub(in crate::vm) fn guest_tcp(&self) -> [SocketAddr; 6] {
            [
                self.ipv4,
                self.ipv6,
                self.host,
                (Ipv4Addr::new(10, 0, 2, 2), self.host_alias_port).into(),
                self.management,
                self.public,
            ]
        }

        pub(in crate::vm) fn valid(&self) -> bool {
            let tcp = self.guest_tcp();
            let v4 = |address: SocketAddr| match address.ip() {
                IpAddr::V4(ip) => Some(ip),
                IpAddr::V6(_) => None,
            };
            let is_slirp = |ip: Ipv4Addr| {
                let [a, b, c, _] = ip.octets();
                a == 10 && b == 0 && c == 2
            };
            let is_private_v4 = |ip: Ipv4Addr| {
                ip.is_private()
                    || ip.is_link_local()
                    || matches!(ip.octets(), [100, b, ..] if (64..=127).contains(&b))
            };
            let Some(ipv4) = v4(self.ipv4) else {
                return false;
            };
            let Some(host) = v4(self.host) else {
                return false;
            };
            let Some(management) = v4(self.management) else {
                return false;
            };
            let Some(public) = v4(self.public) else {
                return false;
            };
            let dns_not_slirp = !matches!(self.dns.ip(), IpAddr::V4(ip) if is_slirp(ip));
            self.ipv6.is_ipv6()
                && !self.ipv6.ip().is_loopback()
                && !ipv4.is_loopback()
                && !is_slirp(ipv4)
                && !host.is_loopback()
                && !is_slirp(host)
                && !management.is_loopback()
                && !is_slirp(management)
                && is_private_v4(management)
                && !public.is_loopback()
                && !is_slirp(public)
                && !is_private_v4(public)
                && self.dns.port() == 53
                && dns_not_slirp
                && tcp.into_iter().chain([self.dns]).all(valid_socket)
                && tcp.into_iter().collect::<BTreeSet<_>>().len() == tcp.len()
        }

        /// Bind the enrolled DNS witness to a real host resolver backing the
        /// slirp `10.0.2.3` alias.
        pub(in crate::vm) fn valid_with_resolvers(&self, host_resolvers: &[IpAddr]) -> bool {
            self.valid() && host_resolvers.contains(&self.dns.ip())
        }
    }

    #[cfg(test)]
    fn valid_socket(address: SocketAddr) -> bool {
        address.port() != 0
            && !address.ip().is_unspecified()
            && !address.ip().is_multicast()
            && !matches!(address.ip(), IpAddr::V4(ip) if ip == Ipv4Addr::BROADCAST)
    }

    pub(in crate::vm) fn dns_query(nonce: &[u8; 16]) -> Vec<u8> {
        let mut packet = Vec::from(&nonce[..2]);
        packet.extend_from_slice(&[1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 36]);
        packet.extend_from_slice(b"arx-");
        packet.extend_from_slice(hex::encode(nonce).as_bytes());
        packet.extend_from_slice(b"\x07invalid\x00\x00\x01\x00\x01");
        packet
    }

    /// Require a full, non-truncated, standard DNS reply whose transaction ID
    /// and echoed question section match the exact nonce-bound query.
    #[cfg(test)]
    pub(in crate::vm) fn validate_dns_reply(query: &[u8], reply: &[u8]) -> bool {
        if query.len() < 12 || reply.len() < query.len() {
            return false;
        }
        let qr = reply[2] & 0x80 != 0;
        let opcode = (reply[2] >> 3) & 0x0f;
        let truncated = reply[2] & 0x02 != 0;
        let rcode = reply[3] & 0x0f;
        let qdcount = u16::from_be_bytes([reply[4], reply[5]]);
        reply[..2] == query[..2]
            && qr
            && opcode == 0
            && !truncated
            && matches!(rcode, 0 | 3)
            && qdcount == 1
            && reply[12..query.len()] == query[12..]
    }

    /// One exact structured response, bound to the seed. The enclosing caller
    /// holds the PID/Job-verified pipe and imposes the absolute read deadline.
    #[cfg(test)]
    pub(in crate::vm) async fn response(
        mut source: impl AsyncRead + Unpin,
        nonce: &[u8; 16],
    ) -> Result<NetworkProbeReport, ChannelError> {
        let mut buffer = [0; 55];
        source
            .read_exact(&mut buffer)
            .await
            .map_err(|_| ChannelError::Eof)?;
        let text = std::str::from_utf8(&buffer).map_err(|_| ChannelError::Invalid)?;
        let body = text
            .strip_prefix("ARXNET2 ")
            .and_then(|rest| rest.strip_suffix('\n'))
            .ok_or(ChannelError::Invalid)?;
        let mut parts = body.split(' ');
        let (Some(got_nonce), Some(setup), Some(outcomes), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(ChannelError::Invalid);
        };
        let flags: [u8; 4] = setup
            .as_bytes()
            .try_into()
            .map_err(|_| ChannelError::Unexpected)?;
        let codes: [u8; 8] = outcomes
            .as_bytes()
            .try_into()
            .map_err(|_| ChannelError::Unexpected)?;
        if got_nonce != hex::encode(nonce) || !flags.iter().all(|&b| matches!(b, b'0' | b'1')) {
            return Err(ChannelError::Unexpected);
        }
        let setup_ready = flags == *b"1111";
        if (!setup_ready && codes != *b"EEEEEEEE") || codes[..6].contains(&b'M') {
            return Err(ChannelError::Unexpected);
        }
        let mut outcomes = [ProbeOutcome::SocketError; 8];
        for (outcome, code) in outcomes.iter_mut().zip(codes) {
            *outcome = match code {
                b'E' if !setup_ready => ProbeOutcome::NotAttempted,
                b'E' => ProbeOutcome::SocketError,
                b'R' => ProbeOutcome::Reachable,
                b'M' => ProbeOutcome::MalformedReply,
                _ => return Err(ChannelError::Unexpected),
            };
        }
        let [nic_ready, ipv4_lease, default_route, no_ipv6_egress_route] = flags.map(|b| b == b'1');
        Ok(NetworkProbeReport {
            nic_ready,
            ipv4_lease,
            default_route,
            no_ipv6_egress_route,
            outcomes,
        })
    }

    #[cfg(test)]
    pub(in crate::vm) fn fixture_targets() -> Targets {
        serde_json::from_str(
            r#"{
            "ipv4":"192.0.2.10:8001","ipv6":"[2001:db8::2]:8002",
            "host":"192.0.2.11:8003","host_alias_port":8004,
            "management":"10.0.1.2:8005","public":"192.0.2.13:8006",
            "dns":"192.0.2.14:53"
        }"#,
        )
        .unwrap()
    }

    #[cfg(test)]
    #[tokio::test]
    async fn restricted_network_response_preserves_observations_without_claiming_denials() {
        let nonce = [42; 16];
        let targets = fixture_targets();
        assert!(targets.valid());
        assert!(targets.valid_with_resolvers(&[IpAddr::V4(Ipv4Addr::new(192, 0, 2, 14))]));
        assert!(!targets.valid_with_resolvers(&[IpAddr::V4(Ipv4Addr::new(192, 0, 2, 99))]));
        let frame =
            |nonce, setup, outcomes| format!("ARXNET2 {} {setup} {outcomes}\n", hex::encode(nonce));
        assert_eq!(
            response(frame(nonce, "1111", "EREEEEMR").as_bytes(), &nonce).await,
            Ok(NetworkProbeReport {
                nic_ready: true,
                ipv4_lease: true,
                default_route: true,
                no_ipv6_egress_route: true,
                outcomes: [
                    ProbeOutcome::SocketError,
                    ProbeOutcome::Reachable,
                    ProbeOutcome::SocketError,
                    ProbeOutcome::SocketError,
                    ProbeOutcome::SocketError,
                    ProbeOutcome::SocketError,
                    ProbeOutcome::MalformedReply,
                    ProbeOutcome::Reachable,
                ],
            })
        );
        for setup in ["1001", "1101", "1110", "0000"] {
            let report = response(frame(nonce, setup, "EEEEEEEE").as_bytes(), &nonce)
                .await
                .unwrap();
            assert_eq!(
                [
                    report.nic_ready,
                    report.ipv4_lease,
                    report.default_route,
                    report.no_ipv6_egress_route
                ],
                std::array::from_fn(|index| setup.as_bytes()[index] == b'1'),
            );
            assert_eq!(report.outcomes, [ProbeOutcome::NotAttempted; 8]);
        }
        for (bad_nonce, setup, outcomes) in [
            ([14; 16], "1111", "EEEEEEEE"),
            (nonce, "1111", "DDDDDDDD"), // obsolete denial codes
            (nonce, "1011", "REEEEEEE"), // probe with incomplete setup
            (nonce, "1111", "MEEEEEEE"), // TCP never emits a DNS parse result
            (nonce, "11x1", "EEEEEEEE"),
        ] {
            assert_eq!(
                response(frame(bad_nonce, setup, outcomes).as_bytes(), &nonce).await,
                Err(ChannelError::Unexpected),
                "{setup} {outcomes}"
            );
        }
        assert_eq!(
            response(b"ARXNET2 ".as_slice(), &nonce).await,
            Err(ChannelError::Eof)
        );
    }

    #[cfg(test)]
    #[test]
    fn dns_reply_validation_requires_exact_nonce_question_and_header() {
        let nonce = [0x5a; 16];
        let query = dns_query(&nonce);
        let mut reply = query.clone();
        reply[2] = 0x81; // QR=1, opcode=0, RD=1
        reply[3] = 0x83; // RA=1, NXDOMAIN
        assert!(validate_dns_reply(&query, &reply));
        let mut noerror = reply.clone();
        noerror[3] = 0x80;
        assert!(validate_dns_reply(&query, &noerror));
        // Wrong txid, QR=0, truncated, SERVFAIL, QDCOUNT=0, mutated question, short packet.
        for mutate in [
            |r: &mut Vec<u8>| r[0] ^= 1,
            |r: &mut Vec<u8>| r[2] &= 0x7f,
            |r: &mut Vec<u8>| r[2] |= 0x02,
            |r: &mut Vec<u8>| r[3] = 0x82,
            |r: &mut Vec<u8>| r[5] = 0,
            |r: &mut Vec<u8>| r[20] ^= 1,
            |r: &mut Vec<u8>| r.truncate(12),
        ] {
            let mut bad = reply.clone();
            mutate(&mut bad);
            assert!(!validate_dns_reply(&query, &bad));
        }
    }

    #[cfg(test)]
    #[test]
    fn target_validation_enforces_roles_and_rejects_slirp_and_wrong_dns_port() {
        let mut bad_public = fixture_targets();
        bad_public.public = "10.0.1.9:8006".parse().unwrap();
        assert!(!bad_public.valid());

        let mut bad_mgmt = fixture_targets();
        bad_mgmt.management = "192.0.2.50:8005".parse().unwrap();
        assert!(!bad_mgmt.valid());

        let mut bad_slirp = fixture_targets();
        bad_slirp.host = "10.0.2.99:8003".parse().unwrap();
        assert!(!bad_slirp.valid());

        let mut bad_dns_port = fixture_targets();
        bad_dns_port.dns = "192.0.2.14:5353".parse().unwrap();
        assert!(!bad_dns_port.valid());
    }
}
