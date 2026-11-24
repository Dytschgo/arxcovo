//! Project held preflight and paused-trial observations into the closed graph.
//! Native runs publish an incomplete diagnostic. Later stages remain closed.
//!
//! Native observations are **unverified on real hardware**.

use crate::frame::*;
use crate::trial::*;
#[cfg(test)]
use arxd::vm::qualification::DiagnosticFile;
use arxd::vm::qualification::{
    self, Baseline, Entry, PausedTrial, PausedVerdict, Preflight, Taken,
};
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
        false
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
        // No reviewed artifact pin exists in 7b1; 7c binds the selected one.
        artifact: Measured::Unmeasured,
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
        })),
        owner_profile: Measured::Unmeasured,
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

/// Only this collector can construct the native store proof. The OS
/// adapter separately owns the exclusive on-volume slot.
pub(crate) struct PausedStore(());

pub(crate) struct PausedCollectors<'a> {
    baseline: &'a Baseline,
    projected: NativeCollectors,
    runtime: Option<&'a tokio::runtime::Runtime>,
    result: Option<PausedTrial>,
}

impl<'a> PausedCollectors<'a> {
    pub(crate) fn new(
        baseline: &'a Baseline,
        runtime: Option<&'a tokio::runtime::Runtime>,
    ) -> Self {
        Self {
            baseline,
            projected: NativeCollectors::new(baseline.observations(), baseline.origin()),
            runtime,
            result: None,
        }
    }
}

impl Collectors<Native> for PausedCollectors<'_> {
    fn baseline(&mut self, stage: Stage) -> Collected {
        self.projected.take_stage(stage)
    }

    fn store(&mut self) -> (Collected, Option<PausedStore>) {
        let open =
            self.runtime.is_some() && observed(&self.baseline.observations().limits).is_some();
        let result = if open {
            StageResult::Passed(Observation::PausedTrial { verified: true })
        } else {
            StageResult::Failed(Diagnostic::DistributionUnverified)
        };
        (
            Collected::at(self.projected.now_ms(), result),
            open.then_some(PausedStore(())),
        )
    }

    fn implemented(&self, stage: Stage) -> bool {
        matches!(
            stage,
            Stage::SuspendedChild | Stage::QemuPrelaunchPause | Stage::HeldBundleInventory
        )
    }

    fn plan(&mut self, _: Stage) -> EffectPlan {
        EffectPlan::CONFINED
    }

    fn live(&mut self) -> Option<u64> {
        self.baseline.paused_live().ok().map(|captured| {
            u64::try_from(captured.duration_since(self.baseline.origin()).as_millis())
                .unwrap_or(u64::MAX)
        })
    }

    fn effect(&mut self, stage: Stage, _: EffectPlan, _: &PausedStore) -> Collected {
        let started_ms = self.projected.now_ms();
        if stage == Stage::SuspendedChild
            && let Some(runtime) = self.runtime
        {
            // Startup, paused QMP, inventory recheck and stop are one held
            // operation. Subsequent rows only project its verified result.
            self.result = Some(runtime.block_on(async {
                let cancel = Shutdown::default();
                let signal = cancel.clone();
                let interrupt = tokio::spawn(async move {
                    if tokio::signal::ctrl_c().await.is_ok() {
                        signal.cancel();
                    }
                });
                let result = self.baseline.paused_trial(&cancel).await;
                interrupt.abort();
                result
            }));
        }
        let result = match self.result.as_ref().map(|trial| &trial.verdict) {
            Some(PausedVerdict::Verified) => {
                StageResult::Passed(Observation::PausedTrial { verified: true })
            }
            Some(PausedVerdict::Refused(reason)) => StageResult::Failed(refused(*reason)),
            Some(PausedVerdict::OwnerActive) => StageResult::Failed(Diagnostic::OwnerActive),
            Some(PausedVerdict::Qmp) => StageResult::Failed(Diagnostic::OtherRefusal),
            Some(PausedVerdict::Timeout) => StageResult::Failed(Diagnostic::Expired),
            Some(PausedVerdict::Cancelled) => StageResult::Failed(Diagnostic::Cancelled),
            None => StageResult::Inconclusive(Diagnostic::PrerequisiteMissing),
        };
        Collected {
            started_ms,
            finished_ms: self.projected.now_ms(),
            result,
        }
    }

    fn cleanup(&mut self, _: u16) -> Teardown {
        if self
            .result
            .as_ref()
            .is_some_and(|run| run.release == StopDisposition::ChargedQuarantine)
        {
            Teardown::Uncertain
        } else {
            Teardown::Released
        }
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

pub(crate) fn run(enrollment: &str, evidence: &str, paused: bool) -> Result<Outcome, RunError> {
    let runtime = paused
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
    let preflight = if paused {
        qualification::preflight_paused
    } else {
        qualification::preflight
    };
    let baseline = preflight(enrollment, evidence).map_err(RunError::Native)?;
    let clock = NativeClock {
        origin: baseline.origin(),
    };
    let dependencies = dependencies(baseline.observations());
    let issued = clock.now();
    // The authority takes the preflight custody. The diagnostic sink and the
    // observations are reached only through the trial's own reference.
    let trial_authority = TrialAuthority::issue(baseline, Class::Test, dependencies, issued);
    let mut trial = Trial::begin(trial_authority);
    trial.drive_native(runtime.as_ref(), &clock);
    publish_incomplete(&trial, &mut BaselineSink(trial.custody()))?;
    Ok(Outcome::Incomplete)
}
