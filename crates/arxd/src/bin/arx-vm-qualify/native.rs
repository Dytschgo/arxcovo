//! Project held preflight and paused-trial observations into the closed graph.
//! Native runs publish an incomplete diagnostic. Later stages remain closed.
//!
//! Native observations are **unverified on real hardware**.

use crate::Mode;
use crate::frame::*;
use crate::trial::*;
use arxd::vm::qualification::{
    self, Baseline, Entry, FirmwareProbe, PausedPhase, PausedTrial, PausedTrialPermit,
    PausedVerdict, Preflight, Taken,
};
#[cfg(test)]
use arxd::vm::qualification::{DiagnosticFile, PausedCompleted};
use arxd::{
    service::Shutdown,
    vm::runtime::{NativeRefusal, StopDisposition},
};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum Outcome {
    /// Diagnostics were written; the dossier cannot complete.
    Incomplete,
}

struct NativeClock {
    origin: Instant,
    cancel: Shutdown,
}

fn ms_since(origin: Instant) -> u64 {
    u64::try_from(origin.elapsed().as_millis()).unwrap_or(u64::MAX)
}

impl Clock for NativeClock {
    fn now(&self) -> Instant2 {
        Instant2 {
            // A pre-epoch clock reads as zero, which no binding accepts.
            utc: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |time| time.as_secs()),
            mono_ms: ms_since(self.origin),
        }
    }

    fn cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }
}

fn observed<T>(taken: &Taken<T>) -> Option<&T> {
    match taken {
        Taken::Observed(observation) => Some(&observation.result),
        Taken::Refused { .. } => None,
    }
}

fn measured<T>(value: Option<T>) -> Measured<T> {
    value.map_or(Measured::Unmeasured, Measured::Measured)
}

/// The local invalidation projection. A dependency whose observation was
/// refused, or that no reviewed collector measures yet, stays unmeasured,
/// which keeps every native trial unsealable.
pub(crate) fn dependencies(p: &Preflight) -> Dependencies {
    let s = &p.store;
    Dependencies {
        release: release(),
        artifact: measured(p.reviewed_release),
        host: measured(observed(&p.host).map(|host| HostDependency {
            windows_build: host.windows_build,
            windows_revision: host.windows_revision,
            machine: host.machine,
        })),
        store: measured(observed(&p.limits).map(|limits| StoreDependency {
            trial_volume: s.trial_volume,
            trial_volume_root: s.trial_volume_root,
            trial_volume_guid: s.trial_volume_guid,
            trial_root: s.trial_root,
            outer_volume: s.outer_volume,
            outer_volume_root: s.outer_volume_root,
            outer_volume_guid: s.outer_volume_guid,
            backing_file: s.backing_file,
            vhd_virtual_bytes: limits.vhd_virtual_bytes,
            inner_extent_bytes: limits.inner_extent_bytes,
            test_scope: p.test_scope,
        })),
        owner_profile: measured(p.owner_profile),
        distribution: measured(observed(&p.distribution).map(|distribution| {
            DistributionDependency {
                receipt: distribution.receipt,
                manifest: distribution.manifest,
            }
        })),
        install_media: Measured::Unmeasured,
        recipes: Measured::Unmeasured,
        firmware: Measured::Unmeasured,
        guest_protocol: Measured::Unmeasured,
        network_topology: Measured::Unmeasured,
        policy: PolicyDependency::CURRENT,
    }
}

/// The typed diagnostic for a refused post-claim observation. Exhaustive,
/// so a new refusal cannot be settled silently.
pub(crate) fn refused(reason: NativeRefusal) -> Diagnostic {
    use NativeRefusal as R;
    match reason {
        R::UnsupportedPlatform => Diagnostic::UnsupportedPlatform,
        R::DistributionUnverified => Diagnostic::DistributionUnverified,
        R::PathCustodyUnverified => Diagnostic::PathCustodyUnverified,
        R::JobUnverified => Diagnostic::JobUnverified,
        R::WhpxUnverified => Diagnostic::WhpxUnverified,
        R::DiskCeilingUnverified => Diagnostic::DiskCeilingUnverified,
        R::OwnerActivityUnverified => Diagnostic::OwnerActivityUnverified,
        R::ReservesUnverified => Diagnostic::ReservesUnverified,
        R::Disabled
        | R::QualificationMissing
        | R::IdentityUnverified
        | R::QmpUnverified
        | R::GuestEndpointUnverified
        | R::NetworkFenceUnverified
        | R::InstallerPhaseUnavailable
        | R::InstallerPhaseMismatch
        | R::InstallerDeadlineExpired => Diagnostic::OtherRefusal,
    }
}

