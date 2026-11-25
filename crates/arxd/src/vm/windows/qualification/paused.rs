//! Private contract for host qualification trials. One private `LaunchPurpose`
//! enum derives argv, channel name, parser, stop protocol and Job envelope.
//! The reviewed stock bundle inventory remains held from admission through teardown.

mod native;

pub use native::{
    CapacityProbe, CapacityProbePermit, FirmwareProbe, FirmwareProbePermit, NetworkProbe,
    NetworkProbePermit, PausedCompleted, PausedPhase, PausedTrial, PausedTrialPermit,
    PausedVerdict, ReserveActivityPermit, ReserveActivityProbe, SupervisorLossPermit,
    SupervisorLossProbe,
};

use super::*;
use crate::vm::guest::network_probe::WitnessReport;
use crate::vm::qualification_record::ControlledFault;
use crate::vm::{qmp::bounded, runtime::capacity};
use std::{
    cell::Cell,
    future::{Future, poll_fn},
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, ReadBuf},
    time::Instant as Tick,
};

/// Bounded quiet window proving no early guest or firmware bytes arrive on the
/// channel before `-S` is released.
const PRELAUNCH_QUIET_WINDOW: Duration = Duration::from_millis(25);

/// Single verified per-purpose Job and outside-Job overhead envelope.
/// Replaces implicit arithmetic between Job limits and reserve admission.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct JobEnvelope {
    job_memory_bytes: usize,
    job_cpu_rate: u32,
    outside_ram_bytes: u64,
    outside_cpu_rate: u32,
    inner_bytes: u64,
    outer_bytes: u64,
}

impl JobEnvelope {
    const DISKLESS: Self = Self {
        job_memory_bytes: JOB_MEMORY_BYTES,
        job_cpu_rate: JOB_CPU_RATE,
        outside_ram_bytes: GIB,
        outside_cpu_rate: 1250,
        inner_bytes: 1 << 20,
        outer_bytes: 1 << 20,
    };

    const NETWORK_LIVE: Self = Self {
        job_memory_bytes: 6 * (1 << 30),
        job_cpu_rate: JOB_CPU_RATE,
        outside_ram_bytes: GIB,
        outside_cpu_rate: 1250,
        inner_bytes: 2 << 20,
        outer_bytes: 2 << 20,
    };
}

/// Stop protocol derived from `LaunchPurpose`. Keeps control-plane QMP `quit`
/// distinct from guest `system_powerdown` and forced/supervisor-loss stop.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StopProtocol {
    ForceTerminate,
    QmpQuit,
    QmpPowerdown,
    DropLastJobHandle,
}

/// Closed launch purpose enum. Every mode's argv, channel name, parser,
/// Job envelope and stop protocol are derived exclusively from this type.
enum LaunchPurpose<'a> {
    Paused,
    Firmware(Firmware<'a>),
    SupervisorLoss,
    ControlledFault(ControlledFault),
    Network(NetworkBoot<'a>),
}

impl<'a> LaunchPurpose<'a> {
    fn channel(&self) -> Option<&'a SerialName> {
        match self {
            Self::Firmware(firmware) => Some(firmware.console),
            Self::Network(network) => Some(network.console),
            Self::Paused | Self::SupervisorLoss | Self::ControlledFault(_) => None,
        }
    }

    fn stop_protocol(&self) -> StopProtocol {
        match self {
            Self::Paused | Self::ControlledFault(_) => StopProtocol::ForceTerminate,
            Self::Firmware(_) => StopProtocol::QmpQuit,
            Self::Network(_) => StopProtocol::QmpPowerdown,
            Self::SupervisorLoss => StopProtocol::DropLastJobHandle,
        }
    }

    fn envelope(&self) -> JobEnvelope {
        match self {
            Self::Network(_) => JobEnvelope::NETWORK_LIVE,
            Self::Paused | Self::Firmware(_) | Self::SupervisorLoss | Self::ControlledFault(_) => {
                JobEnvelope::DISKLESS
            }
        }
    }

    fn needs_job_watch(&self) -> bool {
        matches!(self, Self::SupervisorLoss)
    }
}

/// Fixed recipe. No caller-supplied options or inherited environment.
struct PausedPlan<'a> {
    image: &'a LocalPath,
    data: &'a LocalPath,
    bios: &'a LocalPath,
    purpose: LaunchPurpose<'a>,
}

struct Firmware<'a> {
    code: &'a LocalPath,
    vars: &'a LocalPath,
    console: &'a SerialName,
}

struct NetworkBoot<'a> {
    source: &'a super::test::NetworkInputs,
    seed: &'a PathPin,
    code: &'a LocalPath,
    vars: &'a LocalPath,
    console: &'a SerialName,
    nonce: [u8; 16],
}

