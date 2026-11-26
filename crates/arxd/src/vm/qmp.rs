use super::{
    qemu_args::QEMU_VERSION,
    qualification_record::{PausedCheck, PausedQmpStep},
    runtime::{InstallerPhase, InstallerStage},
};
use crate::service::Shutdown;
use serde::{Deserialize, Serialize};
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
const INSTALLER_STOP_GRACE: Duration = Duration::from_secs(2);
const SETUP_PROGRESS: Duration = Duration::from_secs(60 * 60);
const MAX_SETUP_RESETS: u8 = 16;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
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

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
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
    id: u64,
    #[serde(rename = "return")]
    result: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RemoteError {
    id: u64,
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

    fn guest_shutdown(&self) -> Result<bool, ChannelError> {
        self.envelope()?;
        if self.event != "SHUTDOWN" {
            return Ok(false);
        }
        let data = self.data.as_ref().ok_or(ChannelError::Invalid)?;
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
    next_id: u64,
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
        command: Command<'_>,
        events: &mut Events,
    ) -> Result<Value, ChannelError> {
        let id = self.next_id;
        self.next_id = id.checked_add(1).ok_or(ChannelError::Closed)?;
        let (name, arguments) = match command {
            Command::Capabilities => ("qmp_capabilities", None),
            Command::Status => ("query-status", None),
            #[cfg(all(windows, target_arch = "x86_64"))]
            Command::Accelerators => ("query-accelerators", None),
            Command::Continue => ("cont", None),
            Command::Powerdown => ("system_powerdown", None),
            #[cfg(any(test, all(windows, target_arch = "x86_64")))]
            Command::Quit => ("quit", None),
            Command::Screendump(filename) => (
                "screendump",
                Some(serde_json::json!({
                    "filename": filename, "format": "png", "device": "arxdisplay0", "head": 0
                })),
            ),
        };
        let request = match arguments {
            Some(arguments) => {
                format!("{{\"execute\":\"{name}\",\"id\":{id},\"arguments\":{arguments}}}\r\n")
            }
            None => format!("{{\"execute\":\"{name}\",\"id\":{id}}}\r\n"),
        };
        self.writer
            .write_all(request.as_bytes())
            .await
            .map_err(|_| ChannelError::Io)?;
        self.writer.flush().await.map_err(|_| ChannelError::Io)?;
        loop {
            let record = if let Some(installer) = &events.installer {
                let deadline = installer.deadline();
                let record = timeout_at(deadline, self.record())
                    .await
                    .map_err(|_| ChannelError::Timeout)??;
                if Instant::now() >= deadline {
                    return Err(ChannelError::Timeout);
                }
                record
            } else {
                self.record().await?
            };
            match record {
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

enum Command<'a> {
    Capabilities,
    Status,
    #[cfg(all(windows, target_arch = "x86_64"))]
    Accelerators,
    Continue,
    Powerdown,
    #[cfg(any(test, all(windows, target_arch = "x86_64")))]
    Quit,
    Screendump(&'a str),
}

#[derive(Default)]
struct Events {
    count: u8,
    guest_shutdown: bool,
    installer: Option<InstallerEvents>,
}

impl Events {
    fn observe(&mut self, event: Event) -> Result<(), ChannelError> {
        if self.count == MAX_EVENTS {
            return Err(ChannelError::Oversize);
        }
        self.count += 1;
        if let Some(installer) = &mut self.installer {
            self.guest_shutdown |= installer.observe(event)?;
            return Ok(());
        }
        self.guest_shutdown |= event.guest_shutdown()?;
        Ok(())
    }
}

struct InstallerEvents {
    stage: InstallerStage,
    progress: Option<Instant>,
    stop_by: Option<Instant>,
    resets: u8,
    active: bool,
}

impl InstallerEvents {
    fn start(&mut self) {
        self.active = true;
        if self.stage.phase() == InstallerPhase::WindowsSetup {
            self.progress = Some(Instant::now() + SETUP_PROGRESS);
        }
    }

    fn deadline(&self) -> Instant {
        [self.progress, self.stop_by]
            .into_iter()
            .flatten()
            .fold(self.stage.deadline(), Instant::min)
    }

    fn observe(&mut self, event: Event) -> Result<bool, ChannelError> {
        event.envelope()?;
        match event.event.as_str() {
            "SHUTDOWN" if self.active && event.guest_shutdown()? => Ok(true),
            "RESET" if self.active && self.stop_by.is_none() => {
                let data = event.data.ok_or(ChannelError::Invalid)?;
                if self.stage.phase() != InstallerPhase::WindowsSetup
                    || self.resets == MAX_SETUP_RESETS
                    || data.get("guest").and_then(Value::as_bool) != Some(true)
                    || data.get("reason").and_then(Value::as_str) != Some("guest-reset")
                {
                    return Err(ChannelError::Unexpected);
                }
                self.resets += 1;
                self.progress = Some(Instant::now() + SETUP_PROGRESS);
                Ok(false)
            }
            "STOP" if self.active && self.stop_by.is_none() && event.data.is_none() => {
                self.stop_by = Some(Instant::now() + INSTALLER_STOP_GRACE);
                Ok(false)
            }
            "RESET" | "SHUTDOWN" | "STOP" | "GUEST_PANICKED" | "GUEST_CRASHLOADED" | "WATCHDOG"
            | "BLOCK_IO_ERROR" | "SUSPEND" | "SUSPEND_DISK" => Err(ChannelError::Unexpected),
            _ => Ok(false),
        }
    }
}

fn verify_greeting(greeting: GreetingBody) -> Result<(), PausedCheck> {
    const MAX_PACKAGE_BYTES: usize = 256;
    const MAX_CAPABILITIES: usize = 16;
    const MAX_CAPABILITY_BYTES: usize = 64;
    let release = greeting.version.qemu;
    if format!("{}.{}.{}", release.major, release.minor, release.micro) != QEMU_VERSION {
        let mut required = QEMU_VERSION.split('.');
        return Err(PausedCheck::QmpGreetingVersion {
            measured: [release.major, release.minor, release.micro],
            required: std::array::from_fn(|_| {
                required
                    .next()
                    .and_then(|part| part.parse().ok())
                    .expect("pinned QEMU version")
            }),
        });
    }
    if greeting.version.package.len() > MAX_PACKAGE_BYTES {
        return Err(PausedCheck::QmpGreetingPackageBytes {
            measured_bytes: greeting.version.package.len() as u32,
            maximum_bytes: MAX_PACKAGE_BYTES as u32,
        });
    }
    if greeting.capabilities.len() > MAX_CAPABILITIES {
        return Err(PausedCheck::QmpGreetingCapabilityCount {
            measured_count: greeting.capabilities.len() as u32,
            maximum_count: MAX_CAPABILITIES as u32,
        });
    }
    if let Some(length) = greeting
        .capabilities
        .iter()
        .map(String::len)
        .find(|length| !(1..=MAX_CAPABILITY_BYTES).contains(length))
    {
        return Err(PausedCheck::QmpGreetingCapabilityBytes {
            measured_bytes: length as u32,
            minimum_bytes: 1,
            maximum_bytes: MAX_CAPABILITY_BYTES as u32,
        });
    }
    Ok(())
}

/// Protocol engine. Native callers must supply the two owned parent stdio
/// streams from the same verified Job launch.
pub struct Qmp<R, W> {
    stream: Option<Stream<R, W>>,
    started: bool,
    installer: Option<Events>,
}

impl<R: AsyncRead + Unpin, W: AsyncWrite + Unpin> Qmp<R, W> {
    pub(super) async fn screendump(&mut self, filename: &str) -> Result<(), ChannelError> {
        if self.installer.is_some() {
            return Err(ChannelError::Unexpected);
        }
        let mut stream = self.stream.take().ok_or(ChannelError::Closed)?;
        let result = bounded(
            &Shutdown::default(),
            Instant::now() + Duration::from_secs(2),
            async {
                empty(
                    stream
                        .command(Command::Screendump(filename), &mut Events::default())
                        .await?,
                )
            },
        )
        .await;
        if result.is_ok() || result == Err(ChannelError::Remote) {
            self.stream = Some(stream);
        }
        result
    }

    pub async fn handshake(reader: R, writer: W, cancel: &Shutdown) -> Result<Self, ChannelError> {
        Self::handshake_for(reader, writer, cancel, None, || {}, |_| {}).await
    }

    pub async fn installer_handshake(
        reader: R,
        writer: W,
        cancel: &Shutdown,
        stage: InstallerStage,
    ) -> Result<Self, ChannelError> {
        Self::handshake_for(reader, writer, cancel, Some(stage), || {}, |_| {}).await
    }

    #[cfg(all(windows, target_arch = "x86_64"))]
    pub(super) async fn handshake_diagnosed(
        reader: R,
        writer: W,
        cancel: &Shutdown,
        on_capabilities: impl FnOnce(),
        report: impl FnMut(PausedCheck),
    ) -> Result<Self, ChannelError> {
        Self::handshake_for(reader, writer, cancel, None, on_capabilities, report).await
    }

    async fn handshake_for(
        reader: R,
        writer: W,
        cancel: &Shutdown,
        stage: Option<InstallerStage>,
        on_capabilities: impl FnOnce(),
        mut report: impl FnMut(PausedCheck),
    ) -> Result<Self, ChannelError> {
        let mut stream = Stream {
            reader: BufReader::new(reader),
            writer,
            next_id: 1,
            pending: Vec::new(),
        };
        let mut events = Events {
            installer: stage.map(|stage| InstallerEvents {
                stage,
                progress: None,
                stop_by: None,
                resets: 0,
                active: false,
            }),
            ..Events::default()
        };
        let deadline = setup_deadline(stage);
        bounded(cancel, deadline, async {
            let Greeting { qmp: greeting } = stream.record().await.inspect_err(|error| {
                report(PausedCheck::QmpChannel {
                    step: PausedQmpStep::Greeting,
                    error: *error,
                });
            })?;
            if let Err(failure) = verify_greeting(greeting) {
                report(failure);
                return Err(ChannelError::Invalid);
            }
            on_capabilities();
            stream
                .command(Command::Capabilities, &mut events)
                .await
                .and_then(empty)
                .inspect_err(|error| {
                    report(PausedCheck::QmpChannel {
                        step: PausedQmpStep::Capabilities,
                        error: *error,
                    });
                })
        })
        .await?;
        Ok(Self {
            stream: Some(stream),
            started: false,
            installer: stage.map(|_| events),
        })
    }

    pub async fn status(&mut self, cancel: &Shutdown) -> Result<RunStatus, ChannelError> {
        if self.installer.is_some() {
            return Err(ChannelError::Unexpected);
        }
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

    /// Checks which accelerator the held paused QEMU child actually initialized.
    #[cfg(all(windows, target_arch = "x86_64"))]
    pub(super) async fn verify_whpx(&mut self, cancel: &Shutdown) -> Result<(), ChannelError> {
        self.verify_whpx_diagnosed(cancel, |_| {}).await
    }

    /// As `verify_whpx`, reporting the specific accelerator refusal. The query
    /// keeps the setup deadline and cancellation, and a refusal consumes the
    /// stream.
    #[cfg(all(windows, target_arch = "x86_64"))]
    pub(super) async fn verify_whpx_diagnosed(
        &mut self,
        cancel: &Shutdown,
        report: impl FnOnce(PausedCheck),
    ) -> Result<(), ChannelError> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Accelerators {
            enabled: String,
            present: Vec<String>,
        }

        let mut events = self.installer.take().unwrap_or_default();
        let deadline = setup_deadline(events.installer.as_ref().map(|installer| installer.stage));
        let mut stream = self.stream.take().ok_or(ChannelError::Closed)?;
        let response = bounded(cancel, deadline, async {
            stream.command(Command::Accelerators, &mut events).await
        })
        .await?;
        let actual: Accelerators =
            serde_json::from_value(response).map_err(|_| ChannelError::Invalid)?;
        const MAX_ACCELERATORS: usize = 8;
        let whpx_count = actual
            .present
            .iter()
            .filter(|name| name.as_str() == "whpx")
            .count();
        let failure = if actual.enabled != "whpx" {
            PausedCheck::WhpxNotEnabled
        } else if actual.present.len() > MAX_ACCELERATORS {
            PausedCheck::WhpxAcceleratorCount {
                measured_count: actual.present.len() as u32,
                maximum_count: MAX_ACCELERATORS as u32,
            }
        } else if whpx_count != 1 {
            PausedCheck::WhpxPresenceCount {
                measured_count: whpx_count as u32,
                required_count: 1,
            }
        } else if actual.present.iter().any(|name| {
            !matches!(
                name.as_str(),
                "hvf" | "kvm" | "mshv" | "nvmm" | "qtest" | "tcg" | "whpx" | "xen"
            )
        }) {
            PausedCheck::WhpxUnknownAccelerator
        } else {
            self.installer = events.installer.is_some().then_some(events);
            self.stream = Some(stream);
            return Ok(());
        };
        report(failure);
        Err(ChannelError::Unexpected)
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
        let mut events = self.installer.take().unwrap_or_default();
        let deadline = setup_deadline(events.installer.as_ref().map(|events| events.stage));
        bounded(cancel, deadline, async {
            let result = stream.command(Command::Status, &mut events).await?;
            let status: Status =
                serde_json::from_value(result).map_err(|_| ChannelError::Invalid)?;
            if status.running || status.status != RunStatus::Prelaunch || events.guest_shutdown {
                return Err(ChannelError::Unexpected);
            }
            if events.installer.is_some() && Instant::now() >= deadline {
                return Err(ChannelError::Timeout);
            }
            if !revalidate() {
                return Err(ChannelError::Refused);
            }
            if let Some(installer) = &mut events.installer {
                if Instant::now() >= deadline {
                    return Err(ChannelError::Timeout);
                }
                installer.start();
            }
            empty(stream.command(Command::Continue, &mut events).await?)?;
            if events.installer.is_none() && events.guest_shutdown {
                return Err(ChannelError::Unexpected);
            }
            Ok(())
        })
        .await?;
        self.installer = events.installer.is_some().then_some(events);
        self.stream = Some(stream);
        self.started = true;
        Ok(())
    }

    /// Observe guest poweroff with no command and keep the owned stream
    /// open until the caller has reaped the child. STOP alone is insufficient.
    pub async fn observe_installer_shutdown(
        &mut self,
        cancel: &Shutdown,
    ) -> Result<(), ChannelError> {
        let events = self.installer.as_mut().ok_or(ChannelError::Unexpected)?;
        let stream = self.stream.as_mut().ok_or(ChannelError::Closed)?;
        if !self.started {
            return Err(ChannelError::Unexpected);
        }
        while !events.guest_shutdown {
            let installer = events.installer.as_ref().ok_or(ChannelError::Unexpected)?;
            let deadline = installer.deadline();
            if Instant::now() >= deadline {
                return Err(ChannelError::Timeout);
            }
            let Record::Event(event) = bounded(cancel, deadline, stream.record()).await? else {
                return Err(ChannelError::Unexpected);
            };
            events.observe(event)?;
        }
        if cancel.is_cancelled() {
            return Err(ChannelError::Cancelled);
        }
        let installer = events.installer.as_ref().ok_or(ChannelError::Unexpected)?;
        if Instant::now() >= installer.deadline() {
            return Err(ChannelError::Timeout);
        }
        Ok(())
    }

    /// QEMU's control-plane exit for the diskless firmware probe. Require
    /// the matching empty reply and a clean stream EOF inside one cutoff.
    /// The caller still owns all process, Job and writer release checks.
    #[cfg(any(test, all(windows, target_arch = "x86_64")))]
    pub(super) async fn quit(self, cancel: &Shutdown, cutoff: Instant) -> Result<(), ChannelError> {
        if !self.started || self.installer.is_some() {
            return Err(ChannelError::Unexpected);
        }
        let mut stream = self.stream.ok_or(ChannelError::Closed)?;
        let deadline = cutoff.min(Instant::now() + INSTALLER_STOP_GRACE);
        bounded(cancel, deadline, async {
            let mut events = Events::default();
            empty(stream.command(Command::Quit, &mut events).await?)?;
            loop {
                match stream.record().await {
                    Ok(Record::Event(event)) => events.observe(event)?,
                    Err(ChannelError::Eof)
                        if stream.pending.is_empty() && !events.guest_shutdown =>
                    {
                        return Ok(());
                    }
                    Err(error) => return Err(error),
                    Ok(_) => return Err(ChannelError::Unexpected),
                }
            }
        })
        .await
    }

    /// Consumes the channel. Success is guest evidence only, independent of
    /// process exit, Job-empty, worker joins and artifact release.
    pub async fn powerdown(mut self, cancel: &Shutdown) -> Result<(), ChannelError> {
        let mut stream = self.stream.take().ok_or(ChannelError::Closed)?;
        if !self.started || self.installer.is_some() {
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

fn setup_deadline(stage: Option<InstallerStage>) -> Instant {
    let deadline = Instant::now() + SETUP;
    stage.map_or(deadline, |stage| deadline.min(stage.deadline()))
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
        if !self.started || self.installer.is_some() {
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

#[cfg(all(test, windows, target_arch = "x86_64"))]
mod accelerator_tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

    #[tokio::test(start_paused = true)]
    async fn silent_accelerator_respects_setup_deadline_and_cancellation() {
        for interrupt in [false, true] {
            let (host, simulated) = tokio::io::duplex(1024);
            let (reader, writer) = tokio::io::split(host);
            let mut qmp = Qmp {
                stream: Some(Stream {
                    reader: BufReader::new(reader),
                    writer,
                    next_id: 1,
                    pending: Vec::new(),
                }),
                started: false,
                installer: None,
            };
            let cancel = Shutdown::default();
            let mut peer = BufReader::new(simulated);
            let (_, result) = tokio::join!(
                async {
                    let mut command = String::new();
                    peer.read_line(&mut command).await.unwrap();
                    assert!(command.contains("query-accelerators"));
                    if interrupt {
                        cancel.cancel();
                    }
                },
                tokio::time::timeout(SETUP * 2, qmp.verify_whpx(&cancel))
            );
            assert_eq!(
                result.unwrap(),
                Err(if interrupt {
                    ChannelError::Cancelled
                } else {
                    ChannelError::Timeout
                })
            );
            assert!(qmp.stream.is_none());
        }
    }

    #[tokio::test]
    async fn paused_trial_requires_typed_whpx_selection() {
        for (response, accepted) in [
            (r#"{"enabled":"whpx","present":["whpx","tcg"]}"#, true),
            (r#"{"enabled":"tcg","present":["whpx","tcg"]}"#, false),
            (r#"{"enabled":"whpx","present":["tcg"]}"#, false),
            (r#"{"enabled":"whpx","present":["whpx","whpx"]}"#, false),
            (
                r#"{"enabled":"whpx","present":["whpx"],"extra":true}"#,
                false,
            ),
        ] {
            let (host, simulated) = tokio::io::duplex(1024);
            let (reader, writer) = tokio::io::split(host);
            let mut qmp = Qmp {
                stream: Some(Stream {
                    reader: BufReader::new(reader),
                    writer,
                    next_id: 1,
                    pending: Vec::new(),
                }),
                started: false,
                installer: None,
            };
            let cancel = Shutdown::default();
            let (_, result) = tokio::join!(
                async {
                    let mut peer = BufReader::new(simulated);
                    let mut command = String::new();
                    peer.read_line(&mut command).await.unwrap();
                    assert_eq!(command, "{\"execute\":\"query-accelerators\",\"id\":1}\r\n");
                    peer.get_mut()
                        .write_all(format!("{{\"return\":{response},\"id\":1}}\r\n").as_bytes())
                        .await
                        .unwrap();
                },
                qmp.verify_whpx(&cancel)
            );
            assert_eq!(result.is_ok(), accepted, "{response}");
        }
    }
}

#[cfg(test)]
mod quit_tests {
    use super::*;
    use tokio::io::AsyncBufReadExt;

    #[tokio::test]
    async fn quit_requires_correlated_reply_and_bounded_clean_eof() {
        let empty_reply = b"{\"return\":{},\"id\":1}\r\n".as_slice();
        for (reply, close, expected) in [
            (empty_reply, true, Ok(())),
            (b"".as_slice(), true, Err(ChannelError::Eof)),
            (
                b"{\"return\":{},\"id\":2}\r\n".as_slice(),
                true,
                Err(ChannelError::Unexpected),
            ),
            (
                b"{\"return\":{},\"id\":1}\r\n{".as_slice(),
                true,
                Err(ChannelError::Eof),
            ),
            (empty_reply, false, Err(ChannelError::Timeout)),
        ] {
            let (host, simulated) = tokio::io::duplex(1024);
            let (reader, writer) = tokio::io::split(host);
            let qmp = Qmp {
                stream: Some(Stream {
                    reader: BufReader::new(reader),
                    writer,
                    next_id: 1,
                    pending: Vec::new(),
                }),
                started: true,
                installer: None,
            };
            let cancel = Shutdown::default();
            let deadline = Instant::now() + Duration::from_millis(250);
            let (_, result) = tokio::join!(
                async {
                    let mut peer = BufReader::new(simulated);
                    let mut command = String::new();
                    peer.read_line(&mut command).await.unwrap();
                    assert_eq!(command, "{\"execute\":\"quit\",\"id\":1}\r\n");
                    peer.get_mut().write_all(reply).await.unwrap();
                    if !close {
                        tokio::time::sleep_until(deadline).await;
                        // Keep the peer alive past the deadline.
                        tokio::task::yield_now().await;
                    }
                },
                qmp.quit(&cancel, deadline),
            );
            assert_eq!(result, expected);
        }
    }
}
