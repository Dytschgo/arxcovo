//! Native glue: project the held, read-only preflight into the closed graph
//! and publish only an incomplete diagnostic. Every collector after the
//! baseline is unimplemented in this contract version, and the native
//! disposable-store proof type is uninhabited, so no effect can dispatch.
//!
//! Native observations are **unverified on real hardware**.

use crate::frame::*;
use crate::trial::*;
use arxd::vm::qualification::{self, Baseline, DiagnosticFile, Preflight};
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

/// The local invalidation projection. Dependencies without a reviewed
/// collector stay unmeasured, which keeps every native trial unsealable.
pub(crate) fn dependencies(p: &Preflight) -> Dependencies {
    let s = &p.store;
    Dependencies {
        release: release(),
        host: Measured::Measured(HostDependency {
            windows_build: p.host.result.windows_build,
            windows_revision: p.host.result.windows_revision,
        }),
        store: Measured::Measured(StoreDependency {
            trial_volume: s.trial_volume,
            trial_volume_root: s.trial_volume_root,
            trial_root: s.trial_root,
            backing_volume: s.backing_volume,
            backing_file: s.backing_file,
            outer_volume: s.outer_volume,
            vhd_virtual_bytes: s.vhd_virtual_bytes,
            inner_extent_bytes: s.inner_extent_bytes,
        }),
        owner_profile: Measured::Unmeasured,
        distribution: Measured::Measured(DistributionDependency {
            receipt: p.distribution.result.receipt,
            manifest: p.distribution.result.manifest,
        }),
        install_media: Measured::Unmeasured,
        recipes: Measured::Unmeasured,
        firmware: Measured::Unmeasured,
        guest_protocol: Measured::Unmeasured,
        network_topology: Measured::Unmeasured,
        policy: PolicyDependency::CURRENT,
    }
}

/// Project one baseline stage from the preflight observations. A valid
/// negative observation becomes a typed failure, never a pass.
pub(crate) fn project(p: &Preflight, stage: Stage) -> Collected {
    let timed = |started_ms, finished_ms, result| Collected {
        started_ms,
        finished_ms,
        result,
    };
    match stage {
        Stage::Inputs => {
            let o = &p.inputs;
            timed(
                o.started_ms,
                o.finished_ms,
                StageResult::Passed(Observation::InputPins {
                    held_roots: o.result.held_roots,
                    enrollment_private: o.result.enrollment_private,
                    evidence_private: o.result.evidence_private,
                    trial_root_private: o.result.trial_root_private,
                    evidence_claimed: o.result.evidence_claimed,
                }),
            )
        }
        Stage::HostBaseline => {
            let o = &p.host;
            timed(
                o.started_ms,
                o.finished_ms,
                StageResult::Passed(Observation::Host {
                    windows_build: o.result.windows_build,
                    windows_revision: o.result.windows_revision,
                }),
            )
        }
        Stage::Distribution => {
            let o = &p.distribution;
            timed(
                o.started_ms,
                o.finished_ms,
                StageResult::Passed(Observation::Distribution {
                    receipt: o.result.receipt,
                    receipt_bytes: o.result.receipt_bytes,
                    manifest: o.result.manifest,
                    manifest_bytes: o.result.manifest_bytes,
                    payload_files: o.result.payload_files,
                    payload_bytes: o.result.payload_bytes,
                }),
            )
        }
        Stage::CustodyJobCapacityBaseline => {
            let o = &p.limits;
            let l = &o.result;
            timed(
                o.started_ms,
                o.finished_ms,
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
                }),
            )
        }
        Stage::ActivityReservesBaseline => {
            let o = &p.activity;
            let a = &o.result;
            let result = if !a.console_idle {
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
            };
            timed(o.started_ms, o.finished_ms, result)
        }
        Stage::WhpxCapability => {
            let o = &p.whpx_present;
            let result = if o.result {
                StageResult::Passed(Observation::Whpx { present: true })
            } else {
                StageResult::Failed(Diagnostic::WhpxAbsent)
            };
            timed(o.started_ms, o.finished_ms, result)
        }
        // Only baseline stages reach this projection.
        _ => Collected::at(
            0,
            StageResult::Inconclusive(Diagnostic::PrerequisiteMissing),
        ),
    }
}

/// Baseline results are projected once from the held preflight, before the
/// graph runs; the custody itself stays owned by the trial. The store proof
/// type is uninhabited, so these collectors can never dispatch an effect.
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

impl<S: Source<Store = NoNativeStore>> Collectors<S> for NativeCollectors {
    fn baseline(&mut self, stage: Stage) -> Collected {
        match self.baseline.iter().position(|(s, _)| *s == stage) {
            Some(index) => self.baseline.swap_remove(index).1,
            None => Collected::at(
                self.now_ms(),
                StageResult::Inconclusive(Diagnostic::PrerequisiteMissing),
            ),
        }
    }

    fn store(&mut self) -> (Collected, Option<NoNativeStore>) {
        (
            Collected::at(self.now_ms(), StageResult::Unimplemented),
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

/// The diagnostic file for each record file. Exhaustive in both enums, and
/// the name equality is tested, so the contract cannot drift silently.
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

struct BaselineSink<'a>(&'a Baseline);

impl DiagnosticSink for BaselineSink<'_> {
    fn write(&mut self, output: Output, bytes: &[u8]) -> Result<(), RunError> {
        self.0
            .write(diagnostic_file(output), bytes)
            .map_err(|_| RunError::Output)
    }
}

pub(crate) fn run(enrollment: &str, evidence: &str) -> Result<Outcome, RunError> {
    let baseline = qualification::preflight(enrollment, evidence).map_err(RunError::Native)?;
    let clock = NativeClock {
        origin: baseline.origin(),
    };
    let dependencies = dependencies(baseline.observations());
    let issued = clock.now();
    // The authority takes the preflight custody. The diagnostic sink and the
    // observations are reached only through the trial's own reference.
    let trial_authority = TrialAuthority::issue(baseline, Class::Test, dependencies, issued)?;
    let mut trial = Trial::begin(trial_authority);
    let mut collectors = NativeCollectors::new(trial.custody().observations(), clock.origin);
    trial.drive(&mut collectors, &clock);
    publish_incomplete(&trial, &mut BaselineSink(trial.custody()))?;
    Ok(Outcome::Incomplete)
}