impl PausedPlan<'_> {
    fn args(&self) -> Vec<String> {
        let mut args: Vec<_> = [
            "-no-user-config",
            "-nodefaults",
            "-display",
            "none",
            "-serial",
            "none",
            "-monitor",
            "none",
            "-accel",
            "whpx",
            "-machine",
            match &self.purpose {
                LaunchPurpose::Firmware(_) | LaunchPurpose::Network(_) => {
                    "pc-q35-11.1,pflash0=code,pflash1=vars"
                }
                LaunchPurpose::Paused
                | LaunchPurpose::SupervisorLoss
                | LaunchPurpose::ControlledFault(_) => "pc-q35-11.1",
            },
            "-smp",
            match &self.purpose {
                LaunchPurpose::Network(_) => "2",
                _ => "1",
            },
            "-m",
            match &self.purpose {
                LaunchPurpose::Network(_) => "4096M",
                _ => "256M",
            },
            "-L",
            self.data.as_str(),
            "-S",
            "-chardev",
            "stdio,id=arxqmp,signal=off",
            "-mon",
            "chardev=arxqmp,mode=control",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();

        match &self.purpose {
            LaunchPurpose::Network(network) => {
                use crate::vm::qemu_args::{self as args_for, GUEST_PORT, GUEST_PORT_ID, NETWORK};
                let [iso, kernel, initrd] = &network.source.media;
                for (node, path) in [("code", network.code), ("vars", network.vars)] {
                    args_for::block(&mut args, "raw", node, path, true, None);
                }
                args_for::cd(&mut args, "osiso", &iso.path, "ide.0");
                args_for::cd(&mut args, "cidata", &network.seed.path, "ide.1");
                args.extend([
                    "-kernel".into(),
                    kernel.path.as_str().into(),
                    "-initrd".into(),
                    initrd.path.as_str().into(),
                    "-append".into(),
                    "boot=casper ds=nocloud ---".into(),
                    "-netdev".into(),
                    NETWORK.into(),
                    "-device".into(),
                    "virtio-net-pci,netdev=arxnet0".into(),
                    "-device".into(),
                    "virtio-serial-pci,id=arxserial".into(),
                    "-chardev".into(),
                    format!("pipe,id=arxguest,path={}", network.console.basename()),
                    "-device".into(),
                    format!(
                        "virtserialport,bus=arxserial.0,chardev=arxguest,id={GUEST_PORT_ID},name={GUEST_PORT}"
                    ),
                ]);
            }
            LaunchPurpose::Firmware(firmware) => {
                let at = args
                    .iter()
                    .position(|arg| arg == "-accel")
                    .expect("fixed recipe");
                args.splice(at..at, ["-nic".into(), "none".into()]);
                for (node, path) in [("code", firmware.code), ("vars", firmware.vars)] {
                    args.extend([
                        "-blockdev".into(),
                        serde_json::json!({
                            "driver": "raw", "node-name": node, "read-only": true,
                            "file": {"driver": "file", "filename": path.as_str(), "read-only": true}
                        })
                        .to_string(),
                    ]);
                }
                args.extend([
                    "-chardev".into(),
                    format!("pipe,id=arxfirmware,path={}", firmware.console.basename()),
                    "-device".into(),
                    "isa-debugcon,iobase=0x402,chardev=arxfirmware".into(),
                ]);
            }
            LaunchPurpose::Paused
            | LaunchPurpose::SupervisorLoss
            | LaunchPurpose::ControlledFault(_) => {
                // Keep the original paused recipe's exact argument order.
                let at = args
                    .iter()
                    .position(|arg| arg == "-accel")
                    .expect("fixed recipe");
                args.splice(at..at, ["-nic".into(), "none".into()]);
                let at = args
                    .iter()
                    .position(|arg| arg == "-S")
                    .expect("fixed recipe");
                args.splice(at..at, ["-bios".into(), self.bios.as_str().into()]);
            }
        }
        args
    }

    /// Prove zero unintended ingress/hostfwd, zero TAP/bridge/sharing/IPv6,
    /// and read-only media on the generated argv before process creation.
    fn verify_network_isolation(&self) -> Result<(), NativeRefusal> {
        let reason = NativeRefusal::NetworkFenceUnverified;
        let args = self.args();
        let forbidden = [
            "hostfwd=",
            "guestfwd=",
            "smb=",
            "tftp=",
            "ipv6=on",
            "restrict=off",
            "tap,",
            "bridge,",
            "socket,",
        ];
        if args
            .iter()
            .any(|arg| forbidden.iter().any(|bad| arg.contains(bad)))
        {
            return Err(reason);
        }
        let mut netdevs = Vec::new();
        let mut nics = Vec::new();
        let mut blockdevs = Vec::new();
        let mut iter = args.iter();
        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "-net" => return Err(reason),
                "-netdev" => netdevs.push(iter.next().ok_or(reason)?.as_str()),
                "-nic" => nics.push(iter.next().ok_or(reason)?.as_str()),
                "-blockdev" => blockdevs.push(iter.next().ok_or(reason)?.as_str()),
                _ => {}
            }
        }
        // Every attached blockdev must be explicitly read-only.
        for blockdev in blockdevs {
            let parsed: serde_json::Value = serde_json::from_str(blockdev).map_err(|_| reason)?;
            if parsed.get("read-only") != Some(&serde_json::Value::Bool(true))
                || parsed.pointer("/file/read-only") != Some(&serde_json::Value::Bool(true))
            {
                return Err(reason);
            }
        }
        match &self.purpose {
            LaunchPurpose::Network(_) => {
                if netdevs != [crate::vm::qemu_args::NETWORK]
                    || !nics.is_empty()
                    || !args.windows(2).any(|pair| {
                        pair[0] == "-device" && pair[1] == "virtio-net-pci,netdev=arxnet0"
                    })
                {
                    return Err(reason);
                }
            }
            _ => {
                if !netdevs.is_empty() || nics != ["none"] {
                    return Err(reason);
                }
            }
        }
        Ok(())
    }
}

