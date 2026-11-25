//! Win32 adapter for host qualification trials. No production launch
//! authority is available here.

use super::*;
use crate::vm::qualification_record::FaultRecord;
use inventory::{restricted_environment as environment, safe_dll_search};
use std::{cell::RefCell, io::Write, rc::Rc};

const SLOT: &str = ".arx-paused-trial-charge";
const CWD: &str = "paused-cwd";
const IMAGE: &str = "qemu-system-x86_64.exe";
const FIRMWARE: &str = "bios-256k.bin";
const CODE: &str = "edk2-x86_64-code.fd";
const VARS: &str = "edk2-i386-vars.fd";

/// Path-free outcome for the binary's diagnostic graph.
pub enum PausedVerdict {
    Verified,
    Refused(NativeRefusal),
    OwnerActive,
    Qmp,
    Timeout,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PausedPhase {
    SuspendedChild,
    Prelaunch,
    BundleInventory,
}

/// Phase completion booleans set strictly after each corresponding check passes.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PausedCompleted {
    pub suspended_child: bool,
    pub prelaunch: bool,
    pub bundle_inventory: bool,
}

pub struct PausedTrial {
    pub verdict: PausedVerdict,
    pub release: StopDisposition,
    pub phase: PausedPhase,
    pub completed: PausedCompleted,
}

/// Consumed by the fixed owner-local trial. A diagnostic baseline cannot
/// produce this permit, and the held prerequisites are borrowed for its life.
pub struct PausedTrialPermit<'a>(Prepared<'a>);

impl<'a> PausedTrialPermit<'a> {
    pub(in crate::vm::windows::qualification) fn prepare(
        baseline: &'a Baseline,
    ) -> Result<Self, NativeRefusal> {
        Prepared::new(baseline).map(Self)
    }

    pub async fn run(self, cancel: &Shutdown) -> PausedTrial {
        let Prepared {
            mut host,
            image,
            data,
            bios,
        } = self.0;
        let plan = PausedPlan {
            image: &image,
            data: &data,
            bios: &bios,
            purpose: LaunchPurpose::Paused,
        };
        let result = run(
            &mut host,
            &plan,
            cancel,
            Tick::now() + Duration::from_secs(15),
            Duration::from_secs(5),
        )
        .await;
        let completed = host.completed.get();
        let phase = if completed.bundle_inventory || completed.prelaunch {
            PausedPhase::BundleInventory
        } else if completed.suspended_child {
            PausedPhase::Prelaunch
        } else {
            PausedPhase::SuspendedChild
        };
        PausedTrial {
            verdict: match result.paused {
                Ok(()) => PausedVerdict::Verified,
                Err(Failure::Refused(reason)) => PausedVerdict::Refused(reason),
                Err(Failure::OwnerActive) => PausedVerdict::OwnerActive,
                Err(Failure::Qmp(_)) => PausedVerdict::Qmp,
                Err(Failure::Timeout) => PausedVerdict::Timeout,
                Err(Failure::Cancelled) => PausedVerdict::Cancelled,
            },
            release: result.release,
            phase,
            completed,
        }
    }
}

/// Bounded typed observation for the diskless OVMF + QMP quit probe.
pub struct FirmwareProbe {
    pub accepted: bool,
    pub refusal: Option<NativeRefusal>,
    pub release: StopDisposition,
    pub sec_marker_verified: bool,
    pub prelaunch_quiet: bool,
    pub greeting_verified: bool,
    pub whpx_enabled: bool,
    pub quit_clean_eof: bool,
    pub job_memory_limit_bytes: u64,
    pub job_cpu_rate: u32,
}

/// Privately prepared firmware-only launch owned by a full TEST preflight.
pub struct FirmwareProbePermit<'a>(Prepared<'a>);

impl<'a> FirmwareProbePermit<'a> {
    pub(in crate::vm::windows::qualification) fn prepare(
        baseline: &'a Baseline,
    ) -> Result<Self, NativeRefusal> {
        if !baseline._held.full_test {
            return Err(NativeRefusal::QualificationMissing);
        }
        Prepared::new(baseline).map(Self)
    }

    /// Pass only for the fixed SEC marker, bounded prelaunch quiet check,
    /// bounded QMP exchange and independently confirmed CleanRelease.
    pub async fn run(self, cancel: &Shutdown) -> FirmwareProbe {
        let Prepared {
            mut host,
            image,
            data,
            bios,
        } = self.0;
        let probe = || {
            let reason = NativeRefusal::DistributionUnverified;
            Ok((
                data.join(CODE).map_err(|_| reason)?,
                data.join(VARS).map_err(|_| reason)?,
                SerialName::generate().map_err(|_| NativeRefusal::GuestEndpointUnverified)?,
            ))
        };
        let (code, vars, console) = match probe() {
            Ok(probe) => probe,
            Err(reason) => {
                return FirmwareProbe {
                    accepted: false,
                    refusal: Some(reason),
                    release: StopDisposition::DirtyRelease,
                    sec_marker_verified: false,
                    prelaunch_quiet: false,
                    greeting_verified: false,
                    whpx_enabled: false,
                    quit_clean_eof: false,
                    job_memory_limit_bytes: 0,
                    job_cpu_rate: 0,
                };
            }
        };
        let plan = PausedPlan {
            image: &image,
            data: &data,
            bios: &bios,
            purpose: LaunchPurpose::Firmware(Firmware {
                code: &code,
                vars: &vars,
                console: &console,
            }),
        };
        let envelope = plan.purpose.envelope();
        let trial = run(
            &mut host,
            &plan,
            cancel,
            Tick::now() + Duration::from_secs(15),
            Duration::from_secs(5),
        )
        .await;
        let completed = host.completed.get();
        let sec_marker_verified = host.sec_marker_verified.get();
        let prelaunch_quiet = host.prelaunch_quiet.get();
        let quit_clean_eof = host.stop_done.get() == Some(StopProtocol::QmpQuit);
        let accepted = trial.paused.is_ok()
            && trial.release == StopDisposition::CleanRelease
            && completed.prelaunch
            && completed.bundle_inventory
            && prelaunch_quiet
            && sec_marker_verified
            && quit_clean_eof;
        FirmwareProbe {
            accepted,
            refusal: match trial.paused {
                Ok(()) => None,
                Err(Failure::Refused(reason)) => Some(reason),
                Err(Failure::OwnerActive) => Some(NativeRefusal::OwnerActivityUnverified),
                Err(_) => Some(NativeRefusal::QmpUnverified),
            },
            release: trial.release,
            sec_marker_verified,
            prelaunch_quiet,
            greeting_verified: completed.prelaunch,
            whpx_enabled: completed.prelaunch,
            quit_clean_eof,
            job_memory_limit_bytes: envelope.job_memory_bytes as u64,
            job_cpu_rate: envelope.job_cpu_rate,
        }
    }
}

/// Bounded typed observation for the supervisor-loss row.
pub struct SupervisorLossProbe {
    pub accepted: bool,
    pub inconclusive: bool,
    pub refusal: Option<NativeRefusal>,
    pub release: StopDisposition,
    pub job_memory_limit_bytes: u64,
    pub job_cpu_rate: u32,
    pub active_process_limit: u32,
    pub child_exited: bool,
    pub job_empty_notified: bool,
}

/// One-use, diskless supervisor-loss probe. Closes the actual last Job handle
/// while the held QEMU child is live and waits for both held child exit and
/// the attributable `JOB_OBJECT_MSG_ACTIVE_PROCESS_ZERO` notice.
pub struct SupervisorLossPermit<'a>(Prepared<'a>);

impl<'a> SupervisorLossPermit<'a> {
    pub(in crate::vm::windows::qualification) fn prepare(
        baseline: &'a Baseline,
    ) -> Result<Self, NativeRefusal> {
        if !baseline._held.full_test {
            return Err(NativeRefusal::QualificationMissing);
        }
        Prepared::new(baseline).map(Self)
    }

    pub async fn run(self, cancel: &Shutdown) -> SupervisorLossProbe {
        let Prepared {
            mut host,
            image,
            data,
            bios,
        } = self.0;
        let plan = PausedPlan {
            image: &image,
            data: &data,
            bios: &bios,
            purpose: LaunchPurpose::SupervisorLoss,
        };
        let envelope = plan.purpose.envelope();
        let trial = run(
            &mut host,
            &plan,
            cancel,
            Tick::now() + Duration::from_secs(15),
            Duration::from_secs(5),
        )
        .await;
        let child_exited = host.loss_child_exited.get();
        let job_empty_notified = host.loss_job_empty.get();
        let accepted = trial.paused.is_ok()
            && trial.release == StopDisposition::DirtyRelease
            && supervisor_loss_released(child_exited, job_empty_notified);
        let inconclusive = trial.paused.is_ok()
            && (!job_empty_notified || trial.release == StopDisposition::ChargedQuarantine);
        SupervisorLossProbe {
            accepted,
            inconclusive,
            refusal: trial.paused.err().map(|_| NativeRefusal::JobUnverified),
            release: trial.release,
            job_memory_limit_bytes: envelope.job_memory_bytes as u64,
            job_cpu_rate: envelope.job_cpu_rate,
            active_process_limit: 1,
            child_exited,
            job_empty_notified,
        }
    }
}

/// Bounded typed observation for the capacity and recipe-workspace probe.
pub struct CapacityProbe {
    pub accepted: bool,
    pub refusal: Option<NativeRefusal>,
    pub release: StopDisposition,
    pub trial_allocated_bytes: u64,
    pub recipe_allocated_bytes: u64,
    pub denial_checks: u8,
    pub overlap_excluded: bool,
    pub aggregate_cap_checked: bool,
    pub trial_released: bool,
    pub recipe_released: bool,
}

/// Drop-safe custody guard for direct slot operations (capacity & network seed).
/// Any error, cancellation, or drop without an explicit verified release settles
/// the slot as `ChargedQuarantine`, keeping the charge durable.
struct SlotGuard {
    slot: Option<Slot>,
}

impl SlotGuard {
    fn new(slot: Slot) -> Self {
        Self { slot: Some(slot) }
    }

    fn slot(&self) -> &Slot {
        self.slot.as_ref().expect("guard owns slot")
    }

    fn settle(mut self, disposition: StopDisposition) -> StopDisposition {
        self.slot
            .take()
            .expect("guard owns slot")
            .settle(disposition)
    }
}

impl Drop for SlotGuard {
    fn drop(&mut self) {
        if let Some(slot) = self.slot.take() {
            let _ = slot.settle(StopDisposition::ChargedQuarantine);
        }
    }
}