/// Project one post-claim observation: a refusal becomes a typed failure at
/// its own time, and an observation goes through `pass`.
fn settle<T>(taken: &Taken<T>, pass: impl FnOnce(&T) -> StageResult) -> Collected {
    match taken {
        Taken::Observed(o) => Collected {
            started_ms: o.started_ms,
            finished_ms: o.finished_ms,
            result: pass(&o.result),
        },
        Taken::Refused { at_ms, reason } => {
            Collected::at(*at_ms, StageResult::Failed(refused(*reason)))
        }
    }
}

/// Project one baseline stage from the preflight observations. A valid
/// negative observation or a refusal becomes a typed failure, never a pass.
pub(crate) fn project(p: &Preflight, stage: Stage) -> Collected {
    match stage {
        Stage::Inputs => {
            let o = &p.inputs;
            Collected {
                started_ms: o.started_ms,
                finished_ms: o.finished_ms,
                result: StageResult::Passed(Observation::InputPins {
                    held_roots: o.result.held_roots,
                    enrollment_private: o.result.enrollment_private,
                    evidence_private: o.result.evidence_private,
                    trial_root_private: o.result.trial_root_private,
                    evidence_claimed: o.result.evidence_claimed,
                }),
            }
        }
        // The machine identity is a local dependency and is never published.
        Stage::HostBaseline => settle(&p.host, |h| {
            StageResult::Passed(Observation::Host {
                windows_build: h.windows_build,
                windows_revision: h.windows_revision,
            })
        }),
        Stage::Distribution => settle(&p.distribution, |d| {
            StageResult::Passed(Observation::Distribution {
                receipt: d.receipt,
                receipt_bytes: d.receipt_bytes,
                manifest: d.manifest,
                manifest_bytes: d.manifest_bytes,
                payload_files: d.payload_files,
                payload_bytes: d.payload_bytes,
            })
        }),
        Stage::CustodyJobCapacityBaseline => settle(&p.limits, |l| {
            StageResult::Passed(Observation::Capacity {
                inner_caller_free_bytes: l.inner_caller_free_bytes,
                inner_total_free_bytes: l.inner_total_free_bytes,
                inner_total_bytes: l.inner_total_bytes,
                inner_extent_bytes: l.inner_extent_bytes,
                vhd_virtual_bytes: l.vhd_virtual_bytes,
                outer_caller_free_bytes: l.outer_caller_free_bytes,
                outer_total_free_bytes: l.outer_total_free_bytes,
                backing_allocated_bytes: l.backing_allocated_bytes,
                backing_length_bytes: l.backing_length_bytes,
                disk_ceiling_bytes: l.disk_ceiling_bytes,
                outer_floor_bytes: l.outer_floor_bytes,
                disk_budget_bytes: l.disk_budget_bytes,
                empty_job_memory_limit_bytes: l.empty_job_memory_limit_bytes,
                empty_job_cpu_rate: l.empty_job_cpu_rate,
            })
        }),
        Stage::ActivityReservesBaseline => settle(&p.activity, |a| {
            if !a.console_idle {
                StageResult::Failed(Diagnostic::OwnerActive)
            } else if a.active_cpus <= 1 || a.available_ram_bytes <= a.owner_ram_reserve_bytes {
                StageResult::Failed(Diagnostic::ReservesShort)
            } else {
                StageResult::Passed(Observation::Activity {
                    console_idle: a.console_idle,
                    active_cpus: a.active_cpus,
                    available_ram_bytes: a.available_ram_bytes,
                    owner_ram_reserve_bytes: a.owner_ram_reserve_bytes,
                    owner_cpu_reserve: a.owner_cpu_reserve,
                })
            }
        }),
        Stage::WhpxCapability => settle(&p.whpx_present, |present| {
            if *present {
                StageResult::Passed(Observation::Whpx { present: true })
            } else {
                StageResult::Failed(Diagnostic::WhpxAbsent)
            }
        }),
        // Only baseline stages reach this projection.
        _ => Collected::at(
            0,
            StageResult::Inconclusive(Diagnostic::PrerequisiteMissing),
        ),
    }
}