/// Confirm no early bytes or EOF arrive on the guest/firmware channel before
/// `-S` is released, using an explicit bounded async receive seam.
async fn verify_prelaunch_quiet(
    mut source: impl AsyncRead + Unpin,
    quiet_window: Duration,
) -> Result<(), Failure> {
    let mut byte = [0u8; 1];
    match tokio::time::timeout(quiet_window, source.read(&mut byte)).await {
        Err(_) => Ok(()),
        Ok(_) => Err(NativeRefusal::GuestEndpointUnverified.into()),
    }
}

/// The exact CODE pin contains this SEC debug print. Neither guest
/// addresses nor raw output are returned.
async fn firmware_sec(mut source: impl AsyncRead + Unpin) -> Result<(), Failure> {
    const MAX_OUTPUT: usize = 16 * 1024;
    const MAX_LINE: usize = 256;
    let mut line = Vec::with_capacity(MAX_LINE);
    let visible = |line: &[u8]| {
        line.strip_prefix(b"SecCoreStartupWithStack(")
            .and_then(|tail| tail.strip_suffix(b")"))
            .and_then(|tail| {
                let comma = tail.iter().position(|&byte| byte == b',')?;
                Some((&tail[..comma], &tail[comma + 1..]))
            })
            .is_some_and(|(left, right)| {
                [left, right.strip_prefix(b" ").unwrap_or(right)]
                    .into_iter()
                    .all(|address| {
                        address.strip_prefix(b"0x").is_some_and(|digits| {
                            (1..=8).contains(&digits.len())
                                && digits.iter().all(u8::is_ascii_hexdigit)
                        })
                    })
            })
    };
    for _ in 0..MAX_OUTPUT {
        let byte = source
            .read_u8()
            .await
            .map_err(|_| Failure::Qmp(ChannelError::Eof))?;
        if byte == b'\n' {
            if visible(line.strip_suffix(b"\r").unwrap_or(&line)) {
                return Ok(());
            }
            line.clear();
        } else {
            if line.len() == MAX_LINE {
                return Err(Failure::Qmp(ChannelError::Oversize));
            }
            line.push(byte);
        }
    }
    Err(Failure::Qmp(ChannelError::Oversize))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Failure {
    Refused(NativeRefusal),
    OwnerActive,
    Qmp(ChannelError),
    Timeout,
    Cancelled,
}

impl From<NativeRefusal> for Failure {
    fn from(reason: NativeRefusal) -> Self {
        Self::Refused(reason)
    }
}

impl From<ChannelError> for Failure {
    fn from(reason: ChannelError) -> Self {
        match reason {
            ChannelError::Timeout => Self::Timeout,
            ChannelError::Cancelled => Self::Cancelled,
            _ => Self::Qmp(reason),
        }
    }
}

struct Live {
    captured: Tick,
    /// Already bound to the enrolled console incarnation.
    idle: bool,
    active_cpus: u32,
    effective_cpus: u32,
    available_ram_bytes: u64,
    /// Verified actual WHPX capability, independent of the QMP exchange.
    whpx: bool,
    limits: Limits,
    /// Other outstanding or quarantined peaks, net of allocated bytes.
    outstanding_inner_bytes: u64,
    outstanding_outer_bytes: u64,
}

impl Live {
    fn check(&self, envelope: JobEnvelope, now: Tick) -> Result<(), Failure> {
        let reserves = NativeRefusal::ReservesUnverified;
        fresh_observation(self.captured.into_std(), now.into_std(), reserves)?;
        if !self.idle {
            return Err(Failure::OwnerActive);
        }
        if !self.whpx {
            return Err(NativeRefusal::WhpxUnverified.into());
        }
        let ram_needed = RAM_RESERVE
            .checked_add(envelope.job_memory_bytes as u64)
            .and_then(|bytes| bytes.checked_add(envelope.outside_ram_bytes))
            .ok_or(reserves)?;
        // A Job rate is in units of 1/100 percent. Bound it against the
        // global capacity and leave the rounded-up half of effective CPUs.
        let cpus = u64::from(self.effective_cpus);
        let rate = u64::from(envelope.job_cpu_rate) + u64::from(envelope.outside_cpu_rate);
        if self.effective_cpus < 2
            || self.effective_cpus > self.active_cpus
            || rate > 10_000
            || u64::from(self.active_cpus) * rate > (cpus / 2) * 10_000
            || self.available_ram_bytes < ram_needed
        {
            return Err(reserves.into());
        }
        self.check_disk(envelope.inner_bytes, envelope.outer_bytes)?;
        Ok(())
    }

    fn check_disk(&self, inner_bytes: u64, outer_bytes: u64) -> Result<(), NativeRefusal> {
        let disk = NativeRefusal::DiskCeilingUnverified;
        let l = &self.limits;
        let inner_free = l.inner_caller_free_bytes.min(l.inner_total_free_bytes);
        let outer_free = l.outer_caller_free_bytes.min(l.outer_total_free_bytes);
        let used = l
            .inner_total_bytes
            .checked_sub(l.inner_total_free_bytes)
            .ok_or(disk)?;
        let charged = used.checked_add(self.outstanding_inner_bytes).ok_or(disk)?;
        let inner = inner_free
            .checked_sub(self.outstanding_inner_bytes)
            .ok_or(disk)?;
        let outer = outer_free
            .checked_sub(self.outstanding_outer_bytes)
            .ok_or(disk)?;
        let ceiling = l.disk_budget_bytes.min(DISK_CEILING);
        let accounted = capacity(
            charged,
            inner,
            outer,
            l.inner_total_bytes,
            ceiling,
            OUTER_FLOOR,
        )?;
        if accounted
            .managed_bytes
            .checked_add(inner_bytes)
            .is_none_or(|bytes| bytes > ceiling)
            || inner_bytes > inner
            || outer
                .checked_sub(outer_bytes)
                .is_none_or(|bytes| bytes < OUTER_FLOOR)
        {
            return Err(disk);
        }
        Ok(())
    }
}

struct Pipes<R, W> {
    stdout: R,
    stdin: W,
    dispatch: Arc<DispatchGate>,
}

/// Private OS adapter contract. Every observation concerns the same held
/// slot and child. Pending operations must yield promptly so the supervisor
/// can monitor pressure and stop the Job. The adapter retains partial
/// creation resources and prevents effects from starting after stop.
trait Host {
    type Opens;
    type Slot;
    type Reader: AsyncRead + Unpin;
    type Writer: AsyncWrite + Unpin;
    type Channel: AsyncRead + Unpin;

    /// Hold and validate the selected stock bundle before acquiring the slot.
    fn open_contract(
        &mut self,
        plan: &PausedPlan<'_>,
    ) -> Result<(Self::Opens, JobEnvelope), NativeRefusal>;
    /// Provide a bounded sample independently of a pending operation.
    /// When a controlled fault is armed on the active slot, the next sample
    /// checks a fresh real sample, records it, applies the mutation and
    /// returns the resulting refusal through the exact live monitoring path.
    fn live(&self, envelope: JobEnvelope) -> Result<Live, NativeRefusal>;
    /// Acquire the charged slot atomically, or fail without acquiring it.
    fn claim(&mut self, live: &Live, envelope: JobEnvelope) -> Result<Self::Slot, NativeRefusal>;
    /// Complete any wait needed to confirm the already owned claim.
    async fn confirm_claim(&self, slot: &Self::Slot) -> Result<(), NativeRefusal>;
    async fn create_suspended(
        &self,
        slot: &Self::Slot,
        plan: &PausedPlan<'_>,
        opens: &Self::Opens,
    ) -> Result<(), NativeRefusal>;
    /// Verify held image identity, the restricted child token, creation-time
    /// assignment and the effective one-process Job before releasing the OS
    /// hold. The pipe worker gates the actual write, not the enqueue.
    async fn verify_child(
        &self,
        slot: &Self::Slot,
        envelope: JobEnvelope,
    ) -> Result<Pipes<Self::Reader, Self::Writer>, NativeRefusal>;
    async fn resume(&self, slot: &Self::Slot) -> Result<(), NativeRefusal>;
    /// Recheck the held bundle and any media drive bindings after prelaunch.
    async fn verify_opens(
        &self,
        slot: &Self::Slot,
        opens: &Self::Opens,
    ) -> Result<(), NativeRefusal>;
    /// Observe the selected accelerator initialized on this same child.
    /// The capability probe and the configure-time argv are not that proof.
    async fn verify_whpx(
        &self,
        slot: &Self::Slot,
        query: impl Future<Output = Result<(), Failure>>,
    ) -> Result<(), Failure>;
    async fn open_channel(
        &self,
        _slot: &Self::Slot,
        _name: &SerialName,
        _cancel: &Shutdown,
        _cutoff: Tick,
    ) -> Result<Self::Channel, NativeRefusal> {
        Err(NativeRefusal::GuestEndpointUnverified)
    }

    fn mark_phase_complete(&self, _slot: &Self::Slot, _phase: PausedPhase) {}
    fn mark_prelaunch_quiet(&self, _slot: &Self::Slot) {}
    fn mark_sec_marker(&self, _slot: &Self::Slot) {}
    fn record_network_report(&self, _slot: &Self::Slot, _report: WitnessReport) {}
    async fn network_post_controls(&self, _network: &NetworkBoot<'_>) -> bool {
        false
    }
    fn mark_post_controls(&self, _slot: &Self::Slot) {}

    /// Record completion of the purpose's expected graceful stop protocol.
    fn mark_stop_protocol_done(&self, _slot: &Self::Slot, _protocol: StopProtocol) {}

    /// Arm a controlled fault on the next live monitoring sample so the
    /// refusal traverses the exact `LivePoll` refusal-to-stop path.
    fn arm_fault(&self, _slot: &Self::Slot, _fault: ControlledFault) -> Result<(), NativeRefusal> {
        Err(NativeRefusal::ReservesUnverified)
    }

    fn lose_supervisor(&self, _slot: &Self::Slot) -> Result<(), NativeRefusal> {
        Err(NativeRefusal::JobUnverified)
    }

    fn completed_stop_protocol(&self, _slot: &Self::Slot) -> Option<StopProtocol> {
        None
    }

    /// Request termination of only the attributed Job and stop its workers.
    /// This and settle must also work on an incomplete claim or launch.
    fn request_stop(&mut self, slot: &Self::Slot);
    async fn observe_stop(&mut self, slot: &Self::Slot, deadline: Tick) -> StopEvidence;
    async fn reopen(&mut self, slot: &Self::Slot, deadline: Tick) -> bool;
    /// Persist the charge on uncertain release. Dropping the scoped slot must
    /// never be enough to make its reservation available again.
    fn settle(&mut self, slot: Self::Slot, disposition: StopDisposition) -> StopDisposition;
}

/// Owns teardown from the instant the charge is acquired. An externally
/// dropped future can still synchronously request stop and quarantine.
/// Only a live caller that collected release evidence can release the slot.
struct Settlement<'a, H: Host> {
    host: &'a mut H,
    slot: Option<H::Slot>,
    stop_requested: bool,
}