/// Five independent pre-allocation denial checks against a live sample and
/// aggregate scope budget. Pure and testable without real volumes.
fn verify_capacity_denials(
    live: &Live,
    base_inner: u64,
    base_outer: u64,
    aggregate_within: impl Fn(u64) -> bool,
) -> Result<u8, NativeRefusal> {
    let reason = NativeRefusal::DiskCeilingUnverified;
    let mut passed = 0u8;

    // 1. Inner caller-available denial.
    let excess_caller = live.limits.inner_caller_free_bytes.saturating_add(1);
    if live.check_disk(excess_caller, base_outer) != Err(reason) {
        return Err(reason);
    }
    passed += 1;

    // 2. Inner total-free / disk-ceiling denial.
    let ceiling = live.limits.disk_budget_bytes.min(DISK_CEILING);
    let used = live
        .limits
        .inner_total_bytes
        .checked_sub(live.limits.inner_total_free_bytes)
        .ok_or(reason)?;
    let excess_ceiling = ceiling
        .saturating_sub(used)
        .saturating_add(1)
        .max(live.limits.inner_total_free_bytes.saturating_add(1));
    if live.check_disk(excess_ceiling, base_outer) != Err(reason) {
        return Err(reason);
    }
    passed += 1;

    // 3. Outer 50 GiB floor denial (crosses OUTER_FLOOR even while below total outer free).
    let outer_free = live
        .limits
        .outer_caller_free_bytes
        .min(live.limits.outer_total_free_bytes);
    let excess_outer_floor = outer_free.saturating_sub(OUTER_FLOOR).saturating_add(1);
    if live.check_disk(base_inner, excess_outer_floor) != Err(reason) {
        return Err(reason);
    }
    passed += 1;

    // 4. Overlapping charge exclusion in accounting (outstanding inner + outer).
    let overlapping = Live {
        outstanding_inner_bytes: live
            .limits
            .inner_caller_free_bytes
            .min(live.limits.inner_total_free_bytes),
        outstanding_outer_bytes: outer_free.saturating_sub(OUTER_FLOOR),
        ..Live {
            captured: live.captured,
            idle: live.idle,
            active_cpus: live.active_cpus,
            effective_cpus: live.effective_cpus,
            available_ram_bytes: live.available_ram_bytes,
            whpx: live.whpx,
            limits: live.limits,
            outstanding_inner_bytes: 0,
            outstanding_outer_bytes: 0,
        }
    };
    if overlapping.check_disk(base_inner, base_outer) != Err(reason) {
        return Err(reason);
    }
    passed += 1;

    // 5. Aggregate 200 GiB reservation cap across trial + recipe + production.
    if !aggregate_within(0) || aggregate_within(DISK_CEILING.saturating_add(1)) {
        return Err(reason);
    }
    passed += 1;

    Ok(passed)
}

/// Measures serialized 1 MiB allocations and release on both the sacrificial
/// trial volume and the disposable recipe workspace, plus five independent
/// capacity denial checks.
pub struct CapacityProbePermit<'a>(Prepared<'a>);

impl<'a> CapacityProbePermit<'a> {
    pub(in crate::vm::windows::qualification) fn prepare(
        baseline: &'a Baseline,
    ) -> Result<Self, NativeRefusal> {
        if !baseline._held.full_test || baseline._held.scope.is_none() {
            return Err(NativeRefusal::QualificationMissing);
        }
        Prepared::new(baseline).map(Self)
    }

    pub async fn run(self, cancel: &Shutdown) -> CapacityProbe {
        const ALLOCATION: u64 = 1 << 20;
        const TRIAL_FILE: &str = ".arx-test-capacity-allocation";
        const RECIPE_SLOT: &str = ".arx-recipe-capacity-charge";
        const RECIPE_FILE: &str = ".arx-recipe-capacity-allocation";
        let host = self.0.host;
        let cutoff = Tick::now() + Duration::from_secs(15);
        let reason = NativeRefusal::DiskCeilingUnverified;
        let root = &host.baseline._held.trial.leaf;
        let Some(scope) = host.baseline._held.scope.as_ref() else {
            return CapacityProbe {
                accepted: false,
                refusal: Some(NativeRefusal::QualificationMissing),
                release: StopDisposition::DirtyRelease,
                trial_allocated_bytes: 0,
                recipe_allocated_bytes: 0,
                denial_checks: 0,
                overlap_excluded: false,
                aggregate_cap_checked: false,
                trial_released: false,
                recipe_released: false,
            };
        };
        let recipe_root = &scope.recipe.root.leaf;
        let envelope = JobEnvelope {
            inner_bytes: 2 * ALLOCATION,
            outer_bytes: 2 * ALLOCATION,
            ..JobEnvelope::DISKLESS
        };

        let ready: Result<(SlotGuard, u8), Failure> = (|| {
            before(cancel, cutoff)?;
            let live = host.live(envelope)?;
            live.check(envelope, Tick::now())?;
            if live.limits.vhd_virtual_bytes > DISK_CEILING {
                return Err(reason.into());
            }
            let denials = verify_capacity_denials(
                &live,
                envelope.inner_bytes,
                envelope.outer_bytes,
                |extra| scope.aggregate_reservation_within_cap(&live.limits, extra),
            )?;
            let slot = Slot::claim(root, host.baseline.run())?;
            Ok((SlotGuard::new(slot), denials))
        })();
        let (trial_guard, denial_checks) = match ready {
            Ok(pair) => pair,
            Err(_) => {
                return CapacityProbe {
                    accepted: false,
                    refusal: Some(reason),
                    release: StopDisposition::DirtyRelease,
                    trial_allocated_bytes: 0,
                    recipe_allocated_bytes: 0,
                    denial_checks: 0,
                    overlap_excluded: false,
                    aggregate_cap_checked: false,
                    trial_released: false,
                    recipe_released: false,
                };
            }
        };

        let mut trial_file = None;
        let mut trial_allocated_bytes = 0u64;
        let mut overlap_excluded = false;
        let trial_result: Result<(), Failure> = async {
            host.confirm_claim(trial_guard.slot()).await?;
            before(cancel, cutoff)?;
            let initial = host.live(envelope)?;
            initial.check(envelope, Tick::now())?;
            // Overlapping claim on the same held root must be excluded.
            if Slot::claim(root, host.baseline.run()).is_ok() {
                return Err(reason.into());
            }
            overlap_excluded = true;
            trial_file = Some(sink::create_hidden(root, TRIAL_FILE).map_err(|_| reason)?);
            let writer = trial_file.as_mut().ok_or(reason)?;
            for _ in 0..16 {
                tokio::task::yield_now().await;
                before(cancel, cutoff)?;
                writer.write_all(&[0x59; 65_536]).map_err(|_| reason)?;
            }
            writer.sync_all().map_err(|_| reason)?;
            let (_, allocated) = file_identity(writer, false).map_err(|_| reason)?;
            trial_allocated_bytes = allocated;
            let sampled = host.live(envelope)?;
            sampled.check(envelope, Tick::now())?;
            if allocated < ALLOCATION
                || sampled
                    .limits
                    .inner_total_free_bytes
                    .checked_add(ALLOCATION)
                    .is_none_or(|after| after > initial.limits.inner_total_free_bytes)
            {
                return Err(reason.into());
            }
            before(cancel, cutoff)?;
            Ok(())
        }
        .await;

        let trial_discarded = trial_file.is_none_or(|f| sink::discard(&f).is_ok());
        let trial_settled = trial_discarded
            && listing_bounded(root, 4_096, Some(Instant::now() + Duration::from_secs(2)))
                .is_ok_and(|entries| {
                    !entries
                        .iter()
                        .any(|(name, _)| name.eq_ignore_ascii_case(TRIAL_FILE))
                });
        let trial_disp = if trial_settled && !trial_guard.slot().state.borrow().uncertain {
            StopDisposition::DirtyRelease
        } else {
            StopDisposition::ChargedQuarantine
        };
        let trial_release = trial_guard.settle(trial_disp);

        // Exercise serialized allocation and actual release on the disposable
        // recipe workspace as well, only if the trial slot released cleanly.
        let mut recipe_allocated_bytes = 0u64;
        let mut recipe_release = StopDisposition::DirtyRelease;
        let mut recipe_ok = false;
        if trial_result.is_ok() && trial_release == StopDisposition::DirtyRelease {
            if let Ok(recipe_slot) =
                Slot::claim_named(recipe_root, host.baseline.run(), RECIPE_SLOT)
            {
                let recipe_guard = SlotGuard::new(recipe_slot);
                let mut recipe_file = None;
                let res: Result<(), Failure> = async {
                    host.confirm_claim(recipe_guard.slot()).await?;
                    before(cancel, cutoff)?;
                    // Overlapping claim on the recipe workspace must fail.
                    if Slot::claim_named(recipe_root, host.baseline.run(), RECIPE_SLOT).is_ok() {
                        return Err(reason.into());
                    }
                    let before_free = super::super::capacity::held_free(recipe_root)?.total_free;
                    recipe_file =
                        Some(sink::create_hidden(recipe_root, RECIPE_FILE).map_err(|_| reason)?);
                    let writer = recipe_file.as_mut().ok_or(reason)?;
                    for _ in 0..16 {
                        tokio::task::yield_now().await;
                        before(cancel, cutoff)?;
                        writer.write_all(&[0x5a; 65_536]).map_err(|_| reason)?;
                    }
                    writer.sync_all().map_err(|_| reason)?;
                    let (_, allocated) = file_identity(writer, false).map_err(|_| reason)?;
                    recipe_allocated_bytes = allocated;
                    let after_free = super::super::capacity::held_free(recipe_root)?.total_free;
                    if allocated < ALLOCATION
                        || after_free
                            .checked_add(ALLOCATION)
                            .is_none_or(|sum| sum > before_free)
                    {
                        return Err(reason.into());
                    }
                    Ok(())
                }
                .await;
                let recipe_discarded = recipe_file.is_none_or(|f| sink::discard(&f).is_ok());
                let recipe_gone = recipe_discarded
                    && listing_bounded(
                        recipe_root,
                        4_096,
                        Some(Instant::now() + Duration::from_secs(2)),
                    )
                    .is_ok_and(|entries| {
                        !entries
                            .iter()
                            .any(|(name, _)| name.eq_ignore_ascii_case(RECIPE_FILE))
                    });
                let disp = if recipe_gone && !recipe_guard.slot().state.borrow().uncertain {
                    StopDisposition::DirtyRelease
                } else {
                    StopDisposition::ChargedQuarantine
                };
                recipe_release = recipe_guard.settle(disp);
                recipe_ok = res.is_ok() && recipe_release == StopDisposition::DirtyRelease;
            } else {
                recipe_release = StopDisposition::ChargedQuarantine;
            }
        }

        let release = if trial_release == StopDisposition::ChargedQuarantine
            || recipe_release == StopDisposition::ChargedQuarantine
        {
            StopDisposition::ChargedQuarantine
        } else {
            StopDisposition::DirtyRelease
        };
        let trial_released = trial_release == StopDisposition::DirtyRelease;
        let recipe_released = recipe_ok && recipe_release == StopDisposition::DirtyRelease;
        let accepted = trial_result.is_ok()
            && denial_checks == 5
            && overlap_excluded
            && trial_released
            && recipe_released
            && release == StopDisposition::DirtyRelease;
        CapacityProbe {
            accepted,
            refusal: (!accepted).then_some(reason),
            release,
            trial_allocated_bytes,
            recipe_allocated_bytes,
            denial_checks,
            overlap_excluded,
            aggregate_cap_checked: denial_checks == 5,
            trial_released,
            recipe_released,
        }
    }
}

pub struct ReserveActivityPermit<'a>(Prepared<'a>);

pub struct ReserveActivityProbe {
    pub observations: Option<([FaultRecord; 4], [u64; 5])>,
    pub release: StopDisposition,
    /// One disposition per controlled-fault invocation so quarantine latches
    /// across all four runs rather than overwriting a single slot.
    pub invocations: Vec<StopDisposition>,
}