/// Baseline results are projected once from the held preflight, before the
/// graph runs. The trial retains the custody.
pub(crate) struct NativeCollectors {
    baseline: Vec<(Stage, Collected)>,
    origin: Instant,
    /// Later records never precede the projected baseline.
    floor_ms: u64,
}

impl NativeCollectors {
    pub(crate) fn new(preflight: &Preflight, origin: Instant) -> Self {
        let baseline: Vec<(Stage, Collected)> = stages(Class::Test)
            .iter()
            .filter(|stage| stage.kind() == StageKind::Baseline)
            .map(|&stage| (stage, project(preflight, stage)))
            .collect();
        let floor_ms = baseline
            .iter()
            .map(|(_, collected)| collected.finished_ms)
            .max()
            .unwrap_or(0);
        Self {
            baseline,
            origin,
            floor_ms,
        }
    }

    fn now_ms(&self) -> u64 {
        ms_since(self.origin).max(self.floor_ms)
    }
}

impl NativeCollectors {
    fn take_stage(&mut self, stage: Stage) -> Collected {
        match self.baseline.iter().position(|(s, _)| *s == stage) {
            Some(index) => self.baseline.swap_remove(index).1,
            None => Collected::at(
                self.now_ms(),
                StageResult::Inconclusive(Diagnostic::PrerequisiteMissing),
            ),
        }
    }
}

#[cfg(test)]
impl<S: Source<Store = NoNativeStore>> Collectors<S> for NativeCollectors {
    fn baseline(&mut self, stage: Stage) -> Collected {
        self.take_stage(stage)
    }

    fn store(&mut self) -> (Collected, Option<NoNativeStore>) {
        (
            Collected::at(
                self.now_ms(),
                StageResult::Failed(Diagnostic::DistributionUnverified),
            ),
            None,
        )
    }

    fn plan(&mut self, _stage: Stage) -> EffectPlan {
        // No native plan exists; report the least confined shape.
        EffectPlan {
            paused: false,
            disks: u8::MAX,
            network: true,
        }
    }

    fn live(&mut self) -> Option<u64> {
        None
    }

    fn effect(&mut self, _stage: Stage, _plan: EffectPlan, store: &NoNativeStore) -> Collected {
        match *store {}
    }

    fn cleanup(&mut self, dispatched: u16) -> Teardown {
        // Nothing can be dispatched natively in this version; anything
        // else is uncertain and keeps its charge.
        if dispatched == 0 {
            Teardown::Released
        } else {
            Teardown::Uncertain
        }
    }
}

/// Records one verified release disposition per invocation and latches
/// `ChargedQuarantine` for the entire run so no later stage can clear it.
#[derive(Default)]
pub(crate) struct ReleaseLedger {
    invocations: Vec<(Stage, StopDisposition)>,
    quarantined: bool,
}

impl ReleaseLedger {
    pub(crate) fn record(&mut self, stage: Stage, disposition: StopDisposition) {
        self.invocations.push((stage, disposition));
        if disposition == StopDisposition::ChargedQuarantine {
            self.quarantined = true;
        }
    }

    pub(crate) fn is_quarantined(&self) -> bool {
        self.quarantined
    }

    pub(crate) fn teardown(&self) -> Teardown {
        if self.quarantined {
            Teardown::Uncertain
        } else {
            Teardown::Released
        }
    }

    #[cfg(test)]
    pub(crate) fn count(&self) -> usize {
        self.invocations.len()
    }
}

/// Only this collector can construct the native store proof. The OS
/// adapter separately owns the exclusive on-volume slot.
pub(crate) struct PausedStore(());