impl<H: Host> Settlement<'_, H> {
    fn stop(&mut self) {
        if !self.stop_requested
            && let Some(slot) = &self.slot
        {
            self.host.request_stop(slot);
            self.stop_requested = true;
        }
    }

    async fn finish(mut self, deadline: Tick, expected_stop: StopProtocol) -> StopDisposition {
        self.stop();
        let slot = self.slot.as_ref().expect("settlement owns a slot");
        let mut stop = by_deadline(deadline, self.host.observe_stop(slot, deadline))
            .await
            .unwrap_or_default();
        // Exclusive reopen is itself an effect. Wait for independent stop
        // evidence and reject any proof that arrives at or after the cutoff.
        let reopened = stop.ready_to_verify_release()
            && by_deadline(deadline, self.host.reopen(slot, deadline)).await == Some(true);
        stop.verify_release(|| reopened);
        // Release depends on timely child exit, an empty Job, joined
        // workers, and exclusive reopen. Live activity and reserve samples
        // govern new work, so their expiry cannot block safe settlement.
        let release = if Tick::now() >= deadline
            || !stop.ready_to_verify_release()
            || !stop.artifacts_reopened
        {
            StopDisposition::ChargedQuarantine
        } else {
            match (expected_stop, self.host.completed_stop_protocol(slot)) {
                (StopProtocol::QmpQuit, Some(StopProtocol::QmpQuit)) if stop.clean_process_exit => {
                    StopDisposition::CleanRelease
                }
                (StopProtocol::QmpPowerdown, Some(StopProtocol::QmpPowerdown))
                    if stop.guest_shutdown && stop.clean_process_exit =>
                {
                    StopDisposition::CleanRelease
                }
                _ => stop.disposition(),
            }
        };
        self.host
            .settle(self.slot.take().expect("settlement owns a slot"), release)
    }
}