fn expected_fault_refusal(fault: ControlledFault) -> NativeRefusal {
    match fault {
        ControlledFault::WtsAmbiguity => NativeRefusal::OwnerActivityUnverified,
        ControlledFault::DiskReserve => NativeRefusal::DiskCeilingUnverified,
        ControlledFault::CpuReserve | ControlledFault::RamReserve => {
            NativeRefusal::ReservesUnverified
        }
    }
}

fn fault_settled(fault: ControlledFault, result: &TrialResult) -> bool {
    result.paused == Err(Failure::Refused(expected_fault_refusal(fault)))
        && result.release == StopDisposition::DirtyRelease
}

fn supervisor_loss_released(child_exited: bool, job_empty: bool) -> bool {
    child_exited && job_empty
}

impl<'a> ReserveActivityPermit<'a> {
    pub(in crate::vm::windows::qualification) fn prepare(
        baseline: &'a Baseline,
    ) -> Result<Self, NativeRefusal> {
        if !baseline._held.full_test || baseline._held.witness_session.is_none() {
            return Err(NativeRefusal::QualificationMissing);
        }
        Prepared::new(baseline).map(Self)
    }

    pub async fn run(self, cancel: &Shutdown) -> ReserveActivityProbe {
        let Prepared {
            mut host,
            image,
            data,
            bios,
        } = self.0;
        let mut completed = Vec::with_capacity(ControlledFault::ALL.len());
        let mut invocations = Vec::with_capacity(ControlledFault::ALL.len());
        for fault in ControlledFault::ALL {
            host.fault_sample.set(None);
            host.armed_fault.set(None);
            let plan = PausedPlan {
                image: &image,
                data: &data,
                bios: &bios,
                purpose: LaunchPurpose::ControlledFault(fault),
            };
            let result = run(
                &mut host,
                &plan,
                cancel,
                Tick::now() + Duration::from_secs(15),
                Duration::from_secs(5),
            )
            .await;
            invocations.push(result.release);
            // The injected fault must traverse LivePoll, cause the exact
            // expected refusal, and complete verified forced-stop settlement.
            if !fault_settled(fault, &result) {
                return ReserveActivityProbe {
                    observations: None,
                    release: if invocations.contains(&StopDisposition::ChargedQuarantine) {
                        StopDisposition::ChargedQuarantine
                    } else {
                        result.release
                    },
                    invocations,
                };
            }
            let Some(mut sample) = host.fault_sample.take() else {
                return ReserveActivityProbe {
                    observations: None,
                    release: StopDisposition::DirtyRelease,
                    invocations,
                };
            };
            sample.settled_ms = elapsed(host.baseline.origin);
            completed.push(sample);
        }
        let witness = host
            .baseline
            ._held
            .witness_session
            .as_ref()
            .expect("prepare requires witness_session");
        let observations = match completed.try_into() {
            Ok(faults) => observe_console_transitions(host.baseline, witness, cancel)
                .await
                .map(|wts| (faults, wts)),
            Err(_) => None,
        };
        ReserveActivityProbe {
            observations,
            release: if invocations.contains(&StopDisposition::ChargedQuarantine) {
                StopDisposition::ChargedQuarantine
            } else {
                StopDisposition::DirtyRelease
            },
            invocations,
        }
    }
}

/// Corroborated two-sample console observation used by `ConsoleWitnessMachine`.
#[derive(Clone, Debug)]
struct ConsoleTransitionSnapshot {
    token_stable: bool,
    owner_session: u32,
    active_console: u32,
    owner_lsa: [LsaLogon; 2],
    owner_wts: [WtsSample; 2],
    owner_accounts: [Option<WtsAccount>; 2],
    /// Present whenever `active_console` is a non-zero, non-owner session.
    witness: Option<([WtsSample; 2], [Option<WtsAccount>; 2])>,
    now: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ConsoleStep {
    /// Valid observation, target state not yet corroborated.
    Pending,
    /// Transition phase `0..=4` corroborated.
    PhaseMatched(usize),
    /// Final post-return idle corroborated; safe for next effect.
    IdleVerified,
    /// Invalid or tampered observation; closes the run immediately.
    Invalid(NativeRefusal),
}

/// Pure, testable state machine for the 5-phase owner console witness sequence
/// plus final fresh-idle verification before the next effect.
struct ConsoleWitnessMachine {
    enrolled_witness: WtsAccount,
    anchor_owner_session: u32,
    anchor_owner_logon: i64,
    anchor_owner_lsa: LsaLogon,
    pre_prompt_input: i64,
    phase0_input: Option<i64>,
    phase2_witness: Option<(u32, i64)>,
    phase4_current: Option<i64>,
    /// Current phase: 0..=4 for transitions, 5 for post-return fresh idle.
    phase: usize,
    corroborated: u8,
}

impl ConsoleWitnessMachine {
    const REQUIRED_CORROBORATIONS: u8 = 2;

    fn begin(
        enrolled_witness: WtsAccount,
        initial: &ConsoleTransitionSnapshot,
    ) -> Result<Self, NativeRefusal> {
        let reason = NativeRefusal::OwnerActivityUnverified;
        validate_owner_snapshot(initial, None)?;
        if initial.active_console != initial.owner_session
            || initial.owner_wts[0].state != WTSActive
            || initial.owner_wts[1].state != WTSActive
            || initial.owner_wts[0].flags != WTS_SESSIONSTATE_UNLOCK as i32
            || initial.owner_wts[1].flags != WTS_SESSIONSTATE_UNLOCK as i32
        {
            return Err(reason);
        }
        // Enrolled witness account cannot alias the owner account.
        if enrolled_witness
            .user
            .eq_ignore_ascii_case(&initial.owner_lsa[0].user)
            && enrolled_witness
                .domain
                .eq_ignore_ascii_case(&initial.owner_lsa[0].domain)
        {
            return Err(reason);
        }
        Ok(Self {
            enrolled_witness,
            anchor_owner_session: initial.owner_session,
            anchor_owner_logon: initial.owner_wts[1].logon,
            anchor_owner_lsa: initial.owner_lsa[1].clone(),
            pre_prompt_input: initial.owner_wts[1].input,
            phase0_input: None,
            phase2_witness: None,
            phase4_current: None,
            phase: 0,
            corroborated: 0,
        })
    }

    fn phase(&self) -> usize {
        self.phase
    }

    fn step(&mut self, snap: &ConsoleTransitionSnapshot) -> ConsoleStep {
        let reason = NativeRefusal::OwnerActivityUnverified;
        if validate_owner_snapshot(
            snap,
            Some((
                self.anchor_owner_session,
                self.anchor_owner_logon,
                &self.anchor_owner_lsa,
            )),
        )
        .is_err()
        {
            return ConsoleStep::Invalid(reason);
        }
        // Owner input cursor must never move backwards from the pre-prompt anchor.
        if snap.owner_wts[0].input < self.pre_prompt_input
            || snap.owner_wts[1].input < self.pre_prompt_input
        {
            return ConsoleStep::Invalid(reason);
        }

        let owner_unlocked_active = snap.active_console == self.anchor_owner_session
            && snap.owner_wts[0].state == WTSActive
            && snap.owner_wts[1].state == WTSActive
            && snap.owner_wts[0].flags == WTS_SESSIONSTATE_UNLOCK as i32
            && snap.owner_wts[1].flags == WTS_SESSIONSTATE_UNLOCK as i32;

        let owner_locked_active = snap.active_console == self.anchor_owner_session
            && snap.owner_wts[0].state == WTSActive
            && snap.owner_wts[1].state == WTSActive
            && snap.owner_wts[0].flags == WTS_SESSIONSTATE_LOCK as i32
            && snap.owner_wts[1].flags == WTS_SESSIONSTATE_LOCK as i32;

        // If another user session became active, validate its session IDs and
        // require it to be the enrolled witness (wrong witness closes the run).
        let witness_active = if snap.active_console != self.anchor_owner_session
            && snap.active_console != 0
            && snap.active_console != u32::MAX
        {
            let Some(([w0, w1], [a0, a1])) = &snap.witness else {
                return ConsoleStep::Invalid(reason);
            };
            if w0.session != snap.active_console
                || w1.session != snap.active_console
                || w0.logon <= 0
                || w0.logon != w1.logon
                || a0 != a1
            {
                return ConsoleStep::Invalid(reason);
            }
            let Some(account) = a0.as_ref() else {
                return ConsoleStep::Invalid(reason);
            };
            if !account
                .user
                .eq_ignore_ascii_case(&self.enrolled_witness.user)
                || !account
                    .domain
                    .eq_ignore_ascii_case(&self.enrolled_witness.domain)
            {
                return ConsoleStep::Invalid(reason);
            }
            if let Some((expected_id, expected_logon)) = self.phase2_witness
                && (snap.active_console != expected_id || w1.logon != expected_logon)
            {
                return ConsoleStep::Invalid(reason);
            }
            w0.state == WTSActive
                && w1.state == WTSActive
                && w0.flags == WTS_SESSIONSTATE_UNLOCK as i32
                && w1.flags == WTS_SESSIONSTATE_UNLOCK as i32
        } else {
            false
        };

        let matched = match self.phase {
            // Phase 0: strictly advance past pre-prompt input cursor.
            0 => {
                owner_unlocked_active
                    && snap.owner_wts[1].input > self.pre_prompt_input
                    && snap.owner_wts[1].current - snap.owner_wts[1].input < IDLE_TICKS
            }
            // Phase 1: owner session locked on console.
            1 => owner_locked_active,
            // Phase 2: switched to enrolled witness session.
            2 => witness_active,
            // Phase 3: enrolled witness active and owner session disconnected.
            3 => {
                witness_active
                    && snap.owner_wts[0].state == WTSDisconnected
                    && snap.owner_wts[1].state == WTSDisconnected
            }
            // Phase 4: returned to original unlocked owner session with fresh unlock/input.
            4 => {
                let prior = self.phase0_input.unwrap_or(self.pre_prompt_input);
                owner_unlocked_active
                    && snap.owner_wts[1].input > prior
                    && snap.owner_wts[1].current - snap.owner_wts[1].input < IDLE_TICKS
            }
            // Phase 5: fresh idle observation after return before next effect.
            5 => {
                let floor = self.phase4_current.unwrap_or(0);
                owner_unlocked_active
                    && snap.owner_wts[1].current >= floor
                    && snap.owner_wts[1].current - snap.owner_wts[1].input >= IDLE_TICKS
            }
            _ => return ConsoleStep::Invalid(reason),
        };

        if !matched {
            self.corroborated = 0;
            return ConsoleStep::Pending;
        }
        self.corroborated = self.corroborated.saturating_add(1);
        if self.corroborated < Self::REQUIRED_CORROBORATIONS {
            return ConsoleStep::Pending;
        }
        self.corroborated = 0;
        let finished = self.phase;
        match finished {
            0 => self.phase0_input = Some(snap.owner_wts[1].input),
            2 => {
                if let Some(([_, w1], _)) = &snap.witness {
                    self.phase2_witness = Some((snap.active_console, w1.logon));
                }
            }
            4 => self.phase4_current = Some(snap.owner_wts[1].current),
            5 => {
                self.phase = 6;
                return ConsoleStep::IdleVerified;
            }
            _ => {}
        }
        self.phase += 1;
        ConsoleStep::PhaseMatched(finished)
    }
}

fn validate_owner_snapshot(
    snap: &ConsoleTransitionSnapshot,
    anchor: Option<(u32, i64, &LsaLogon)>,
) -> Result<(), NativeRefusal> {
    let reason = NativeRefusal::OwnerActivityUnverified;
    let [first, second] = snap.owner_wts;
    let session = snap.owner_session;
    let lsa_ok = snap.owner_lsa.iter().all(|lsa| {
        lsa.owner
            && lsa.session == session
            && lsa.logon_type == LOGON_INTERACTIVE
            && lsa.logon_time > 0
            && lsa.logon_time.abs_diff(first.logon) <= LOGON_SKEW_TICKS as u64
    }) && snap.owner_lsa[0] == snap.owner_lsa[1];
    if !snap.token_stable
        || !lsa_ok
        || snap.owner_accounts[0] != snap.owner_accounts[1]
        || !account_bound(snap.owner_accounts[0].as_ref(), &snap.owner_lsa[0])
        || first.session != session
        || second.session != session
        || first.logon <= 0
        || first.logon != second.logon
        || first.input < first.logon
        || second.input < first.input
        || second.current < first.current
        || second.current < second.input
        || !(0..=LIVE_TICKS).contains(&snap.now.saturating_sub(second.current))
    {
        return Err(reason);
    }
    if let Some((expected_session, expected_logon, expected_lsa)) = anchor
        && (session != expected_session
            || first.logon != expected_logon
            || &snap.owner_lsa[0] != expected_lsa)
    {
        return Err(reason);
    }
    Ok(())
}

fn capture_transition_snapshot(
    token: &VmToken,
) -> Result<ConsoleTransitionSnapshot, NativeRefusal> {
    let e = console_evidence(token)?;
    let witness = if e.active_console != e.token_session
        && e.active_console != 0
        && e.active_console != u32::MAX
    {
        let (w0, a0) =
            wts_sample(e.active_console).ok_or(NativeRefusal::OwnerActivityUnverified)?;
        let (w1, a1) =
            wts_sample(e.active_console).ok_or(NativeRefusal::OwnerActivityUnverified)?;
        Some(([w0, w1], [a0, a1]))
    } else {
        None
    };
    Ok(ConsoleTransitionSnapshot {
        token_stable: e.token_stable,
        owner_session: e.token_session,
        active_console: e.active_console,
        owner_lsa: e.lsa,
        owner_wts: e.wts,
        owner_accounts: e.accounts,
        witness,
        now: e.now,
    })
}

async fn observe_console_transitions(
    baseline: &Baseline,
    witness: &WtsAccount,
    cancel: &Shutdown,
) -> Option<[u64; 5]> {
    let token = &baseline._held.token;
    let initial = capture_transition_snapshot(token).ok()?;
    let mut machine = ConsoleWitnessMachine::begin(witness.clone(), &initial).ok()?;
    let mut output = [0; 5];
    let deadline = Tick::now() + Duration::from_secs(20 * 60);
    let prompts = [
        "Console witness: make one new input in the unlocked owner session.",
        "Console witness: lock the owner session.",
        "Console witness: switch to the enrolled witness session.",
        "Console witness: keep the enrolled witness session active until owner disconnect is observed.",
        "Console witness: return to the original owner session and unlock it without signing out.",
        "Console witness: leave the owner session unlocked and idle.",
    ];
    let mut prompted = None;
    loop {
        before(cancel, deadline).ok()?;
        let phase = machine.phase();
        if prompted != Some(phase) {
            eprintln!("{}", prompts[phase]);
            prompted = Some(phase);
        }
        let snapshot = capture_transition_snapshot(token).ok()?;
        match machine.step(&snapshot) {
            ConsoleStep::Pending => {}
            ConsoleStep::PhaseMatched(done) => {
                output[done] = elapsed(baseline.origin);
            }
            ConsoleStep::IdleVerified => return Some(output),
            ConsoleStep::Invalid(_) => return None,
        }
        tokio::select! {
            biased;
            _ = cancel.cancelled() => return None,
            _ = tokio::time::sleep_until((Tick::now() + Duration::from_millis(250)).min(deadline)) => {}
        }
    }
}

/// Bounded typed observation for the live restricted TEST network probe.
pub struct NetworkProbe {
    pub refusal: Option<NativeRefusal>,
    pub release: StopDisposition,
}

pub struct NetworkProbePermit<'a>(Prepared<'a>);