pub(crate) struct PausedCollectors<'a> {
    baseline: &'a Baseline,
    projected: NativeCollectors,
    runtime: Option<&'a tokio::runtime::Runtime>,
    cancel: Shutdown,
    permit: Option<PausedTrialPermit<'a>>,
    result: Option<PausedTrial>,
    full_test: bool,
    releases: ReleaseLedger,
    firmware_qmp: Option<FirmwareProbe>,
}

impl<'a> PausedCollectors<'a> {
    pub(crate) fn new(
        baseline: &'a Baseline,
        runtime: Option<&'a tokio::runtime::Runtime>,
        full_test: bool,
        cancel: Shutdown,
    ) -> Self {
        Self {
            baseline,
            projected: NativeCollectors::new(baseline.observations(), baseline.origin()),
            runtime,
            cancel,
            permit: None,
            result: None,
            full_test,
            releases: ReleaseLedger::default(),
            firmware_qmp: None,
        }
    }
}

impl Collectors<Native> for PausedCollectors<'_> {
    fn baseline(&mut self, stage: Stage) -> Collected {
        self.projected.take_stage(stage)
    }

    fn store(&mut self) -> (Collected, Option<PausedStore>) {
        let prepared = if self.runtime.is_some() {
            self.baseline.prepare_paused_trial()
        } else {
            Err(NativeRefusal::DistributionUnverified)
        };
        let (result, proof) = match prepared {
            Ok(permit) => {
                self.permit = Some(permit);
                (
                    StageResult::Passed(Observation::PausedStore { validated: true }),
                    Some(PausedStore(())),
                )
            }
            Err(reason) => (StageResult::Failed(refused(reason)), None),
        };
        (Collected::at(self.projected.now_ms(), result), proof)
    }

    fn implemented(&self, stage: Stage) -> bool {
        paused_phase(stage).is_some()
            || (self.full_test
                && matches!(
                    stage,
                    Stage::FirstGuestExecution
                        | Stage::SupervisorLossRelease
                        | Stage::CapacityTrials
                        | Stage::ReserveActivityTrials
                        | Stage::QmpChannel
                        | Stage::RestrictedTestNetwork
                ))
    }

    fn previous_effect(&mut self, stage: Stage) -> Option<Collected> {
        if self.full_test && stage == Stage::QmpChannel {
            let result = match &self.firmware_qmp {
                Some(probe)
                    if probe.accepted
                        && probe.greeting_verified
                        && probe.whpx_enabled
                        && probe.quit_clean_eof
                        && probe.release == StopDisposition::CleanRelease =>
                {
                    StageResult::Passed(Observation::QmpFirmware {
                        greeting_verified: probe.greeting_verified,
                        whpx_enabled: probe.whpx_enabled,
                        quit_clean_eof: probe.quit_clean_eof,
                        clean_release: probe.release == StopDisposition::CleanRelease,
                    })
                }
                _ => StageResult::Failed(Diagnostic::PrerequisiteMissing),
            };
            return Some(Collected::at(self.projected.now_ms(), result));
        }
        matches!(
            stage,
            Stage::QemuPrelaunchPause | Stage::HeldBundleInventory
        )
        .then(|| {
            Collected::at(
                self.projected.now_ms(),
                project_paused(stage, self.result.as_ref()),
            )
        })
    }

    fn plan(&mut self, stage: Stage) -> EffectPlan {
        EffectPlan {
            paused: !matches!(
                stage,
                Stage::FirstGuestExecution
                    | Stage::SupervisorLossRelease
                    | Stage::RestrictedTestNetwork
            ),
            network: stage == Stage::RestrictedTestNetwork,
            ..EffectPlan::CONFINED
        }
    }

    fn live(&mut self) -> Option<u64> {
        self.baseline.paused_live().ok().map(|captured| {
            u64::try_from(captured.duration_since(self.baseline.origin()).as_millis())
                .unwrap_or(u64::MAX)
        })
    }

    fn effect(&mut self, stage: Stage, _: EffectPlan, _: &PausedStore) -> Collected {
        let started_ms = self.projected.now_ms();
        if self.releases.is_quarantined() {
            return Collected::at(
                started_ms,
                StageResult::Failed(Diagnostic::TeardownUncertain),
            );
        }
        if self.full_test && stage == Stage::ReserveActivityTrials {
            let result = match (self.runtime, self.baseline.prepare_reserve_activity()) {
                (Some(runtime), Ok(permit)) => {
                    let observed = runtime.block_on(permit.run(&self.cancel));
                    for disp in &observed.invocations {
                        self.releases.record(stage, *disp);
                    }
                    if observed.invocations.is_empty() {
                        self.releases.record(stage, observed.release);
                    }
                    match observed.observations {
                        Some((controlled_faults, console_transitions_ms))
                            if !self.releases.is_quarantined() =>
                        {
                            StageResult::Passed(Observation::ReserveActivity {
                                controlled_faults: Box::new(controlled_faults),
                                console_transitions_ms,
                            })
                        }
                        _ if self.releases.is_quarantined() => {
                            StageResult::Failed(Diagnostic::TeardownUncertain)
                        }
                        None => StageResult::Inconclusive(Diagnostic::OwnerActivityUnverified),
                        Some(_) => StageResult::Failed(Diagnostic::TeardownUncertain),
                    }
                }
                (_, Err(reason)) => StageResult::Failed(refused(reason)),
                _ => StageResult::Inconclusive(Diagnostic::PrerequisiteMissing),
            };
            return Collected {
                started_ms,
                finished_ms: self.projected.now_ms(),
                result,
            };
        }
        if self.full_test && stage == Stage::FirstGuestExecution {
            let result = match (self.runtime, self.baseline.prepare_firmware_probe()) {
                (Some(runtime), Ok(permit)) => {
                    let probe = runtime.block_on(permit.run(&self.cancel));
                    self.releases.record(stage, probe.release);
                    if probe.accepted {
                        let obs = Observation::FirmwareSec {
                            sec_marker_verified: probe.sec_marker_verified,
                            prelaunch_quiet: probe.prelaunch_quiet,
                            job_memory_limit_bytes: probe.job_memory_limit_bytes,
                            job_cpu_rate: probe.job_cpu_rate,
                            clean_release: probe.release == StopDisposition::CleanRelease,
                        };
                        self.firmware_qmp = Some(probe);
                        StageResult::Passed(obs)
                    } else {
                        StageResult::Failed(
                            probe.refusal.map_or(Diagnostic::TeardownUncertain, refused),
                        )
                    }
                }
                (_, Err(reason)) => StageResult::Failed(refused(reason)),
                _ => StageResult::Inconclusive(Diagnostic::PrerequisiteMissing),
            };
            return Collected {
                started_ms,
                finished_ms: self.projected.now_ms(),
                result,
            };
        }
        if self.full_test && stage == Stage::SupervisorLossRelease {
            let result = match (self.runtime, self.baseline.prepare_supervisor_loss()) {
                (Some(runtime), Ok(permit)) => {
                    let probe = runtime.block_on(permit.run(&self.cancel));
                    self.releases.record(stage, probe.release);
                    if probe.accepted {
                        StageResult::Passed(Observation::SupervisorLoss {
                            job_memory_limit_bytes: probe.job_memory_limit_bytes,
                            job_cpu_rate: probe.job_cpu_rate,
                            active_process_limit: probe.active_process_limit,
                            child_exited: probe.child_exited,
                            job_empty_notified: probe.job_empty_notified,
                            dirty_release: probe.release == StopDisposition::DirtyRelease,
                        })
                    } else if probe.inconclusive {
                        StageResult::Inconclusive(Diagnostic::TeardownUncertain)
                    } else {
                        StageResult::Failed(
                            probe.refusal.map_or(Diagnostic::TeardownUncertain, refused),
                        )
                    }
                }
                (_, Err(reason)) => StageResult::Failed(refused(reason)),
                _ => StageResult::Inconclusive(Diagnostic::PrerequisiteMissing),
            };
            return Collected {
                started_ms,
                finished_ms: self.projected.now_ms(),
                result,
            };
        }
        if self.full_test && stage == Stage::CapacityTrials {
            let result = match (self.runtime, self.baseline.prepare_capacity_probe()) {
                (Some(runtime), Ok(permit)) => {
                    let probe = runtime.block_on(permit.run(&self.cancel));
                    self.releases.record(stage, probe.release);
                    if probe.accepted {
                        StageResult::Passed(Observation::CapacityExercised {
                            trial_allocated_bytes: probe.trial_allocated_bytes,
                            recipe_allocated_bytes: probe.recipe_allocated_bytes,
                            denial_checks: probe.denial_checks,
                            overlap_excluded: probe.overlap_excluded,
                            aggregate_cap_checked: probe.aggregate_cap_checked,
                            trial_released: probe.trial_released,
                            recipe_released: probe.recipe_released,
                        })
                    } else {
                        StageResult::Failed(
                            probe.refusal.map_or(Diagnostic::TeardownUncertain, refused),
                        )
                    }
                }
                (_, Err(reason)) => StageResult::Failed(refused(reason)),
                _ => StageResult::Inconclusive(Diagnostic::PrerequisiteMissing),
            };
            return Collected {
                started_ms,
                finished_ms: self.projected.now_ms(),
                result,
            };
        }
        if self.full_test && stage == Stage::RestrictedTestNetwork {
            let result = match (self.runtime, self.baseline.prepare_network_probe()) {
                (Some(runtime), Ok(permit)) => {
                    let probe = runtime.block_on(permit.run(&self.cancel));
                    self.releases.record(stage, probe.release);
                    // Guest socket outcomes do not attribute a denial to the
                    // slirp filter, even when both host controls succeeded.
                    match probe.refusal {
                        Some(reason) => StageResult::Failed(refused(reason)),
                        None => StageResult::Inconclusive(Diagnostic::PrerequisiteMissing),
                    }
                }
                (_, Err(reason)) => StageResult::Failed(refused(reason)),
                _ => StageResult::Inconclusive(Diagnostic::PrerequisiteMissing),
            };
            return Collected {
                started_ms,
                finished_ms: self.projected.now_ms(),
                result,
            };
        }
        if stage != Stage::SuspendedChild {
            return Collected::at(started_ms, StageResult::Unimplemented);
        }
        if let (Some(runtime), Some(permit)) = (self.runtime, self.permit.take()) {
            // Startup, paused QMP, inventory recheck and stop are one held
            // operation. Subsequent rows only project its verified result.
            let trial = runtime.block_on(permit.run(&self.cancel));
            self.releases.record(stage, trial.release);
            self.result = Some(trial);
        }
        let result = project_paused(stage, self.result.as_ref());
        Collected {
            started_ms,
            finished_ms: self.projected.now_ms(),
            result,
        }
    }

    fn cleanup(&mut self, _: u16) -> Teardown {
        match self.releases.teardown() {
            Teardown::Uncertain => Teardown::Uncertain,
            Teardown::Released => paused_teardown(self.result.as_ref()),
        }
    }
}