impl<H: Host> Drop for Settlement<'_, H> {
    fn drop(&mut self) {
        self.stop();
        if let Some(slot) = self.slot.take() {
            self.host.settle(slot, StopDisposition::ChargedQuarantine);
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
struct TrialResult {
    paused: Result<(), Failure>,
    release: StopDisposition,
}

fn before(cancel: &Shutdown, cutoff: Tick) -> Result<Tick, Failure> {
    if cancel.is_cancelled() {
        return Err(Failure::Cancelled);
    }
    let now = Tick::now();
    if now >= cutoff {
        return Err(Failure::Timeout);
    }
    Ok(now)
}

const PRESSURE_INTERVAL: Duration = Duration::from_millis(250);

/// The bounds on one host observation. A pressure sample and the evidence
/// it produced can come due at different times.
#[derive(Clone, Copy)]
struct Freshness {
    captured: Tick,
    sample_due: Tick,
    cutoff: Tick,
}

impl Freshness {
    fn new(captured: Tick, sampled: Tick, cutoff: Tick) -> Self {
        Self {
            captured,
            sample_due: sampled + PRESSURE_INTERVAL,
            cutoff,
        }
    }

    fn deadline(self) -> Tick {
        self.sample_due
            .min(self.captured + FRESHNESS)
            .min(self.cutoff)
    }

    fn fresh_at(self, now: Tick) -> bool {
        (self.captured..self.deadline()).contains(&now)
    }
}

/// Timers wake the supervisor, but each decision compares the monotonic
/// clock to the same pressure, observation and effect deadlines.
struct LivePoll<'a> {
    cancel: &'a Shutdown,
    recheck: &'a dyn Fn() -> Result<Tick, Failure>,
    observation: Cell<Freshness>,
    stale: Cell<bool>,
    refused: Cell<Option<Failure>>,
}

impl<'a> LivePoll<'a> {
    fn new(
        cancel: &'a Shutdown,
        recheck: &'a dyn Fn() -> Result<Tick, Failure>,
        observation: Freshness,
    ) -> Self {
        Self {
            cancel,
            recheck,
            observation: Cell::new(observation),
            stale: Cell::new(false),
            refused: Cell::new(None),
        }
    }

    fn cutoff(&self) -> Tick {
        self.observation.get().cutoff
    }

    /// Force the next monitored poll to take a fresh live sample immediately.
    fn force_resample(&self) {
        self.stale.set(true);
    }

    fn sample(&self) -> Result<Freshness, Failure> {
        let cutoff = self.cutoff();
        before(self.cancel, cutoff)?;
        let captured = (self.recheck)()?;
        let now = before(self.cancel, cutoff)?;
        let observation = Freshness::new(captured, now, cutoff);
        if !observation.fresh_at(now) {
            return Err(NativeRefusal::ReservesUnverified.into());
        }
        self.stale.set(false);
        self.observation.set(observation);
        Ok(observation)
    }

    fn require_fresh(&self) -> Result<Freshness, Failure> {
        let now = before(self.cancel, self.cutoff())?;
        let observation = self.observation.get();
        if !self.stale.get() && observation.fresh_at(now) {
            Ok(observation)
        } else {
            self.sample()
        }
    }

    fn poll<T, E>(
        &self,
        refusal: impl FnOnce(Failure) -> E,
        advance: impl FnOnce(Freshness) -> Poll<Result<T, E>>,
    ) -> Poll<Result<T, E>> {
        let checked = match self.refused.get() {
            Some(reason) => Err(reason),
            None => self.require_fresh(),
        };
        match checked {
            Ok(observation) => advance(observation),
            Err(reason) => {
                self.refused.set(Some(reason));
                Poll::Ready(Err(refusal(reason)))
            }
        }
    }

    async fn watch<T>(
        &self,
        action: impl Future<Output = Result<T, Failure>>,
    ) -> Result<T, Failure> {
        tokio::pin!(action);
        let cutoff_alarm = tokio::time::sleep_until(self.cutoff());
        tokio::pin!(cutoff_alarm);
        loop {
            tokio::select! {
                biased;
                _ = self.cancel.cancelled() => return Err(Failure::Cancelled),
                _ = &mut cutoff_alarm => return Err(Failure::Timeout),
                _ = tokio::time::sleep_until(self.observation.get().deadline()) => {
                    self.require_fresh()?;
                }
                result = poll_fn(|cx| self.poll(|reason| reason, |_| action.as_mut().poll(cx))) => {
                    // An inner QMP read or write can refuse during an action
                    // poll. Preserve its typed reason through the IO boundary.
                    if let Some(reason) = self.refused.get() {
                        return Err(reason);
                    }
                    before(self.cancel, self.cutoff())?;
                    let value = result?;
                    self.require_fresh()?;
                    return Ok(value);
                }
            }
        }
    }
}

/// One QMP future can read the greeting and start writing in a single poll.
/// Use the same gate on the underlying IO calls as on the outer future.
struct LiveIo<'a, I> {
    gate: &'a LivePoll<'a>,
    inner: I,
}