impl<'a> NetworkProbePermit<'a> {
    pub(in crate::vm::windows::qualification) fn prepare(
        baseline: &'a Baseline,
    ) -> Result<Self, NativeRefusal> {
        if !baseline._held.full_test || baseline._held.network.is_none() {
            return Err(NativeRefusal::NetworkFenceUnverified);
        }
        Prepared::new(baseline).map(Self)
    }

    pub async fn run(self, cancel: &Shutdown) -> NetworkProbe {
        const SEED: &str = ".arx-network-cidata.iso";
        const CHARGE: &str = ".arx-network-seed-charge";
        const MAX_SEED: u64 = 64 * 1024;
        let Prepared {
            mut host,
            image,
            data,
            bios,
        } = self.0;
        let baseline = host.baseline;
        let trial = &baseline._held.trial;
        let reason = NativeRefusal::NetworkFenceUnverified;
        let envelope = JobEnvelope::NETWORK_LIVE;
        let mut nonce = [0; 16];
        let absent = |reason| NetworkProbe {
            refusal: Some(reason),
            release: StopDisposition::DirtyRelease,
        };
        let setup = (|| {
            getrandom::fill(&mut nonce).map_err(|_| reason)?;
            let source = baseline._held.network.as_ref().ok_or(reason)?;
            let code = data.join(CODE).map_err(|_| reason)?;
            let vars = data.join(VARS).map_err(|_| reason)?;
            let path = trial.path.join(SEED).map_err(|_| reason)?;
            let console = SerialName::generate().map_err(|_| reason)?;
            let bytes = crate::vm::template::network_probe_seed(&nonce, &source.witnesses)
                .map_err(|_| reason)?;
            if bytes.len() as u64 > MAX_SEED {
                return Err(reason);
            }
            let live = host.live(envelope)?;
            live.check(envelope, Tick::now()).map_err(|_| reason)?;
            Ok((source, code, vars, path, console, bytes))
        })();
        let (source, code, vars, path, console, bytes) = match setup {
            Ok(setup) => setup,
            Err(reason) => return absent(reason),
        };
        let deadline = Tick::now() + Duration::from_secs(240);
        let host_controls_pre = matches!(
            bounded(cancel, deadline, async {
                Ok(source.witnesses.known_live(&nonce).await)
            })
            .await,
            Ok(true)
        );
        if !host_controls_pre {
            return absent(reason);
        }
        let slot = match Slot::claim_named(&trial.leaf, baseline.run(), CHARGE) {
            Ok(slot) => SlotGuard::new(slot),
            Err(reason) => return absent(reason),
        };
        let mut seed_created = false;
        let mut identity = None;
        let staged = (|| {
            before(cancel, deadline).map_err(|_| reason)?;
            host.confirm_claim_for_seed(slot.slot(), envelope)?;
            let mut file = sink::create_hidden(&trial.leaf, SEED).map_err(|_| reason)?;
            seed_created = true;
            identity = Some(file_identity(&file, false).map_err(|_| reason)?.0);
            file.write_all(&bytes)
                .and_then(|()| file.sync_all())
                .map_err(|_| reason)?;
            drop(file);
            let mut seed = super::super::capacity::walk(
                &super::super::capacity::MountManager,
                &baseline._held.volumes[0],
                &path,
                false,
                false,
            )?;
            if Some(seed.identity) != identity {
                return Err(reason);
            }
            seed.verify_content(
                ArtifactPin {
                    size: bytes.len() as u64,
                    sha256: crate::vm::qualification_record::digest(&bytes),
                },
                MAX_SEED,
            )?;
            Ok(seed)
        })();
        let mut zero_hostfwd = false;
        let observed = match staged {
            Ok(seed) => {
                let plan = PausedPlan {
                    image: &image,
                    data: &data,
                    bios: &bios,
                    purpose: LaunchPurpose::Network(NetworkBoot {
                        source,
                        seed: &seed,
                        code: &code,
                        vars: &vars,
                        console: &console,
                        nonce,
                    }),
                };
                if plan.verify_network_isolation().is_ok() {
                    zero_hostfwd = true;
                    run(&mut host, &plan, cancel, deadline, Duration::from_secs(5)).await
                } else {
                    TrialResult {
                        paused: Err(reason.into()),
                        release: StopDisposition::DirtyRelease,
                    }
                }
            }
            Err(reason) => TrialResult {
                paused: Err(reason.into()),
                release: StopDisposition::DirtyRelease,
            },
        };
        // Retain the seed charge on ANY ambiguous creation, open, release or
        // removal (including when create_hidden succeeded before identity/write failed).
        let seed_removed = if !seed_created {
            true
        } else {
            identity.is_some_and(|id| sink::discard_owned(&trial.leaf, SEED, id).is_ok())
                && listing_bounded(
                    &trial.leaf,
                    4_096,
                    Some(Instant::now() + Duration::from_secs(2)),
                )
                .is_ok_and(|entries| {
                    !entries
                        .iter()
                        .any(|(name, _)| name.eq_ignore_ascii_case(SEED))
                })
        };
        let release = if observed.release != StopDisposition::ChargedQuarantine
            && seed_removed
            && !slot.slot().state.borrow().uncertain
        {
            slot.settle(observed.release)
        } else {
            slot.settle(StopDisposition::ChargedQuarantine)
        };
        let report = host.network_report.get();
        let host_controls_post = host.post_controls.get();
        let clean_guest_shutdown = host.stop_done.get() == Some(StopProtocol::QmpPowerdown)
            && observed.release == StopDisposition::CleanRelease;
        let nic_setup_verified = report
            .is_some_and(|r| r.nic_ready && r.ipv4_lease && r.default_route && r.ipv6_disabled);
        let denied_probes = report.map_or(0, |r| r.denied_probes);
        let seed_released = seed_removed && release == StopDisposition::CleanRelease;
        let diagnostics_complete = observed.paused.is_ok()
            && host_controls_pre
            && host_controls_post
            && nic_setup_verified
            && denied_probes == 8
            && zero_hostfwd
            && clean_guest_shutdown
            && seed_released;
        NetworkProbe {
            refusal: (!diagnostics_complete).then_some(reason),
            release,
        }
    }
}

struct JobWatch(OwnedHandle);

const JOB_KEY: usize = 1;

impl JobWatch {
    fn attach(job: &Job) -> io::Result<Self> {
        use windows_sys::Win32::System::IO::CreateIoCompletionPort;
        let port = unsafe {
            owned(CreateIoCompletionPort(
                INVALID_HANDLE_VALUE,
                null_mut(),
                0,
                1,
            ))
        }?;
        let association = JOBOBJECT_ASSOCIATE_COMPLETION_PORT {
            CompletionKey: JOB_KEY as *mut _,
            CompletionPort: port.as_raw_handle(),
        };
        succeeded(unsafe {
            SetInformationJobObject(
                job.0.as_raw_handle(),
                JobObjectAssociateCompletionPortInformation,
                std::ptr::from_ref(&association).cast(),
                size_of_val(&association) as u32,
            )
        })?;
        Ok(Self(port))
    }

