//! Private contract for the first paused host trial. An adapter must prove a
//! reviewed open contract before acquiring a slot. The stock runner has no
//! such adapter. In particular, an inventory hash and a second pathname
//! lookup cannot establish what the loader or device model actually opened.

use super::*;
use crate::vm::{qmp::bounded, runtime::capacity};
use std::{future::Future, sync::Arc};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    time::Instant as Tick,
};

/// Fixed, one-vCPU recipe. No caller-supplied options, inherited environment,
/// guest disk, network, serial endpoint or command that releases `-S`.
struct PausedPlan<'a> {
    image: &'a LocalPath,
    data: &'a LocalPath,
    bios: &'a LocalPath,
}

impl PausedPlan<'_> {
    fn args(&self) -> Vec<&str> {
        vec![
            "-no-user-config",
            "-nodefaults",
            "-display",
            "none",
            "-serial",
            "none",
            "-monitor",
            "none",
            "-nic",
            "none",
            "-accel",
            "whpx",
            "-machine",
            "pc-q35-11.1",
            "-smp",
            "1",
            "-m",
            "256M",
            "-L",
            self.data.as_str(),
            "-bios",
            self.bios.as_str(),
            "-S",
            "-chardev",
            "stdio,id=arxqmp,signal=off",
            "-mon",
            "chardev=arxqmp,mode=control",
        ]
    }
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

/// Measured outside-Job overhead and reserved peak storage for this plan.
/// The stock runner cannot claim any of these measurements.
#[derive(Clone, Copy)]
struct Peak {
    outside_ram_bytes: u64,
    outside_cpu_rate: u32,
    inner_bytes: u64,
    outer_bytes: u64,
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
    fn check(&self, peak: Peak, now: Tick) -> Result<(), Failure> {
        let reserves = NativeRefusal::ReservesUnverified;
        fresh_observation(self.captured.into_std(), now.into_std(), reserves)?;
        if !self.idle {
            return Err(Failure::OwnerActive);
        }
        if !self.whpx {
            return Err(NativeRefusal::WhpxUnverified.into());
        }
        let ram_needed = RAM_RESERVE
            .checked_add(JOB_MEMORY_BYTES as u64)
            .and_then(|bytes| bytes.checked_add(peak.outside_ram_bytes))
            .ok_or(reserves)?;
        // A Job rate is in units of 1/100 percent. Bound it against the
        // global capacity and leave the rounded-up half of effective CPUs.
        let cpus = u64::from(self.effective_cpus);
        let rate = u64::from(JOB_CPU_RATE) + u64::from(peak.outside_cpu_rate);
        if self.effective_cpus < 2
            || self.effective_cpus > self.active_cpus
            || rate > 10_000
            || u64::from(self.active_cpus) * rate > (cpus / 2) * 10_000
            || self.available_ram_bytes < ram_needed
        {
            return Err(reserves.into());
        }
        self.check_disk(peak)?;
        Ok(())
    }

    fn check_disk(&self, peak: Peak) -> Result<(), NativeRefusal> {
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
            .checked_add(peak.inner_bytes)
            .is_none_or(|bytes| bytes > ceiling)
            || peak.inner_bytes > inner
            || outer
                .checked_sub(peak.outer_bytes)
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

    /// Requires a reviewed launch-time witness protocol for both the OS loader
    /// and QEMU (including short-lived consumers). No by-name fallback.
    fn open_contract(
        &mut self,
        plan: &PausedPlan<'_>,
    ) -> Result<(Self::Opens, Peak), NativeRefusal>;
    /// Provide a bounded sample independently of a pending operation.
    fn live(&self) -> Result<Live, NativeRefusal>;
    /// Acquire the charged slot atomically, or fail without acquiring it.
    fn claim(&mut self, live: &Live, peak: Peak) -> Result<Self::Slot, NativeRefusal>;
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
    ) -> Result<Pipes<Self::Reader, Self::Writer>, NativeRefusal>;
    async fn resume(&self, slot: &Self::Slot) -> Result<(), NativeRefusal>;
    /// Confirm the exact loaded modules and consumed firmware/data opens of
    /// this held child. Unobserved opens refuse even while QEMU is paused.
    async fn verify_opens(
        &self,
        slot: &Self::Slot,
        opens: &Self::Opens,
    ) -> Result<(), NativeRefusal>;
    /// Observe the selected accelerator initialized on this same child.
    /// The capability probe and the configure-time argv are not that proof.
    async fn verify_whpx(&self, slot: &Self::Slot) -> Result<(), NativeRefusal>;
    /// Request termination of only the attributed Job and stop its workers.
    /// This and settle must also work on an incomplete claim or launch.
    fn request_stop(&mut self, slot: &Self::Slot);
    async fn observe_stop(&mut self, slot: &Self::Slot, deadline: Tick) -> StopEvidence;
    async fn reopen(&mut self, slot: &Self::Slot, deadline: Tick) -> bool;
    /// Persist the charge on uncertain release. Dropping the scoped slot must
    /// never be enough to make its reservation available again.
    fn settle(&mut self, slot: Self::Slot, disposition: StopDisposition);
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