impl<I: Unpin> LiveIo<'_, I> {
    fn poll_io<T>(
        &mut self,
        advance: impl FnOnce(Pin<&mut I>, Freshness) -> Poll<io::Result<T>>,
    ) -> Poll<io::Result<T>> {
        self.gate.poll(
            |_| io::ErrorKind::PermissionDenied.into(),
            |observation| advance(Pin::new(&mut self.inner), observation),
        )
    }
}

impl<I: AsyncRead + Unpin> AsyncRead for LiveIo<'_, I> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        self.poll_io(|io, _| io.poll_read(cx, buffer))
    }
}

/// Arm at the first actual write attempt of each command. The dispatch
/// worker checks the same deadline at the pipe, including time spent queued.
struct LiveWriter<'a, I> {
    stream: LiveIo<'a, I>,
    dispatch: &'a DispatchGate,
}

impl<I: AsyncWrite + Unpin> AsyncWrite for LiveWriter<'_, I> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let dispatch = self.dispatch;
        // Reading the dispatch state can block. The live check must follow
        // that acquisition, and arming has its own locked deadline check.
        let state = dispatch.current();
        let gate = self.stream.gate;
        self.stream.poll_io(|io, observation| {
            let window = match state {
                Dispatch::Idle => {
                    let deadline = observation.deadline().into_std();
                    if let Err(reason) =
                        dispatch.arm_until(observation.captured.into_std(), Some(deadline))
                    {
                        // Prefer the current live refusal if the effect
                        // expired while waiting for the dispatch lock.
                        let failure = gate.require_fresh().err().unwrap_or_else(|| reason.into());
                        gate.refused.set(Some(failure));
                        return Poll::Ready(Err(io::ErrorKind::PermissionDenied.into()));
                    }
                    deadline
                }
                Dispatch::Armed(deadline)
                | Dispatch::Writing(deadline)
                | Dispatch::WrittenInTime(deadline) => deadline,
                Dispatch::Expired | Dispatch::Ambiguous => {
                    gate.refused.set(Some(Failure::Qmp(ChannelError::Refused)));
                    return Poll::Ready(Err(io::ErrorKind::PermissionDenied.into()));
                }
            };
            // Recheck after arming can block. A refreshed sample cannot
            // extend this command's worker window or authorize stale bytes.
            if let Err(reason) = gate.require_fresh() {
                gate.refused.set(Some(reason));
                return Poll::Ready(Err(io::ErrorKind::PermissionDenied.into()));
            }
            if Tick::now().into_std() >= window {
                gate.refused
                    .set(Some(NativeRefusal::ReservesUnverified.into()));
                return Poll::Ready(Err(io::ErrorKind::PermissionDenied.into()));
            }
            io.poll_write(cx, bytes)
        })
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.stream.poll_io(|io, _| io.poll_flush(cx))
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.stream.poll_io(|io, _| io.poll_shutdown(cx))
    }
}