fn paused_phase(stage: Stage) -> Option<PausedPhase> {
    match stage {
        Stage::SuspendedChild => Some(PausedPhase::SuspendedChild),
        Stage::QemuPrelaunchPause => Some(PausedPhase::Prelaunch),
        Stage::HeldBundleInventory => Some(PausedPhase::BundleInventory),
        _ => None,
    }
}

fn project_paused(stage: Stage, trial: Option<&PausedTrial>) -> StageResult {
    let Some(phase) = paused_phase(stage) else {
        return StageResult::Unimplemented;
    };
    let Some(trial) = trial else {
        return StageResult::Inconclusive(Diagnostic::PrerequisiteMissing);
    };
    let pass = StageResult::Passed(Observation::PausedTrial { verified: true });
    let failed = |verdict: &PausedVerdict| match verdict {
        PausedVerdict::Verified => pass.clone(),
        PausedVerdict::Refused(reason) => StageResult::Failed(refused(*reason)),
        PausedVerdict::OwnerActive => StageResult::Failed(Diagnostic::OwnerActive),
        PausedVerdict::Qmp => StageResult::Failed(Diagnostic::OtherRefusal),
        PausedVerdict::Timeout => StageResult::Failed(Diagnostic::Expired),
        PausedVerdict::Cancelled => StageResult::Failed(Diagnostic::Cancelled),
    };
    let done = trial.completed;
    match phase {
        PausedPhase::SuspendedChild => {
            if done.suspended_child {
                pass
            } else {
                failed(&trial.verdict)
            }
        }
        PausedPhase::Prelaunch => {
            if done.prelaunch {
                pass
            } else if done.suspended_child {
                failed(&trial.verdict)
            } else {
                StageResult::Inconclusive(Diagnostic::PrerequisiteMissing)
            }
        }
        PausedPhase::BundleInventory => {
            if done.bundle_inventory && matches!(trial.verdict, PausedVerdict::Verified) {
                pass
            } else if done.prelaunch {
                failed(&trial.verdict)
            } else {
                StageResult::Inconclusive(Diagnostic::PrerequisiteMissing)
            }
        }
    }
}