    fn next(&self) -> io::Result<Option<(u32, usize)>> {
        use windows_sys::Win32::System::IO::GetQueuedCompletionStatus;
        let (mut message, mut key, mut pid) = (0, 0, null_mut());
        let ok = unsafe {
            GetQueuedCompletionStatus(self.0.as_raw_handle(), &mut message, &mut key, &mut pid, 0)
        };
        if ok == 0 {
            let error = io::Error::last_os_error();
            return if error.raw_os_error() == Some(WAIT_TIMEOUT as i32) && pid.is_null() {
                Ok(None)
            } else {
                Err(error)
            };
        }
        if key != JOB_KEY {
            return Err(io::Error::other("unexpected Job completion source"));
        }
        Ok(Some((message, pid as usize)))
    }

    /// Wait for BOTH the held child process handle to be signaled AND an
    /// attributable `JOB_OBJECT_MSG_ACTIVE_PROCESS_ZERO` on our private IOCP.
    /// Returns `(child_exited, job_empty_notified)`.
    async fn empty_after_loss(
        &self,
        process: &OwnedHandle,
        pid: u32,
        deadline: Tick,
    ) -> io::Result<(bool, bool)> {
        use windows_sys::Win32::System::SystemServices::{
            JOB_OBJECT_MSG_ABNORMAL_EXIT_PROCESS, JOB_OBJECT_MSG_ACTIVE_PROCESS_ZERO,
            JOB_OBJECT_MSG_EXIT_PROCESS,
        };
        let mut empty = false;
        let mut exited = false;
        for _ in 0..512 {
            if Tick::now() >= deadline {
                break;
            }
            match self.next()? {
                Some((JOB_OBJECT_MSG_ACTIVE_PROCESS_ZERO, 0)) if !empty => empty = true,
                Some((
                    JOB_OBJECT_MSG_EXIT_PROCESS | JOB_OBJECT_MSG_ABNORMAL_EXIT_PROCESS,
                    observed,
                )) if observed == pid as usize => {}
                Some(_) => return Err(io::Error::other("unexpected Job loss notice")),
                None => {
                    tokio::time::sleep_until(deadline.min(Tick::now() + Duration::from_millis(10)))
                        .await
                }
            }
            exited |= wait(process, 0)?;
            if empty && exited {
                return Ok((true, true));
            }
        }
        Ok((exited, empty))
    }
}

struct Prepared<'a> {
    host: Windows<'a>,
    image: LocalPath,
    data: LocalPath,
    bios: LocalPath,
}

impl<'a> Prepared<'a> {
    fn new(baseline: &'a Baseline) -> Result<Self, NativeRefusal> {
        let held = &baseline._held;
        let reason = NativeRefusal::DistributionUnverified;
        require_paused(held.paused || held.full_test)?;
        let bundle = held.bundle.as_ref().ok_or(reason)?;
        let limits = match &baseline.observations.limits {
            Taken::Observed(limits) => &limits.result,
            Taken::Refused { reason, .. } => return Err(*reason),
        };
        if let Taken::Refused { reason, .. } = &baseline.observations.host {
            return Err(*reason);
        }
        let reason = NativeRefusal::PathCustodyUnverified;
        let cwd = held.trial.path.join(CWD).map_err(|_| reason)?;
        let cwd = super::super::capacity::walk(
            &super::super::capacity::MountManager,
            &held.volumes[0],
            &cwd,
            true,
            false,
        )?;
        let bundle_root = &bundle.root.path;
        let image = bundle_root.join(IMAGE).map_err(|_| reason)?;
        let data = bundle_root.join("share").map_err(|_| reason)?;
        let bios = data.join(FIRMWARE).map_err(|_| reason)?;
        Ok(Self {
            host: Windows {
                baseline,
                bundle,
                limits,
                cwd,
                completed: Cell::new(PausedCompleted::default()),
                armed_fault: Cell::new(None),
                fault_sample: Cell::new(None),
                prelaunch_quiet: Cell::new(false),
                sec_marker_verified: Cell::new(false),
                network_report: Cell::new(None),
                post_controls: Cell::new(false),
                stop_done: Cell::new(None),
                loss_child_exited: Cell::new(false),
                loss_job_empty: Cell::new(false),
            },
            image,
            data,
            bios,
        })
    }
}

struct Windows<'a> {
    baseline: &'a Baseline,
    bundle: &'a inventory::Bundle,
    limits: &'a Limits,
    cwd: PathPin,
    completed: Cell<PausedCompleted>,
    armed_fault: Cell<Option<ControlledFault>>,
    fault_sample: Cell<Option<FaultRecord>>,
    prelaunch_quiet: Cell<bool>,
    sec_marker_verified: Cell<bool>,
    network_report: Cell<Option<ProbeReport>>,
    post_controls: Cell<bool>,
    stop_done: Cell<Option<StopProtocol>>,
    loss_child_exited: Cell<bool>,
    loss_job_empty: Cell<bool>,
}

struct Opens<'a> {
    bundle: &'a inventory::Bundle,
    image: FileIdentity,
    envelope: JobEnvelope,
    /// Media pins (ISO, kernel, initrd, seed) rechecked at create and after prelaunch.
    media: Vec<(LocalPath, FileIdentity, u64)>,
}

struct Slot {
    charge: File,
    state: RefCell<ChildState>,
}

#[derive(Default)]
struct ChildState {
    child: Option<Rc<Child>>,
    stop_done: Option<StopProtocol>,
    watch: Option<JobWatch>,
    lost: Option<(OwnedHandle, u32)>,
    parent: Option<Stdio>,
    workers: Option<WorkerSet>,
    uncertain: bool,
}

struct LaunchGate<'a> {
    cwd: &'a PathPin,
    owner: PSID,
}

impl LaunchGate<'_> {
    fn validate_search_and_bundle(
        &self,
        bundle: &inventory::Bundle,
        safe_search: bool,
    ) -> Result<(), NativeRefusal> {
        inventory::validate_launch_namespace(bundle, self.cwd, self.owner, safe_search)
    }

    fn verify_opens(
        &self,
        bundle: &inventory::Bundle,
        safe_search: bool,
        verify_live: impl FnOnce() -> Result<(), NativeRefusal>,
    ) -> Result<(), NativeRefusal> {
        verify_live()?;
        self.validate_search_and_bundle(bundle, safe_search)
    }
}

impl Windows<'_> {
    fn confirm_claim_for_seed(
        &self,
        slot: &Slot,
        envelope: JobEnvelope,
    ) -> Result<(), NativeRefusal> {
        if slot.state.borrow().uncertain {
            return Err(NativeRefusal::DiskCeilingUnverified);
        }
        let live = self.live(envelope)?;
        live.check(envelope, Tick::now())
            .map_err(|_| NativeRefusal::ReservesUnverified)
    }

    fn launch_gate(&self) -> LaunchGate<'_> {
        LaunchGate {
            cwd: &self.cwd,
            owner: self.baseline._held.token.account.0,
        }
    }

    fn verify_media_bindings(&self, plan: &PausedPlan<'_>) -> Result<(), NativeRefusal> {
        if let LaunchPurpose::Network(network) = &plan.purpose {
            let reason = NativeRefusal::DistributionUnverified;
            for pin in network.source.media.iter().chain([network.seed]) {
                if pin.mutable || pin.verified_content.is_none() {
                    return Err(reason);
                }
                inventory::binding(pin)?;
                let (id, _) = file_identity(&pin.leaf, false).map_err(|_| reason)?;
                let len = pin.leaf.metadata().map_err(|_| reason)?.len();
                if id != pin.identity
                    || pin
                        .verified_content
                        .is_none_or(|expected| expected.size != len)
                {
                    return Err(reason);
                }
            }
        }
        Ok(())
    }

    fn verify_opens_media(&self, opens: &Opens<'_>) -> Result<(), NativeRefusal> {
        if opens.media.is_empty() {
            return Ok(());
        }
        let reason = NativeRefusal::DistributionUnverified;
        let network = self.baseline._held.network.as_ref().ok_or(reason)?;
        for (pin, (expected_path, expected_id, expected_size)) in
            network.media.iter().zip(opens.media.iter())
        {
            inventory::binding(pin)?;
            let (id, _) = file_identity(&pin.leaf, false).map_err(|_| reason)?;
            let len = pin.leaf.metadata().map_err(|_| reason)?.len();
            if pin.path != *expected_path || id != *expected_id || len != *expected_size {
                return Err(reason);
            }
        }
        // Recheck the held seed drive binding and identity on the trial volume.
        if let Some((seed_path, seed_id, seed_size)) = opens.media.get(3) {
            let seed_pin = super::super::capacity::walk(
                &super::super::capacity::MountManager,
                &self.baseline._held.volumes[0],
                seed_path,
                false,
                false,
            )?;
            let len = seed_pin.leaf.metadata().map_err(|_| reason)?.len();
            if seed_pin.identity != *seed_id || len != *seed_size {
                return Err(reason);
            }
        }
        Ok(())
    }

    fn live_limits(&self) -> Result<Limits, NativeRefusal> {
        use super::super::capacity::{MountManager, held_free, still_bound};

        let held = &self.baseline._held;
        let inner = held_free(&held.trial.leaf)?;
        let outer = held_free(&held.volumes[1].root)?;
        if inner.total != self.limits.inner_total_bytes
            || !held
                .volumes
                .iter()
                .all(|volume| still_bound(&MountManager, volume))
        {
            return Err(NativeRefusal::DiskCeilingUnverified);
        }
        Ok(Limits {
            inner_caller_free_bytes: inner.caller,
            inner_total_free_bytes: inner.total_free,
            outer_caller_free_bytes: outer.caller,
            outer_total_free_bytes: outer.total_free,
            ..*self.limits
        })
    }
}

impl<'a> Host for Windows<'a> {
    type Opens = Opens<'a>;
    type Slot = Slot;
    type Reader = tokio::io::DuplexStream;
    type Writer = tokio::io::DuplexStream;
    type Channel = tokio::net::windows::named_pipe::NamedPipeClient;