async fn confirm_dispatch(dispatch: &DispatchGate) -> Result<(), Failure> {
    dispatch
        .settle(Duration::from_millis(100))
        .await
        .map_err(|_| Failure::Qmp(ChannelError::Refused))
}

async fn observe<T>(action: impl Future<Output = Result<T, NativeRefusal>>) -> Result<T, Failure> {
    Ok(action.await?)
}

async fn launch_paused<H: Host>(
    host: &H,
    slot: &H::Slot,
    plan: &PausedPlan<'_>,
    opens: &H::Opens,
    gate: &LivePoll<'_>,
    envelope: JobEnvelope,
) -> Result<(), Failure> {
    gate.watch(observe(host.confirm_claim(slot))).await?;
    gate.sample()?;
    gate.watch(observe(host.create_suspended(slot, plan, opens)))
        .await?;
    gate.sample()?;
    let pipes = gate
        .watch(observe(host.verify_child(slot, envelope)))
        .await?;
    gate.sample()?;
    gate.watch(observe(host.resume(slot))).await?;
    host.mark_phase_complete(slot, PausedPhase::SuspendedChild);

    let Pipes {
        stdout,
        stdin,
        dispatch,
    } = pipes;
    let (cancel, cutoff) = (gate.cancel, gate.cutoff());
    // Connect channel-bearing modes after workers start and before awaiting
    // the QMP greeting, preserving same-handle owner/DACL/live-child PID proof.
    let mut channel = match plan.purpose.channel() {
        Some(console) => Some(
            gate.watch(observe(host.open_channel(slot, console, cancel, cutoff)))
                .await?,
        ),
        None => None,
    };
    gate.sample()?;
    let stdout = LiveIo {
        gate,
        inner: stdout,
    };
    let stdin = LiveWriter {
        stream: LiveIo { gate, inner: stdin },
        dispatch: &dispatch,
    };
    let mut peer = gate
        .watch(async { Ok(bounded(cancel, cutoff, Qmp::handshake(stdout, stdin, cancel)).await?) })
        .await?;
    gate.watch(confirm_dispatch(&dispatch)).await?;

    gate.sample()?;
    let status = gate
        .watch(async { Ok(bounded(cancel, cutoff, peer.status(cancel)).await?) })
        .await?;
    gate.watch(confirm_dispatch(&dispatch)).await?;
    if status != crate::vm::qmp::RunStatus::Prelaunch {
        return Err(Failure::Qmp(ChannelError::Unexpected));
    }
    // A new observation must still admit the paused child after prelaunch.
    gate.sample()?;

    gate.watch(host.verify_whpx(slot, async {
        bounded(cancel, cutoff, peer.verify_whpx())
            .await
            .map_err(|_| NativeRefusal::WhpxUnverified)?;
        confirm_dispatch(&dispatch).await
    }))
    .await?;
    host.mark_phase_complete(slot, PausedPhase::Prelaunch);
    gate.sample()?;

    gate.watch(observe(host.verify_opens(slot, opens))).await?;
    host.mark_phase_complete(slot, PausedPhase::BundleInventory);
    gate.sample()?;

    match &plan.purpose {
        LaunchPurpose::Paused => Ok(()),
        LaunchPurpose::ControlledFault(fault) => {
            // Arm the controlled fault and let the active LivePoll monitor
            // observe the refusal and drive real refusal-to-stop teardown.
            host.arm_fault(slot, *fault)?;
            gate.force_resample();
            gate.watch(std::future::pending::<Result<(), Failure>>())
                .await
        }
        LaunchPurpose::Firmware(_) => {
            let pipe = channel
                .as_mut()
                .ok_or(NativeRefusal::GuestEndpointUnverified)?;
            gate.watch(verify_prelaunch_quiet(
                LiveIo {
                    gate,
                    inner: &mut *pipe,
                },
                PRELAUNCH_QUIET_WINDOW,
            ))
            .await?;
            host.mark_prelaunch_quiet(slot);
            release_and_confirm_running(&mut peer, &dispatch, gate, cancel).await?;
            gate.watch(firmware_sec(LiveIo { gate, inner: pipe }))
                .await?;
            host.mark_sec_marker(slot);
            gate.watch(async { Ok(peer.quit(cancel, cutoff).await?) })
                .await?;
            gate.watch(confirm_dispatch(&dispatch)).await?;
            host.mark_stop_protocol_done(slot, StopProtocol::QmpQuit);
            Ok(())
        }
        LaunchPurpose::Network(network) => {
            let pipe = channel
                .as_mut()
                .ok_or(NativeRefusal::GuestEndpointUnverified)?;
            gate.watch(verify_prelaunch_quiet(
                LiveIo {
                    gate,
                    inner: &mut *pipe,
                },
                PRELAUNCH_QUIET_WINDOW,
            ))
            .await?;
            host.mark_prelaunch_quiet(slot);
            release_and_confirm_running(&mut peer, &dispatch, gate, cancel).await?;
            let report = gate
                .watch(async {
                    crate::vm::guest::network_probe::witness_response(
                        LiveIo { gate, inner: pipe },
                        &network.nonce,
                    )
                    .await
                    .map_err(Failure::from)
                })
                .await?;
            host.record_network_report(slot, report);
            gate.watch(async {
                if host.network_post_controls(network).await {
                    Ok(())
                } else {
                    Err(NativeRefusal::NetworkFenceUnverified.into())
                }
            })
            .await?;
            host.mark_post_controls(slot);
            gate.sample()?;
            gate.watch(async { Ok(peer.powerdown(cancel).await?) })
                .await?;
            gate.watch(confirm_dispatch(&dispatch)).await?;
            host.mark_stop_protocol_done(slot, StopProtocol::QmpPowerdown);
            Ok(())
        }
        LaunchPurpose::SupervisorLoss => {
            release_and_confirm_running(&mut peer, &dispatch, gate, cancel).await?;
            gate.require_fresh()?;
            host.lose_supervisor(slot)?;
            Ok(())
        }
    }
}