fn paused_teardown(trial: Option<&PausedTrial>) -> Teardown {
    if trial.is_some_and(|trial| trial.release == StopDisposition::ChargedQuarantine) {
        Teardown::Uncertain
    } else {
        Teardown::Released
    }
}

/// The diagnostic file for each record file. Exhaustive in both enums, and
/// the name equality is tested, so the contract cannot drift silently.
#[cfg(test)]
pub(crate) fn diagnostic_file(output: Output) -> DiagnosticFile {
    match output {
        Output::Incomplete => DiagnosticFile::Incomplete,
        Output::Records(kind) => match kind {
            FileKind::Inputs => DiagnosticFile::Inputs,
            FileKind::HostBaseline => DiagnosticFile::HostBaseline,
            FileKind::Distribution => DiagnosticFile::Distribution,
            FileKind::CustodyJobCapacity => DiagnosticFile::CustodyJobCapacity,
            FileKind::ActivityReserves => DiagnosticFile::ActivityReserves,
            FileKind::WhpxQmpEndpoint => DiagnosticFile::WhpxQmpEndpoint,
            FileKind::NetworkRestrictedTest => DiagnosticFile::NetworkRestrictedTest,
            FileKind::RecipeUbuntu => DiagnosticFile::RecipeUbuntu,
            FileKind::RecipeWindows => DiagnosticFile::RecipeWindows,
            FileKind::NetworkRestrictedOrigin => DiagnosticFile::NetworkRestrictedOrigin,
            FileKind::RecipeStaticWeb => DiagnosticFile::RecipeStaticWeb,
            FileKind::NetworkPublicIpv4 => DiagnosticFile::NetworkPublicIpv4,
            FileKind::RecipeDocker => DiagnosticFile::RecipeDocker,
            FileKind::Cleanup => DiagnosticFile::Cleanup,
        },
    }
}