    fn open_contract(
        &mut self,
        plan: &PausedPlan<'_>,
    ) -> Result<(Opens<'a>, JobEnvelope), NativeRefusal> {
        let reason = NativeRefusal::DistributionUnverified;
        let root = &self.bundle.root.path;
        let expected_image = root.join(IMAGE).map_err(|_| reason)?;
        let expected_data = root.join("share").map_err(|_| reason)?;
        if *plan.image != expected_image
            || *plan.data != expected_data
            || *plan.bios != expected_data.join(FIRMWARE).map_err(|_| reason)?
        {
            return Err(reason);
        }
        let image = self.bundle.identity(IMAGE)?;
        self.bundle.identity("share/bios-256k.bin")?;
        let mut media = Vec::new();
        match &plan.purpose {
            LaunchPurpose::Firmware(firmware) => {
                if !self.baseline._held.full_test
                    || *firmware.code != expected_data.join(CODE).map_err(|_| reason)?
                    || *firmware.vars != expected_data.join(VARS).map_err(|_| reason)?
                {
                    return Err(reason);
                }
                self.bundle.identity("share/edk2-x86_64-code.fd")?;
                self.bundle.identity("share/edk2-i386-vars.fd")?;
            }
            LaunchPurpose::Network(network) => {
                if !self.baseline._held.full_test
                    || *network.code != expected_data.join(CODE).map_err(|_| reason)?
                    || *network.vars != expected_data.join(VARS).map_err(|_| reason)?
                {
                    return Err(reason);
                }
                self.verify_media_bindings(plan)?;
                for pin in network.source.media.iter().chain([network.seed]) {
                    let expected = pin.verified_content.ok_or(reason)?;
                    media.push((pin.path.clone(), pin.identity, expected.size));
                }
                self.bundle.identity("share/edk2-x86_64-code.fd")?;
                self.bundle.identity("share/edk2-i386-vars.fd")?;
            }
            LaunchPurpose::SupervisorLoss | LaunchPurpose::ControlledFault(_) => {
                if !self.baseline._held.full_test {
                    return Err(NativeRefusal::QualificationMissing);
                }
            }
            LaunchPurpose::Paused => {}
        }
        self.launch_gate()
            .validate_search_and_bundle(self.bundle, safe_dll_search())?;
        let envelope = plan.purpose.envelope();
        Ok((
            Opens {
                bundle: self.bundle,
                image,
                envelope,
                media,
            },
            envelope,
        ))
    }

    fn live(&self, envelope: JobEnvelope) -> Result<Live, NativeRefusal> {
        let captured = Tick::now();
        // Capture the checked two-sample WTS/LSA observation once so any armed
        // WtsAmbiguity fault records original facts from this exact observation.
        let mut console = console_evidence(&self.baseline._held.token)?;
        let idle = console_decision(&console)?;
        let (active_cpus, available_ram_bytes) = host_resources()?;
        let effective_cpus = cpu_scope()?;
        let whpx = hypervisor_present()?;
        let limits = self.live_limits()?;
        let mut live = Live {
            captured,
            idle,
            active_cpus,
            effective_cpus,
            available_ram_bytes,
            whpx,
            limits,
            // A single exclusive filesystem claim serializes all runs on
            // this held trial root, including an unsettled previous run.
            outstanding_inner_bytes: 0,
            outstanding_outer_bytes: 0,
        };
        if let Some(fault) = self.armed_fault.take() {
            // Confirm the unmodified fresh sample passes admission before mutating.
            live.check(envelope, Tick::now())
                .map_err(|_| NativeRefusal::ReservesUnverified)?;
            let original = FaultRecord {
                fault,
                sampled_ms: u64::try_from(
                    live.captured
                        .into_std()
                        .duration_since(self.baseline.origin)
                        .as_millis(),
                )
                .map_err(|_| NativeRefusal::ReservesUnverified)?,
                settled_ms: 0,
                console_idle: live.idle,
                effective_cpus: live.effective_cpus,
                available_ram_bytes: live.available_ram_bytes,
                inner_free_bytes: live.limits.inner_caller_free_bytes,
                outer_free_bytes: live.limits.outer_caller_free_bytes,
            };
            self.fault_sample.set(Some(original));
            match fault {
                ControlledFault::WtsAmbiguity => {
                    // Mutate the exact checked two-sample WTS/LSA observation.
                    console.accounts[1] = None;
                    let _ = console_decision(&console)?;
                }
                ControlledFault::CpuReserve => {
                    live.effective_cpus = 1;
                }
                ControlledFault::RamReserve => {
                    live.available_ram_bytes = RAM_RESERVE;
                }
                ControlledFault::DiskReserve => {
                    live.limits.inner_caller_free_bytes = 0;
                }
            }
        }
        Ok(live)
    }

    fn claim(&mut self, _: &Live, _: JobEnvelope) -> Result<Slot, NativeRefusal> {
        Slot::claim(&self.baseline._held.trial.leaf, self.baseline.run())
    }

    async fn confirm_claim(&self, slot: &Slot) -> Result<(), NativeRefusal> {
        if slot.state.borrow().uncertain {
            return Err(NativeRefusal::DiskCeilingUnverified);
        }
        Ok(())
    }

    async fn create_suspended(
        &self,
        slot: &Slot,
        plan: &PausedPlan<'_>,
        opens: &Opens<'a>,
    ) -> Result<(), NativeRefusal> {
        let token = &self.baseline._held.token;
        let (stdio, parent) = token.stdio().map_err(|_| NativeRefusal::QmpUnverified)?;
        let job = Job::with_process_limit(
            opens.envelope.job_memory_bytes,
            Some(opens.envelope.job_cpu_rate),
            Some(1),
        )
        .map_err(|_| NativeRefusal::JobUnverified)?;
        let watch = if plan.purpose.needs_job_watch() {
            Some(JobWatch::attach(&job).map_err(|_| NativeRefusal::JobUnverified)?)
        } else {
            None
        };
        let env = environment(&self.cwd.path)?;
        let args = plan.args();
        let launch = Launch::Paused {
            token: &token.token,
            image: plan.image,
            args: &args,
        };
        // No child creation precedes the final held inventory, media-binding
        // and namespace check. Custody stays in the borrowed Baseline through settlement.
        self.launch_gate()
            .validate_search_and_bundle(opens.bundle, safe_dll_search())?;
        self.verify_media_bindings(plan)?;
        let live = self.live(opens.envelope)?;
        live.check(opens.envelope, Tick::now())
            .map_err(|error| match error {
                Failure::Refused(reason) => reason,
                _ => NativeRefusal::OwnerActivityUnverified,
            })?;
        let mut state = slot.state.borrow_mut();
        let (process, thread) = create_suspended(
            &launch,
            Path::new(self.cwd.path.as_str()),
            &env,
            Assignment::Job(&job),
            &stdio,
        )
        .map_err(|error| {
            state.uncertain = error.stage == SpawnStage::Verify || !matches!(job.active(), Ok(0));
            NativeRefusal::JobUnverified
        })?;
        state.child = Some(Rc::new(Child {
            job,
            process,
            suspended: Some(thread),
        }));
        state.parent = Some(parent);
        state.watch = watch;
        let child = state.child.as_ref().expect("child just stored");
        verify_image(child, plan.image, opens.image)?;
        Ok(())
    }

    async fn verify_child(
        &self,
        slot: &Slot,
        envelope: JobEnvelope,
    ) -> Result<Pipes<Self::Reader, Self::Writer>, NativeRefusal> {
        let mut state = slot.state.borrow_mut();
        let child = state.child.as_ref().ok_or(NativeRefusal::JobUnverified)?;
        verify_child_policy(child, &self.baseline._held.token)?;
        // Independently observe effective Job limits after process assignment.
        child
            .job
            .verify_limits(
                envelope.job_memory_bytes,
                Some(envelope.job_cpu_rate),
                Some(1),
            )
            .map_err(|_| NativeRefusal::JobUnverified)?;
        let parent = state.parent.take().ok_or(NativeRefusal::QmpUnverified)?;
        let dispatch = Arc::new(DispatchGate::default());
        let (stdout, stdin, workers) = start_stdio(parent, dispatch.clone()).map_err(|_| {
            state.uncertain = true;
            NativeRefusal::QmpUnverified
        })?;
        state.workers = Some(workers);
        Ok(Pipes {
            stdout,
            stdin,
            dispatch,
        })
    }

    async fn resume(&self, slot: &Slot) -> Result<(), NativeRefusal> {
        slot.state
            .borrow_mut()
            .child
            .as_mut()
            .and_then(Rc::get_mut)
            .ok_or(NativeRefusal::JobUnverified)?
            .resume()
            .map_err(|_| NativeRefusal::JobUnverified)?;
        Ok(())
    }

    async fn verify_opens(&self, slot: &Slot, opens: &Opens<'a>) -> Result<(), NativeRefusal> {
        self.launch_gate()
            .verify_opens(opens.bundle, safe_dll_search(), || require_live(slot))?;
        self.verify_opens_media(opens)
    }

    async fn verify_whpx(
        &self,
        slot: &Slot,
        query: impl Future<Output = Result<(), Failure>>,
    ) -> Result<(), Failure> {
        require_live(slot)?;
        query.await?;
        require_live(slot)?;
        Ok(())
    }

    async fn open_channel(
        &self,
        slot: &Slot,
        name: &SerialName,
        cancel: &Shutdown,
        cutoff: Tick,
    ) -> Result<Self::Channel, NativeRefusal> {
        let child = slot
            .state
            .borrow()
            .child
            .clone()
            .ok_or(NativeRefusal::GuestEndpointUnverified)?;
        let token = &self.baseline._held.token;
        connect_serial(
            name,
            &child,
            token.account.0,
            &token.entries(),
            cancel,
            cutoff,
        )
        .await
    }

    fn mark_phase_complete(&self, _: &Slot, phase: PausedPhase) {
        let mut done = self.completed.get();
        match phase {
            PausedPhase::SuspendedChild => done.suspended_child = true,
            PausedPhase::Prelaunch => done.prelaunch = true,
            PausedPhase::BundleInventory => done.bundle_inventory = true,
        }
        self.completed.set(done);
    }

    fn mark_prelaunch_quiet(&self, _: &Slot) {
        self.prelaunch_quiet.set(true);
    }

    fn mark_sec_marker(&self, _: &Slot) {
        self.sec_marker_verified.set(true);
    }

    fn record_network_report(&self, _: &Slot, report: ProbeReport) {
        self.network_report.set(Some(report));
    }

    fn mark_post_controls(&self, _: &Slot) {
        self.post_controls.set(true);
    }

    fn mark_stop_protocol_done(&self, slot: &Slot, protocol: StopProtocol) {
        slot.state.borrow_mut().stop_done = Some(protocol);
        self.stop_done.set(Some(protocol));
    }

    fn arm_fault(&self, slot: &Slot, fault: ControlledFault) -> Result<(), NativeRefusal> {
        require_live(slot)?;
        self.armed_fault.set(Some(fault));
        Ok(())
    }

    fn lose_supervisor(&self, slot: &Slot) -> Result<(), NativeRefusal> {
        use windows_sys::Win32::System::SystemServices::JOB_OBJECT_MSG_NEW_PROCESS;
        let reason = NativeRefusal::JobUnverified;
        let mut state = slot.state.borrow_mut();
        let child = state.child.as_ref().ok_or(reason)?;
        let pid = child.verify_live().map_err(|_| reason)?;
        let mut flags = 0;
        if unsafe { GetHandleInformation(child.job.0.as_raw_handle(), &mut flags) } == 0
            || flags & HANDLE_FLAG_INHERIT != 0
            || !matches!(child.job.active(), Ok(1))
        {
            return Err(reason);
        }
        let watch = state.watch.as_ref().ok_or(reason)?;
        let mut saw_new = false;
        let deadline = Instant::now() + Duration::from_millis(250);
        while Instant::now() < deadline {
            match watch.next().map_err(|_| reason)? {
                Some((JOB_OBJECT_MSG_NEW_PROCESS, observed)) if observed == pid as usize => {
                    saw_new = true;
                    break;
                }
                Some(_) => return Err(reason),
                None => std::thread::sleep(Duration::from_millis(5)),
            }
        }
        if !saw_new {
            return Err(reason);
        }
        // `Rc::try_unwrap` proves no other Rust handle retains the Job;
        // dropping `job` closes the actual last Job handle.
        let child = state.child.take().ok_or(reason)?;
        match Rc::try_unwrap(child) {
            Ok(Child { job, process, .. }) => {
                state.lost = Some((process, pid));
                drop(job);
                Ok(())
            }
            Err(child) => {
                state.child = Some(child);
                Err(reason)
            }
        }
    }