async fn release_and_confirm_running<R, W>(
    peer: &mut Qmp<R, W>,
    dispatch: &DispatchGate,
    gate: &LivePoll<'_>,
    cancel: &Shutdown,
) -> Result<(), Failure>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    gate.watch(async {
        Ok(peer
            .release_startup_hold(cancel, || gate.require_fresh().is_ok())
            .await?)
    })
    .await?;
    gate.watch(confirm_dispatch(dispatch)).await?;
    gate.sample()?;
    let status = gate.watch(async { Ok(peer.status(cancel).await?) }).await?;
    gate.watch(confirm_dispatch(dispatch)).await?;
    if status != crate::vm::qmp::RunStatus::Running {
        return Err(Failure::Qmp(ChannelError::Unexpected));
    }
    Ok(())
}

fn sample(host: &impl Host, envelope: JobEnvelope) -> Result<Tick, Failure> {
    let live = host.live(envelope)?;
    live.check(envelope, Tick::now())?;
    Ok(live.captured)
}

async fn run<H: Host>(
    host: &mut H,
    plan: &PausedPlan<'_>,
    cancel: &Shutdown,
    cutoff: Tick,
    settlement_budget: Duration,
) -> TrialResult {
    let admitted = (|| {
        before(cancel, cutoff)?;
        plan.verify_network_isolation()?;
        let (opens, envelope) = host.open_contract(plan)?;
        let live = host.live(envelope)?;
        let sampled = Tick::now();
        live.check(envelope, sampled)?;
        let observation = Freshness::new(live.captured, sampled, cutoff);
        if !observation.fresh_at(before(cancel, cutoff)?) {
            return Err(NativeRefusal::ReservesUnverified.into());
        }
        let slot = host.claim(&live, envelope)?;
        Ok((opens, envelope, slot, observation))
    })();
    let (opens, envelope, slot, observation) = match admitted {
        Ok(admission) => admission,
        Err(failure) => {
            return TrialResult {
                paused: Err(failure),
                release: StopDisposition::DirtyRelease,
            };
        }
    };

    let settlement = Settlement {
        host,
        slot: Some(slot),
        stop_requested: false,
    };
    let paused = {
        let recheck = || sample(settlement.host, envelope);
        let gate = LivePoll::new(cancel, &recheck, observation);
        launch_paused(
            settlement.host,
            settlement.slot.as_ref().expect("settlement owns a slot"),
            plan,
            &opens,
            &gate,
            envelope,
        )
        .await
    };
    // The effect deadline and caller cancellation cannot suppress cleanup.
    let release = settlement
        .finish(
            Tick::now() + settlement_budget,
            plan.purpose.stop_protocol(),
        )
        .await;
    TrialResult { paused, release }
}

async fn by_deadline<T>(deadline: Tick, action: impl Future<Output = T>) -> Option<T> {
    if Tick::now() >= deadline {
        return None;
    }
    tokio::pin!(action);
    let guarded = poll_fn(|cx| {
        if Tick::now() >= deadline {
            Poll::Ready(None)
        } else {
            action.as_mut().poll(cx).map(Some)
        }
    });
    let value = tokio::time::timeout_at(deadline, guarded).await.ok()??;
    (Tick::now() < deadline).then_some(value)
}

#[cfg(test)]
mod tests;