/// The summary fields a caller may supply. The writer adds the contract,
/// origin, binding and the list of files it could not commit.
#[derive(serde::Serialize)]
struct SummaryBody<'a> {
    blocked_at: Option<Stage>,
    unattempted: &'a [Stage],
    unmeasured: &'a [String],
    dispatched_effects: u16,
    charge_retained: bool,
    late: Option<Diagnostic>,
}

struct BaselineSink<'a>(&'a Baseline);

impl DiagnosticSink for BaselineSink<'_> {
    /// Only stages, times, results and summary fields cross the boundary.
    /// The writer draws the scope and run, derives the release, the keyed
    /// dependency digest and the record chain, and commits the summary last.
    fn publish(
        &mut self,
        records: &[Record],
        dependencies: &Dependencies,
        summary: &Incomplete,
    ) -> Result<(), RunError> {
        let entries: Vec<Entry<'_, Stage, StageResult>> = records
            .iter()
            .map(|record| Entry {
                stage: &record.stage,
                started_ms: record.started_ms,
                finished_ms: record.finished_ms,
                result: &record.result,
            })
            .collect();
        let body = SummaryBody {
            blocked_at: summary.blocked_at,
            unattempted: &summary.unattempted,
            unmeasured: &summary.unmeasured,
            dispatched_effects: summary.dispatched_effects,
            charge_retained: summary.charge_retained,
            late: summary.late,
        };
        let unwritten = self
            .0
            .publish(
                &summary.binding.class,
                dependencies,
                summary.binding.issued_utc,
                &entries,
                &body,
            )
            .map_err(|_| RunError::Output)?;
        if unwritten.is_empty() {
            Ok(())
        } else {
            Err(RunError::Output)
        }
    }
}

