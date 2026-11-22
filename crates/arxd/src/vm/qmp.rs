use crate::service::Shutdown;
use serde::Deserialize;
use serde_json::Value;
use std::{future::Future, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader},
    time::{Instant, timeout_at},
};

const MAX_RECORD: usize = 65_536;
const MAX_EVENTS: u8 = 64;
const SETUP: Duration = Duration::from_secs(10);
const POWERDOWN: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChannelError {
    Timeout,
    Cancelled,
    Io,
    Eof,
    Oversize,
    Invalid,
    Unexpected,
    Remote,
    Closed,
    /// The caller's live resource revalidation refused the transition.
    Refused,
}

pub(super) async fn bounded<T>(
    cancel: &Shutdown,
    deadline: Instant,
    action: impl Future<Output = Result<T, ChannelError>>,
) -> Result<T, ChannelError> {
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(ChannelError::Cancelled),
        result = timeout_at(deadline, action) => {
            let value = result.map_err(|_| ChannelError::Timeout)??;
            // Tokio polls ready I/O before an expired timer. The cutoff is
            // absolute, so a result completed at or after it still refuses.
            if Instant::now() >= deadline {
                return Err(ChannelError::Timeout);
            }
            Ok(value)
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum RunStatus {
    Prelaunch,
    Running,
    Paused,
    Shutdown,
    GuestPanicked,
    InternalError,
    IoError,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Status {
    running: bool,
    status: RunStatus,
    #[serde(rename = "singlestep")]
    _single_step: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Greeting {
    #[serde(rename = "QMP")]
    qmp: GreetingBody,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GreetingBody {
    version: Version,
    capabilities: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Version {
    qemu: Release,
    package: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Release {
    major: u16,
    minor: u16,
    micro: u16,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Record {
    Reply(Reply),
    Error(RemoteError),
    Event(Event),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    id: u16,
    #[serde(rename = "return")]
    result: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RemoteError {
    id: u16,
    error: ErrorBody,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ErrorBody {
    class: String,
    desc: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Event {
    event: String,
    data: Option<serde_json::Map<String, Value>>,
    timestamp: Option<Timestamp>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Timestamp {
    #[serde(rename = "seconds")]
    _seconds: i64,
    microseconds: i32,
}

impl Event {
    /// The envelope check shared by every event, port events included.
    fn envelope(&self) -> Result<(), ChannelError> {
        if self.event.is_empty()
            || self.event.len() > 64
            || self
                .timestamp
                .as_ref()
                .is_some_and(|t| !(-1..1_000_000).contains(&t.microseconds))
        {
            return Err(ChannelError::Invalid);
        }
        Ok(())
    }

    fn guest_shutdown(self) -> Result<bool, ChannelError> {
        self.envelope()?;
        if self.event != "SHUTDOWN" {
            return Ok(false);
        }
        let data = self.data.ok_or(ChannelError::Invalid)?;
        let guest = data
            .get("guest")
            .and_then(Value::as_bool)
            .ok_or(ChannelError::Invalid)?;
        let reason = data
            .get("reason")
            .and_then(Value::as_str)
            .ok_or(ChannelError::Invalid)?;
        Ok(guest && reason == "guest-shutdown")
    }
}

struct Stream<R, W> {
    reader: BufReader<R>,
    writer: W,
    next_id: u16,
    /// The record read so far. Persisting it makes `record` cancel-safe.
    pending: Vec<u8>,
}

impl<R: AsyncRead + Unpin, W: AsyncWrite + Unpin> Stream<R, W> {
    /// Cancel-safe: a dropped call keeps its partial record for the next one.
    async fn record<T: serde::de::DeserializeOwned>(&mut self) -> Result<T, ChannelError> {
        loop {
            match self.reader.read_u8().await {
                Ok(b'\n') => break,
                Ok(_) if self.pending.len() == MAX_RECORD => return Err(ChannelError::Oversize),
                Ok(byte) => self.pending.push(byte),
                Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => {
                    return Err(ChannelError::Eof);
                }
                Err(_) => return Err(ChannelError::Io),
            }
        }
        let bytes = std::mem::take(&mut self.pending);
        serde_json::from_slice(&bytes).map_err(|_| ChannelError::Invalid)
    }

    async fn command(
        &mut self,
        command: Command,
        events: &mut Events,
    ) -> Result<Value, ChannelError> {
        let id = self.next_id;
        self.next_id = id.checked_add(1).ok_or(ChannelError::Closed)?;
        let name = match command {
            Command::Capabilities => "qmp_capabilities",
            Command::Status => "query-status",
            Command::Continue => "cont",
            Command::Powerdown => "system_powerdown",
        };
        let request = format!("{{\"execute\":\"{name}\",\"id\":{id}}}\r\n");
        self.writer
            .write_all(request.as_bytes())
            .await
            .map_err(|_| ChannelError::Io)?;
        self.writer.flush().await.map_err(|_| ChannelError::Io)?;
        loop {
            match self.record().await? {
                Record::Reply(reply) if reply.id == id => return Ok(reply.result),
                Record::Error(error) if error.id == id => {
                    if error.error.class.is_empty() || error.error.desc.is_empty() {
                        return Err(ChannelError::Invalid);
                    }
                    return Err(ChannelError::Remote);
                }
                Record::Event(event) => events.observe(event)?,
                _ => return Err(ChannelError::Unexpected),
            }
        }
    }
}

enum Command {
    Capabilities,
    Status,
    Continue,
    Powerdown,
}

#[derive(Default)]
struct Events {
    count: u8,
    guest_shutdown: bool,
}

impl Events {
    fn observe(&mut self, event: Event) -> Result<(), ChannelError> {
        if self.count == MAX_EVENTS {
            return Err(ChannelError::Oversize);
        }
        self.count += 1;
        self.guest_shutdown |= event.guest_shutdown()?;
        Ok(())
    }
}

/// Protocol engine. Native callers must supply the two owned parent stdio
/// streams from the same verified Job launch.
pub struct Qmp<R, W> {
    stream: Option<Stream<R, W>>,
    started: bool,
}

impl<R: AsyncRead + Unpin, W: AsyncWrite + Unpin> Qmp<R, W> {
    pub async fn handshake(reader: R, writer: W, cancel: &Shutdown) -> Result<Self, ChannelError> {
        let mut stream = Stream {
            reader: BufReader::new(reader),
            writer,
            next_id: 1,
            pending: Vec::new(),
        };
        bounded(cancel, Instant::now() + SETUP, async {
            let Greeting { qmp: greeting } = stream.record().await?;
            let release = greeting.version.qemu;
            if (release.major, release.minor, release.micro) != (11, 1, 3)
                || greeting.version.package.len() > 256
                || greeting.capabilities.len() > 16
                || greeting
                    .capabilities
                    .iter()
                    .any(|name| name.is_empty() || name.len() > 64)
            {
                return Err(ChannelError::Invalid);
            }
            let result = stream
                .command(Command::Capabilities, &mut Events::default())
                .await?;
            empty(result)
        })
        .await?;
        Ok(Self {
            stream: Some(stream),
            started: false,
        })
    }

    pub async fn status(&mut self, cancel: &Shutdown) -> Result<RunStatus, ChannelError> {
        let mut stream = self.stream.take().ok_or(ChannelError::Closed)?;
        let status = bounded(cancel, Instant::now() + SETUP, async {
            let result = stream
                .command(Command::Status, &mut Events::default())
                .await?;
            let status: Status =
                serde_json::from_value(result).map_err(|_| ChannelError::Invalid)?;
            if status.running != (status.status == RunStatus::Running) {
                return Err(ChannelError::Invalid);
            }
            Ok(status.status)
        })
        .await?;
        self.stream = Some(stream);
        Ok(status)
    }

    /// One-time release of the `-S` hold. `revalidate` runs after the
    /// prelaunch status receipt and before `cont`; `false` sends no `cont`
    /// and closes the channel.
    pub async fn release_startup_hold(
        &mut self,
        cancel: &Shutdown,
        revalidate: impl FnOnce() -> bool,
    ) -> Result<(), ChannelError> {
        let mut stream = self.stream.take().ok_or(ChannelError::Closed)?;
        if self.started {
            return Err(ChannelError::Unexpected);
        }
        bounded(cancel, Instant::now() + SETUP, async {
            let mut events = Events::default();
            let result = stream.command(Command::Status, &mut events).await?;
            let status: Status =
                serde_json::from_value(result).map_err(|_| ChannelError::Invalid)?;
            if status.running || status.status != RunStatus::Prelaunch || events.guest_shutdown {
                return Err(ChannelError::Unexpected);
            }
            if !revalidate() {
                return Err(ChannelError::Refused);
            }
            empty(stream.command(Command::Continue, &mut events).await?)?;
            if events.guest_shutdown {
                return Err(ChannelError::Unexpected);
            }
            Ok(())
        })
        .await?;
        self.stream = Some(stream);
        self.started = true;
        Ok(())
    }

    /// Consumes the channel. Success is guest evidence only, independent of
    /// process exit, Job-empty, worker joins and artifact release.
    pub async fn powerdown(mut self, cancel: &Shutdown) -> Result<(), ChannelError> {
        let mut stream = self.stream.take().ok_or(ChannelError::Closed)?;
        if !self.started {
            return Err(ChannelError::Unexpected);
        }
        bounded(cancel, Instant::now() + POWERDOWN, async {
            let mut events = Events::default();
            empty(stream.command(Command::Powerdown, &mut events).await?)?;
            while !events.guest_shutdown {
                match stream.record().await? {
                    Record::Event(event) => events.observe(event)?,
                    _ => return Err(ChannelError::Unexpected),
                }
            }
            Ok(())
        })
        .await
    }
}

/// Bounds the events a readiness wait may skip while watching one port.
#[derive(Default)]
pub struct PortWatch {
    skipped: u8,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PortChange {
    id: String,
    open: bool,
}

impl<R: AsyncRead + Unpin, W: AsyncWrite + Unpin> Qmp<R, W> {
    /// Next `VSERPORT_CHANGE` for the virtio-serial port device `port`, as
    /// `open`. Available only after the startup `cont`, while no command is
    /// outstanding, so any reply is unexpected. Events for other ports and
    /// other bounded events are skipped and counted; a malformed port event,
    /// any `SHUTDOWN`, or more than `MAX_EVENTS` skips refuses. Cancel-safe:
    /// the stream stays in place and partial records persist.
    pub async fn port_change(
        &mut self,
        port: &str,
        watch: &mut PortWatch,
    ) -> Result<bool, ChannelError> {
        if !self.started {
            return Err(ChannelError::Unexpected);
        }
        let stream = self.stream.as_mut().ok_or(ChannelError::Closed)?;
        loop {
            let Record::Event(event) = stream.record().await? else {
                return Err(ChannelError::Unexpected);
            };
            event.envelope()?;
            if event.event == "VSERPORT_CHANGE" {
                let data = event.data.ok_or(ChannelError::Invalid)?;
                let change: PortChange = serde_json::from_value(Value::Object(data))
                    .map_err(|_| ChannelError::Invalid)?;
                if change.id.is_empty() || change.id.len() > 128 {
                    return Err(ChannelError::Invalid);
                }
                if change.id == port {
                    return Ok(change.open);
                }
            } else if event.event == "SHUTDOWN" {
                event.guest_shutdown()?;
                return Err(ChannelError::Unexpected);
            } else {
                event.guest_shutdown()?;
            }
            if watch.skipped == MAX_EVENTS {
                return Err(ChannelError::Oversize);
            }
            watch.skipped += 1;
        }
    }
}

fn empty(value: Value) -> Result<(), ChannelError> {
    if value.as_object().is_some_and(serde_json::Map::is_empty) {
        Ok(())
    } else {
        Err(ChannelError::Invalid)
    }
}