    async fn finish(mut self, deadline: Tick) -> StopDisposition {
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
        let release = stop.disposition();
        self.host
            .settle(self.slot.take().expect("settlement owns a slot"), release);
        release
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

fn before(cancel: &Shutdown, cutoff: Tick) -> Result<(), Failure> {
    if cancel.is_cancelled() {
        return Err(Failure::Cancelled);
    }
    if Tick::now() >= cutoff {
        return Err(Failure::Timeout);
    }
    Ok(())
}

async fn paused_exchange<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    pipes: Pipes<R, W>,
    cancel: &Shutdown,
    cutoff: Tick,
    recheck: &impl Fn() -> Result<Tick, Failure>,
) -> Result<(), Failure> {
    let Pipes {
        stdout,
        stdin,
        dispatch,
    } = pipes;
    let captured = recheck()?;
    before(cancel, cutoff)?;
    dispatch.arm_until(captured.into_std(), Some(cutoff.into_std()))?;
    let mut peer = watch_pressure(cancel, cutoff, recheck, async {
        Ok(bounded(cancel, cutoff, Qmp::handshake(stdout, stdin, cancel)).await?)
    })
    .await?;
    watch_pressure(cancel, cutoff, recheck, confirm_dispatch(&dispatch)).await?;

    let captured = recheck()?;
    before(cancel, cutoff)?;
    dispatch.arm_until(captured.into_std(), Some(cutoff.into_std()))?;
    let status = watch_pressure(cancel, cutoff, recheck, async {
        Ok(bounded(cancel, cutoff, peer.status(cancel)).await?)
    })
    .await?;
    watch_pressure(cancel, cutoff, recheck, confirm_dispatch(&dispatch)).await?;
    if status != crate::vm::qmp::RunStatus::Prelaunch {
        return Err(Failure::Qmp(ChannelError::Unexpected));
    }
    // A new observation must still admit the paused child after prelaunch.
    recheck()?;
    before(cancel, cutoff)
}

const PRESSURE_INTERVAL: Duration = Duration::from_millis(250);

/// Keep watching the owner and reserves while the paused child is alive,
/// including when the peer stalls. A new sample does not extend a write window.
async fn watch_pressure<T>(
    cancel: &Shutdown,
    cutoff: Tick,
    recheck: &impl Fn() -> Result<Tick, Failure>,
    action: impl Future<Output = Result<T, Failure>>,
) -> Result<T, Failure> {
    before(cancel, cutoff)?;
    tokio::pin!(action);
    let deadline = tokio::time::sleep_until(cutoff);
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(Failure::Cancelled),
            _ = &mut deadline => return Err(Failure::Timeout),
            // Completing an operation can release memory or send QMP.
            // Sample due pressure before the action can change those conditions.
            _ = tokio::time::sleep(PRESSURE_INTERVAL) => {
                recheck()?;
            }
            result = &mut action => {
                before(cancel, cutoff)?;
                return result;
            }
        }
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
    peak: Peak,
    cancel: &Shutdown,
    cutoff: Tick,
) -> Result<(), Failure> {
    let recheck = || sample(host, peak);
    watch_pressure(cancel, cutoff, &recheck, observe(host.confirm_claim(slot))).await?;
    recheck()?;
    watch_pressure(
        cancel,
        cutoff,
        &recheck,
        observe(host.create_suspended(slot, plan, opens)),
    )
    .await?;
    recheck()?;
    let pipes = watch_pressure(cancel, cutoff, &recheck, observe(host.verify_child(slot))).await?;
    recheck()?;
    watch_pressure(cancel, cutoff, &recheck, observe(host.resume(slot))).await?;
    paused_exchange(pipes, cancel, cutoff, &recheck).await?;

    watch_pressure(
        cancel,
        cutoff,
        &recheck,
        observe(host.verify_opens(slot, opens)),
    )
    .await?;
    recheck()?;
    watch_pressure(cancel, cutoff, &recheck, observe(host.verify_whpx(slot))).await?;
    recheck()?;
    before(cancel, cutoff)
}

fn sample(host: &impl Host, peak: Peak) -> Result<Tick, Failure> {
    let live = host.live()?;
    live.check(peak, Tick::now())?;
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
        let (opens, peak) = host.open_contract(plan)?;
        let live = host.live()?;
        live.check(peak, Tick::now())?;
        before(cancel, cutoff)?;
        let slot = host.claim(&live, peak)?;
        Ok((opens, peak, slot))
    })();
    let (opens, peak, slot) = match admitted {
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
    let paused = launch_paused(
        settlement.host,
        settlement.slot.as_ref().expect("settlement owns a slot"),
        plan,
        &opens,
        peak,
        cancel,
        cutoff,
    )
    .await;
    // The effect deadline and caller cancellation cannot suppress cleanup.
    let release = settlement.finish(Tick::now() + settlement_budget).await;
    TrialResult { paused, release }
}

async fn by_deadline<T>(deadline: Tick, action: impl Future<Output = T>) -> Option<T> {
    if Tick::now() >= deadline {
        return None;
    }
    let value = tokio::time::timeout_at(deadline, action).await.ok()?;
    (Tick::now() < deadline).then_some(value)
}

#[cfg(test)]
mod tests;