    fn completed_stop_protocol(&self, slot: &Slot) -> Option<StopProtocol> {
        slot.state.borrow().stop_done
    }

    fn request_stop(&mut self, slot: &Slot) {
        let mut state = slot.state.borrow_mut();
        if !matches!(
            state.stop_done,
            Some(StopProtocol::QmpQuit | StopProtocol::QmpPowerdown)
        ) && let Some(child) = &state.child
        {
            let _ = child.job.terminate();
        }
        if let Some(workers) = &state.workers {
            workers.stop();
        }
        state.parent = None;
    }

    async fn observe_stop(&mut self, slot: &Slot, deadline: Tick) -> StopEvidence {
        let (child, workers, uncertain, lost, watch, stop_done) = {
            let mut state = slot.state.borrow_mut();
            (
                state.child.take(),
                state.workers.take(),
                state.uncertain,
                state.lost.take(),
                state.watch.take(),
                state.stop_done,
            )
        };
        let (loss_child_exited, loss_job_empty) = match (&lost, &watch) {
            (Some((process, pid)), Some(watch)) => watch
                .empty_after_loss(process, *pid, deadline)
                .await
                .unwrap_or((false, false)),
            (None, _) => (true, true),
            _ => (false, false),
        };
        if lost.is_some() {
            self.loss_child_exited.set(loss_child_exited);
            self.loss_job_empty.set(loss_job_empty);
        }
        let loss_released = supervisor_loss_released(loss_child_exited, loss_job_empty);
        let reaped = match &child {
            Some(child) => child.reap(deadline).await.is_ok(),
            None => true,
        };
        let workers_joined = match workers {
            Some(workers) => workers.join(deadline).await.is_ok(),
            None => true,
        };
        let process_exited = child
            .as_ref()
            .is_none_or(|child| matches!(child.exited(), Ok(true)));
        let job_empty = child
            .as_ref()
            .is_none_or(|child| matches!(child.job.active(), Ok(0)));
        let clean_process_exit = child
            .as_ref()
            .is_some_and(|child| child.exit_code() == Ok(0));
        slot.state.borrow_mut().child = child;
        StopEvidence {
            guest_shutdown: stop_done == Some(StopProtocol::QmpPowerdown),
            process_exited: reaped && process_exited && loss_released && !uncertain,
            job_empty: reaped && job_empty && loss_released && !uncertain,
            clean_process_exit,
            workers_joined: workers_joined && !uncertain,
            ..StopEvidence::default()
        }
    }

    async fn reopen(&mut self, _: &Slot, deadline: Tick) -> bool {
        // Diskless: the charge is the only writable file and its original
        // exclusive create handle never leaves the slot.
        self.bundle.check(deadline.into_std()).is_ok()
            && inventory::binding(&self.baseline._held.trial).is_ok()
    }

    fn settle(&mut self, slot: Slot, disposition: StopDisposition) -> StopDisposition {
        slot.settle(disposition)
    }
}

impl Slot {
    fn claim(root: &File, run: [u8; 16]) -> Result<Self, NativeRefusal> {
        Self::claim_named(root, run, SLOT)
    }

    fn claim_named(root: &File, run: [u8; 16], name: &str) -> Result<Self, NativeRefusal> {
        let reason = NativeRefusal::DiskCeilingUnverified;
        // FILE_CREATE with no sharing is the cross-process arbitration.
        // Any existing generation, even an empty crash remnant, refuses.
        let mut charge = sink::create_hidden(root, name).map_err(|_| reason)?;
        let state = RefCell::new(ChildState {
            uncertain: charge
                .write_all(&run)
                .and_then(|()| charge.sync_all())
                .is_err(),
            ..ChildState::default()
        });
        // The caller settles even if flush failed. No child can start
        // until confirm_claim accepts the persisted charge.
        Ok(Self { charge, state })
    }

    fn settle(self, disposition: StopDisposition) -> StopDisposition {
        if disposition == StopDisposition::ChargedQuarantine || sink::discard(&self.charge).is_err()
        {
            StopDisposition::ChargedQuarantine
        } else {
            disposition
        }
    }
}

fn require_live(slot: &Slot) -> Result<(), NativeRefusal> {
    slot.state
        .borrow()
        .child
        .as_ref()
        .ok_or(NativeRefusal::JobUnverified)?
        .verify_live()
        .map(|_| ())
        .map_err(|_| NativeRefusal::JobUnverified)
}