pub(crate) fn run(enrollment: &str, evidence: &str, mode: Mode) -> Result<Outcome, RunError> {
    let runtime = (mode != Mode::Baseline)
        .then(|| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
        })
        .transpose()
        .map_err(|_| RunError::Native(NativeRefusal::JobUnverified))?;
    // Everything fallible happens before the preflight claims the output,
    // including the writer's own random scope and run. After the claim,
    // every failure settles as a typed diagnostic.
    let preflight = match mode {
        Mode::Baseline => qualification::preflight,
        Mode::Paused => qualification::preflight_paused,
        Mode::Test => qualification::preflight_full_test,
    };
    let baseline = preflight(enrollment, evidence).map_err(RunError::Native)?;
    let cancel = Shutdown::default();
    let interrupt = runtime.as_ref().map(|rt| {
        let signal = cancel.clone();
        rt.spawn(async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                signal.cancel();
            }
        })
    });
    let clock = NativeClock {
        origin: baseline.origin(),
        cancel: cancel.clone(),
    };
    let dependencies = dependencies(baseline.observations());
    let issued = clock.now();
    // The authority takes the preflight custody. The diagnostic sink and the
    // observations are reached only through the trial's own reference.
    let trial_authority = TrialAuthority::issue(baseline, Class::Test, dependencies, issued);
    let mut trial = Trial::begin(trial_authority);
    trial.drive_native(runtime.as_ref(), mode == Mode::Test, &cancel, &clock);
    if let Some(handle) = interrupt {
        handle.abort();
    }
    publish_incomplete(&trial, &mut BaselineSink(trial.custody()))?;
    Ok(Outcome::Incomplete)
}

#[cfg(test)]
mod paused_tests {
    use super::*;

    #[test]
    fn projections_are_limited_to_completed_paused_phases() {
        let mut trial = PausedTrial {
            phase: PausedPhase::BundleInventory,
            completed: PausedCompleted {
                suspended_child: true,
                prelaunch: true,
                bundle_inventory: false,
            },
            verdict: PausedVerdict::Refused(NativeRefusal::DistributionUnverified),
            release: StopDisposition::ChargedQuarantine,
        };
        let pass = StageResult::Passed(Observation::PausedTrial { verified: true });
        assert_eq!(project_paused(Stage::SuspendedChild, Some(&trial)), pass);
        assert_eq!(
            project_paused(Stage::QemuPrelaunchPause, Some(&trial)),
            pass
        );
        assert_eq!(
            project_paused(Stage::HeldBundleInventory, Some(&trial)),
            StageResult::Failed(Diagnostic::DistributionUnverified)
        );
        assert!(matches!(paused_teardown(Some(&trial)), Teardown::Uncertain));
        trial.completed.bundle_inventory = true;
        trial.verdict = PausedVerdict::Verified;
        trial.release = StopDisposition::DirtyRelease;
        assert_eq!(
            project_paused(Stage::HeldBundleInventory, Some(&trial)),
            pass
        );
        assert_eq!(
            project_paused(Stage::FirstGuestExecution, Some(&trial)),
            StageResult::Unimplemented
        );
        assert!(matches!(paused_teardown(Some(&trial)), Teardown::Released));
        assert!(matches!(paused_teardown(None), Teardown::Released));
    }

    #[test]
    fn release_ledger_records_each_invocation_and_latches_quarantine() {
        let mut ledger = ReleaseLedger::default();
        ledger.record(Stage::SuspendedChild, StopDisposition::DirtyRelease);
        ledger.record(Stage::FirstGuestExecution, StopDisposition::CleanRelease);
        assert!(!ledger.is_quarantined());
        assert_eq!(ledger.teardown(), Teardown::Released);
        ledger.record(
            Stage::SupervisorLossRelease,
            StopDisposition::ChargedQuarantine,
        );
        // Subsequent CleanRelease must NEVER clear latched quarantine.
        ledger.record(Stage::CapacityTrials, StopDisposition::CleanRelease);
        assert!(ledger.is_quarantined());
        assert_eq!(ledger.teardown(), Teardown::Uncertain);
        assert_eq!(ledger.count(), 4);
    }
}