fn cpu_scope() -> Result<u32, NativeRefusal> {
    let process = unsafe { GetCurrentProcess() };
    let (mut in_job, mut current, mut system) = (0, 0, 0);
    // Refuse nesting and multiple processor groups until there is a measured
    // effective cap for those configurations.
    if unsafe {
        IsProcessInJob(process, null_mut(), &mut in_job) == 0
            || in_job != 0
            || GetActiveProcessorGroupCount() != 1
            || GetProcessAffinityMask(process, &mut current, &mut system) == 0
    } {
        return Err(NativeRefusal::ReservesUnverified);
    }
    Ok(current.count_ones())
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::vm::windows::qualification::inventory::tests::{
        Fixture, READ_ONLY, WRITABLE, set_dacl,
    };

    #[test]
    fn supervisor_loss_requires_both_release_signals() {
        for exited in [false, true] {
            for empty in [false, true] {
                assert_eq!(
                    supervisor_loss_released(exited, empty),
                    (exited, empty) == (true, true)
                );
            }
        }
    }

    #[test]
    fn controlled_fault_requires_its_own_refusal_and_dirty_release() {
        for fault in ControlledFault::ALL {
            let expected = Err(Failure::Refused(expected_fault_refusal(fault)));
            let result = TrialResult {
                paused: expected,
                release: StopDisposition::DirtyRelease,
            };
            assert!(fault_settled(fault, &result));
            assert!(!fault_settled(
                fault,
                &TrialResult {
                    paused: Err(Failure::Refused(NativeRefusal::JobUnverified)),
                    ..result
                }
            ));
            assert!(!fault_settled(
                fault,
                &TrialResult {
                    release: StopDisposition::CleanRelease,
                    ..result
                }
            ));
        }
    }

    #[tokio::test]
    async fn supervisor_loss_needs_job_empty_evidence_even_if_the_child_has_exited() {
        use windows_sys::Win32::System::{
            IO::PostQueuedCompletionStatus, SystemServices::JOB_OBJECT_MSG_ACTIVE_PROCESS_ZERO,
        };

        let job = Job::with_process_limit(JOB_MEMORY_BYTES, None, Some(1)).unwrap();
        let watch = JobWatch::attach(&job).unwrap();
        let signaled = unsafe { owned(CreateEventW(null(), 1, 1, null())) }.unwrap();
        let deadline = Tick::now() + Duration::from_millis(40);
        // Missing empty notice returns (exited=true, empty=false) => inconclusive.
        assert_eq!(
            watch
                .empty_after_loss(&signaled, 11, deadline)
                .await
                .unwrap(),
            (true, false)
        );

        succeeded(unsafe {
            PostQueuedCompletionStatus(
                watch.0.as_raw_handle(),
                JOB_OBJECT_MSG_ACTIVE_PROCESS_ZERO,
                JOB_KEY,
                null(),
            )
        })
        .unwrap();
        let deadline = Tick::now() + Duration::from_secs(1);
        assert_eq!(
            watch
                .empty_after_loss(&signaled, 11, deadline)
                .await
                .unwrap(),
            (true, true)
        );
    }

    fn owner_snapshot(
        active_console: u32,
        state: i32,
        flags: i32,
        input: i64,
        current: i64,
        witness: Option<(u32, i32, &str, &str)>,
    ) -> ConsoleTransitionSnapshot {
        let wts = WtsSample {
            session: 2,
            state,
            flags,
            logon: 1_000,
            input,
            current,
        };
        let lsa = LsaLogon {
            logon_type: LOGON_INTERACTIVE,
            session: 2,
            logon_time: 1_050,
            owner: true,
            user: "Owner".to_owned(),
            domain: "AzureAD".to_owned(),
        };
        let account = Some(WtsAccount {
            user: "Owner".to_owned(),
            domain: "AzureAD".to_owned(),
        });
        let witness = witness.map(|(sid, wstate, user, domain)| {
            let ws = WtsSample {
                session: sid,
                state: wstate,
                flags: WTS_SESSIONSTATE_UNLOCK as i32,
                logon: 2_000,
                input: current,
                current,
            };
            let wa = Some(WtsAccount {
                user: user.to_owned(),
                domain: domain.to_owned(),
            });
            ([ws, ws], [wa.clone(), wa])
        });
        ConsoleTransitionSnapshot {
            token_stable: true,
            owner_session: 2,
            active_console,
            owner_lsa: [lsa.clone(), lsa],
            owner_wts: [wts, wts],
            owner_accounts: [account.clone(), account],
            witness,
            now: current + 10,
        }
    }

    #[test]
    fn console_witness_enforces_ordered_corroborated_transitions_cursor_witness_and_idle() {
        let enrolled = WtsAccount {
            user: "Witness".to_owned(),
            domain: "LOCAL".to_owned(),
        };
        let t0 = 1_000 + IDLE_TICKS;
        let initial = owner_snapshot(
            2,
            WTSActive,
            WTS_SESSIONSTATE_UNLOCK as i32,
            1_000,
            t0,
            None,
        );
        let mut machine = ConsoleWitnessMachine::begin(enrolled.clone(), &initial).unwrap();

        // Phase 0: pre-prompt stale input stays Pending, does NOT advance.
        let stale_input = owner_snapshot(
            2,
            WTSActive,
            WTS_SESSIONSTATE_UNLOCK as i32,
            1_000,
            t0 + 100,
            None,
        );
        assert_eq!(machine.step(&stale_input), ConsoleStep::Pending);
        assert_eq!(machine.step(&stale_input), ConsoleStep::Pending);

        // Phase 0: new input requires two consecutive corroborated polls.
        let p0 = owner_snapshot(
            2,
            WTSActive,
            WTS_SESSIONSTATE_UNLOCK as i32,
            t0 + 200,
            t0 + 250,
            None,
        );
        assert_eq!(machine.step(&p0), ConsoleStep::Pending);
        assert_eq!(machine.step(&p0), ConsoleStep::PhaseMatched(0));

        // Phase 1: lock.
        let p1 = owner_snapshot(
            2,
            WTSActive,
            WTS_SESSIONSTATE_LOCK as i32,
            t0 + 200,
            t0 + 300,
            None,
        );
        assert_eq!(machine.step(&p1), ConsoleStep::Pending);
        assert_eq!(machine.step(&p1), ConsoleStep::PhaseMatched(1));

        // Phase 2: wrong witness closes immediately as Invalid.
        let wrong_witness = owner_snapshot(
            3,
            WTSActive,
            WTS_SESSIONSTATE_LOCK as i32,
            t0 + 200,
            t0 + 350,
            Some((3, WTSActive, "Intruder", "LOCAL")),
        );
        assert_eq!(
            machine.step(&wrong_witness),
            ConsoleStep::Invalid(NativeRefusal::OwnerActivityUnverified)
        );

        // Phase 2: enrolled witness succeeds with corroboration.
        let p2 = owner_snapshot(
            3,
            WTSActive,
            WTS_SESSIONSTATE_LOCK as i32,
            t0 + 200,
            t0 + 400,
            Some((3, WTSActive, "Witness", "LOCAL")),
        );
        assert_eq!(machine.step(&p2), ConsoleStep::Pending);
        assert_eq!(machine.step(&p2), ConsoleStep::PhaseMatched(2));

        // Phase 3: owner disconnected while enrolled witness active.
        let p3 = owner_snapshot(
            3,
            WTSDisconnected,
            WTS_SESSIONSTATE_LOCK as i32,
            t0 + 200,
            t0 + 500,
            Some((3, WTSActive, "Witness", "LOCAL")),
        );
        assert_eq!(machine.step(&p3), ConsoleStep::Pending);
        assert_eq!(machine.step(&p3), ConsoleStep::PhaseMatched(3));

        // Phase 4: return to original unlocked owner session with fresh unlock input.
        let p4 = owner_snapshot(
            2,
            WTSActive,
            WTS_SESSIONSTATE_UNLOCK as i32,
            t0 + 600,
            t0 + 650,
            None,
        );
        assert_eq!(machine.step(&p4), ConsoleStep::Pending);
        assert_eq!(machine.step(&p4), ConsoleStep::PhaseMatched(4));

        // Phase 5: recent input stays Pending until a fresh idle window passes.
        assert_eq!(machine.step(&p4), ConsoleStep::Pending);
        let p5_idle = owner_snapshot(
            2,
            WTSActive,
            WTS_SESSIONSTATE_UNLOCK as i32,
            t0 + 600,
            t0 + 600 + IDLE_TICKS,
            None,
        );
        assert_eq!(machine.step(&p5_idle), ConsoleStep::Pending);
        assert_eq!(machine.step(&p5_idle), ConsoleStep::IdleVerified);
    }

    #[test]
    fn console_witness_rejects_lost_owner_logon_and_mismatched_session_id() {
        let enrolled = WtsAccount {
            user: "Witness".to_owned(),
            domain: "LOCAL".to_owned(),
        };
        let t0 = 1_000 + IDLE_TICKS;
        let initial = owner_snapshot(
            2,
            WTSActive,
            WTS_SESSIONSTATE_UNLOCK as i32,
            1_000,
            t0,
            None,
        );
        let mut machine = ConsoleWitnessMachine::begin(enrolled, &initial).unwrap();

        let mut lost_logon = initial.clone();
        lost_logon.owner_wts[0].logon = 1_001;
        lost_logon.owner_wts[1].logon = 1_001;
        assert_eq!(
            machine.step(&lost_logon),
            ConsoleStep::Invalid(NativeRefusal::OwnerActivityUnverified)
        );

        let mut bad_sid = initial.clone();
        bad_sid.owner_wts[1].session = 99;
        assert_eq!(
            machine.step(&bad_sid),
            ConsoleStep::Invalid(NativeRefusal::OwnerActivityUnverified)
        );

        let mut unstable_token = initial;
        unstable_token.token_stable = false;
        assert_eq!(
            machine.step(&unstable_token),
            ConsoleStep::Invalid(NativeRefusal::OwnerActivityUnverified)
        );
    }

    #[test]
    fn capacity_denials_verify_all_five_independent_boundaries() {
        let live = Live {
            captured: Tick::now(),
            idle: true,
            active_cpus: 8,
            effective_cpus: 8,
            available_ram_bytes: 32 * GIB,
            whpx: true,
            limits: Limits {
                inner_caller_free_bytes: 70 * GIB,
                inner_total_free_bytes: 71 * GIB,
                inner_total_bytes: 99 * GIB,
                inner_extent_bytes: 99 * GIB,
                vhd_virtual_bytes: 100 * GIB,
                outer_caller_free_bytes: 60 * GIB,
                outer_total_free_bytes: 61 * GIB,
                backing_allocated_bytes: 100 * GIB,
                backing_length_bytes: 100 * GIB,
                disk_ceiling_bytes: DISK_CEILING,
                outer_floor_bytes: OUTER_FLOOR,
                disk_budget_bytes: 99 * GIB,
                empty_job_memory_limit_bytes: JOB_MEMORY_BYTES as u64,
                empty_job_cpu_rate: JOB_CPU_RATE,
            },
            outstanding_inner_bytes: 0,
            outstanding_outer_bytes: 0,
        };
        assert_eq!(
            verify_capacity_denials(&live, 1 << 20, 1 << 20, |extra| extra <= 50 * GIB),
            Ok(5)
        );
        assert_eq!(
            verify_capacity_denials(&live, 1 << 20, 1 << 20, |_| true),
            Err(NativeRefusal::DiskCeilingUnverified)
        );
    }

    struct GateFixture {
        bundle: inventory::Bundle,
        fixture: Fixture,
        pin: PathPin,
        cwd: tempfile::TempDir,
        user: Vec<usize>,
    }

    impl GateFixture {
        fn new() -> Self {
            let fixture = Fixture::new();
            // Establish holds before changing the fixture DACL, so the
            // check must use those objects through the namespace change.
            let bundle = fixture
                .inspect(Instant::now() + Duration::from_secs(2))
                .unwrap();
            let cwd = tempfile::tempdir().unwrap();
            let pin =
                PathPin::open(&local(cwd.path().to_str().unwrap()).unwrap(), true, false).unwrap();
            let user = current_user().unwrap();
            let fixture = Self {
                bundle,
                fixture,
                cwd,
                pin,
                user,
            };
            for path in [
                fixture.fixture.root.path(),
                &fixture.fixture.root.path().join("share"),
                fixture.cwd.path(),
            ] {
                set_dacl(path, READ_ONLY);
            }
            fixture
        }

        fn gate(&self) -> LaunchGate<'_> {
            LaunchGate {
                cwd: &self.pin,
                owner: user_sid(&self.user),
            }
        }
    }

    impl Drop for GateFixture {
        fn drop(&mut self) {
            for path in [
                self.fixture.root.path(),
                &self.fixture.root.path().join("share"),
                self.cwd.path(),
            ] {
                set_dacl(path, WRITABLE);
            }
        }
    }

    #[test]
    fn native_launch_gate_refuses_unsafe_or_nonempty_cwd() {
        let f = GateFixture::new();
        let bundle = &f.bundle;
        let gate = f.gate();
        assert_eq!(gate.validate_search_and_bundle(bundle, true), Ok(()));
        assert_eq!(
            gate.validate_search_and_bundle(bundle, false),
            Err(NativeRefusal::PathCustodyUnverified)
        );
        set_dacl(f.cwd.path(), WRITABLE);
        assert_eq!(
            gate.validate_search_and_bundle(bundle, true),
            Err(NativeRefusal::PathCustodyUnverified)
        );
        std::fs::write(f.cwd.path().join("unexpected.bin"), b"fixture").unwrap();
        set_dacl(f.cwd.path(), READ_ONLY);
        assert_eq!(
            gate.validate_search_and_bundle(bundle, true),
            Err(NativeRefusal::PathCustodyUnverified)
        );
    }

    #[test]
    fn native_launch_gate_refuses_writable_bundle_root_or_subdirectory() {
        let f = GateFixture::new();
        let bundle = &f.bundle;
        for path in [f.fixture.root.path(), &f.fixture.root.path().join("share")] {
            assert_eq!(f.gate().validate_search_and_bundle(bundle, true), Ok(()));
            set_dacl(path, WRITABLE);
            assert_eq!(
                f.gate().validate_search_and_bundle(bundle, true),
                Err(NativeRefusal::DistributionUnverified)
            );
            set_dacl(path, READ_ONLY);
        }
    }

    #[test]
    fn native_post_prelaunch_gate_rechecks_for_new_file_and_directory() {
        for directory in [false, true] {
            let f = GateFixture::new();
            let bundle = &f.bundle;
            let gate = f.gate();
            assert_eq!(gate.verify_opens(bundle, true, || Ok(())), Ok(()));
            assert_eq!(
                gate.verify_opens(bundle, true, || Err(NativeRefusal::JobUnverified)),
                Err(NativeRefusal::JobUnverified)
            );
            let share = f.fixture.root.path().join("share");
            set_dacl(&share, WRITABLE);
            let extra = share.join("unlisted.dll");
            if directory {
                std::fs::create_dir(extra).unwrap();
            } else {
                std::fs::write(extra, b"fixture").unwrap();
            }
            // Restore the ACL to isolate the held inventory as the refusal.
            set_dacl(&share, READ_ONLY);
            assert_eq!(
                gate.verify_opens(bundle, true, || Ok(())),
                Err(NativeRefusal::DistributionUnverified)
            );
        }
    }

    #[test]
    fn paused_environment_is_minimal_and_nul_terminated() {
        let cwd = LocalPath::parse(r"D:\fixture\paused-cwd").unwrap();
        let block = environment(&cwd).unwrap();
        assert!(block.ends_with(&[0, 0]));
        let values = block[..block.len() - 2]
            .split(|unit| *unit == 0)
            .map(|value| String::from_utf16(value).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(values.len(), 4);
        let root = values[0].strip_prefix("SystemRoot=").unwrap();
        assert!(LocalPath::parse(root).is_ok());
        assert_eq!(values[1], format!("TEMP={}", cwd.as_str()));
        assert_eq!(values[2], format!("TMP={}", cwd.as_str()));
        assert_eq!(values[3], format!("WINDIR={root}"));
    }

    #[test]
    fn durable_slot_releases_only_a_settled_generation() {
        let trial = tempfile::tempdir().unwrap();
        let root = open_directory(trial.path().to_str().unwrap()).unwrap();
        let first = Slot::claim(&root, [1; 16]).unwrap();
        assert!(!first.state.borrow().uncertain);
        assert!(Slot::claim(&root, [2; 16]).is_err());
        assert_eq!(
            first.settle(StopDisposition::DirtyRelease),
            StopDisposition::DirtyRelease
        );
        let second = Slot::claim(&root, [2; 16]).unwrap();
        assert_eq!(
            second.settle(StopDisposition::ChargedQuarantine),
            StopDisposition::ChargedQuarantine
        );
        assert_eq!(std::fs::read(trial.path().join(SLOT)).unwrap(), [2; 16]);
        assert!(Slot::claim(&root, [3; 16]).is_err());
    }

    #[test]
    fn dropping_unsettled_slot_guard_quarantines_the_charge() {
        let trial = tempfile::tempdir().unwrap();
        let root = open_directory(trial.path().to_str().unwrap()).unwrap();
        {
            let _guard = SlotGuard::new(Slot::claim(&root, [9; 16]).unwrap());
            // Dropped without explicit settle -> must quarantine!
        }
        assert_eq!(std::fs::read(trial.path().join(SLOT)).unwrap(), [9; 16]);
        assert!(Slot::claim(&root, [10; 16]).is_err());
    }

    #[test]
    fn host_dns_servers_reads_valid_local_resolvers_without_failing() {
        let resolvers = host_dns_servers().unwrap();
        assert!(resolvers.iter().all(|ip| !ip.is_unspecified()));
    }
}
