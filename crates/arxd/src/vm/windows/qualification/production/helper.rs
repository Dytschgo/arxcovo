//! The shared checked helper adapter (ADR 0054, 7c chunk 11). Clone staging
//! is its first consumer; build reuses it in 7c 12.
//!
//! The empty target is precreated and identified under the held workspace
//! before any helper exists, and the helper cutoff is fixed then: the
//! earliest of the worker deadline, the effect cutoff and the helper budget.
//! Helper output and helper release are separate outcomes. Output is used
//! only after a matched clean release of exactly the expected inputs and a
//! final cancellation and cutoff check, so the #344 post-sample refusal
//! still holds. A refused output keeps its release proof for settlement; an
//! unproven release keeps the target charged. Native helper execution is
//! **unverified on real hardware**.
#![cfg_attr(
    not(test),
    allow(dead_code, reason = "the closed TEST installation lands in 7c 16")
)]

use super::*;
use crate::vm::runtime::OfflineLease;

type Q = NativeRefusal;
const PATH: Q = NativeRefusal::PathCustodyUnverified;
#[cfg(test)]
thread_local! {
    /// Runs once inside `accept`, after the final re-pin observation.
    pub(super) static AFTER_REPIN: std::cell::RefCell<Option<Box<dyn FnOnce()>>> =
        const { std::cell::RefCell::new(None) };
}

/// The individual helper bound (spec §1 phase budgets).
pub(crate) const HELPER_BUDGET: Duration = Duration::from_secs(30);

/// The earliest cutoff a checked helper may run to.
pub(crate) fn helper_deadline(worker: Instant, effect: Instant, now: Instant) -> Instant {
    now.checked_add(HELPER_BUDGET)
        .unwrap_or(now)
        .min(worker)
        .min(effect)
}

/// An identified, empty helper target under the held workspace, its offline
/// binding and its fixed cutoff.
pub(crate) struct PreparedHelper<'w, 'l> {
    workspace: &'w ProductionWorkspace,
    target: HelperTarget<'l>,
    lease: OfflineLease,
    deadline: Instant,
}

impl ProductionWorkspace {
    /// Fixes the cutoff, then precreates and identifies the empty target.
    /// No helper can be dispatched before the identity is durable.
    pub(crate) fn prepare_helper<'w, 'l>(
        &'w self,
        slot: ArtifactSlot,
        ledger: &'l dyn CustodyCheckpoints,
        worker_deadline: Instant,
        effect_cutoff: Instant,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<PreparedHelper<'w, 'l>, Q> {
        let deadline = helper_deadline(worker_deadline, effect_cutoff, Instant::now());
        launch::check_preparation(deadline, cancelled)?;
        let target = self.precreate_helper_target(slot, ledger)?;
        let lease = OfflineLease::for_helper(&target);
        launch::check_preparation(deadline, cancelled)?;
        Ok(PreparedHelper {
            workspace: self,
            target,
            lease,
            deadline,
        })
    }
}

/// The helper's result, separated from its release.
#[must_use = "settle the helper target with its outcome"]
pub(crate) enum HelperOutcome<'a> {
    /// Output after a matched clean release and the final checks. The proof
    /// keeps the target writer-excluded while its consumer verifies it.
    Accepted {
        output: Vec<u8>,
        proof: ReleasedGeneration<'a>,
    },
    /// Output withheld; the matched release proves the target quiesced.
    Refused {
        reason: NativeRefusal,
        proof: ReleasedGeneration<'a>,
    },
    /// No matched release: the target stays charged and fenced.
    Uncertain(NativeRefusal),
}

impl<'w> PreparedHelper<'w, '_> {
    pub(crate) fn slot(&self) -> ArtifactSlot {
        self.target.slot()
    }

    pub(crate) fn identity(&self) -> FileIdentity {
        self.target.identity()
    }

    pub(crate) fn deadline(&self) -> Instant {
        self.deadline
    }

    pub(crate) fn lease(&self) -> &OfflineLease {
        &self.lease
    }

    pub(crate) fn workspace(&self) -> &'w ProductionWorkspace {
        self.workspace
    }

    /// The writable staging input of the helper plan.
    pub(crate) fn staging(&self) -> Result<crate::vm::paths::Artifact, Q> {
        launch::artifact(ArtifactRole::Staging, &self.target.pin)
    }

    /// The launch target the issuer mints for and the checked creation
    /// consumes: a Mutate of exactly this slot on the staged generation.
    pub(crate) fn launch_target<'t, 'p>(
        &'t self,
        plan: &'t CommandPlan<'p>,
        cwd: &'t LocalPath,
    ) -> LaunchTarget<'t, 'p> {
        LaunchTarget {
            plan,
            writer_epoch: self.workspace.binding().generation,
            cwd,
            pair: None,
            installer: None,
            recipe: None,
            helper: Some(self.target.slot()),
            memory: JOB_MEMORY_BYTES,
            cpu_rate: JOB_CPU_RATE,
        }
    }

    /// I08/T20–T24: output only after a release of exactly `inputs`, with
    /// this target as the staging input, then a final cancellation and
    /// cutoff check. A release of anything else is no release.
    pub(crate) fn accept<'a>(
        &self,
        settlement: HelperSettlement<'a>,
        inputs: &[(ArtifactRole, FileIdentity)],
        cancelled: &dyn Fn() -> bool,
    ) -> HelperOutcome<'a> {
        let HelperSettlement {
            output,
            disposition,
            released,
        } = settlement;
        let Some(proof) = released.filter(|proof| {
            inputs.contains(&(ArtifactRole::Staging, self.target.identity()))
                && proof.matches_helper(self.workspace.binding(), PlanKind::Mutate, inputs)
        }) else {
            return HelperOutcome::Uncertain(output.err().unwrap_or(PATH));
        };
        let output = match output {
            Ok(_) if disposition != StopDisposition::CleanRelease => Err(PATH),
            output => output,
        };
        // The initial check spares the observation; the recheck after it
        // keeps a cancellation or cutoff during the re-pin from exposing
        // output.
        match output.and_then(|output| {
            launch::check_preparation(self.deadline, cancelled)?;
            self.workspace
                .pin_slot(self.slot(), self.identity(), false)?;
            #[cfg(test)]
            AFTER_REPIN.with(|hook| hook.borrow_mut().take().map(|hook| hook()));
            launch::check_preparation(self.deadline, cancelled)?;
            Ok(output)
        }) {
            Ok(output) => HelperOutcome::Accepted { output, proof },
            Err(reason) => HelperOutcome::Refused { reason, proof },
        }
    }

    /// Runs the checked helper plan against this target. A pre-launch
    /// refusal settles any created child and reports no release. Native
    /// execution is **unverified on real hardware**.
    #[allow(clippy::too_many_arguments, reason = "one checked creation's inputs")]
    pub(crate) async fn run(
        &'w self,
        qualification: &'w ForegroundQualification<'w>,
        plan: CommandPlan<'w>,
        cwd: &'w LocalPath,
        permit: impl LaunchPermit,
        cancel: &'w Shutdown,
        cancelled: &'w dyn Fn() -> bool,
        mut refresh: impl FnMut() -> Result<Instant, NativeRefusal>,
    ) -> HelperSettlement<'w> {
        let deadline = tokio::time::Instant::from_std(self.deadline);
        let request = LaunchRequest {
            plan,
            boot_inputs: None,
            cwd,
            workspace: self.workspace,
            writer_epoch: self.workspace.binding().generation,
            pair: None,
            installer: None,
            helper_target: Some(&self.target),
            memory: JOB_MEMORY_BYTES,
            cpu_rate: JOB_CPU_RATE,
            prepare_by: self.deadline,
            cancelled,
        };
        let launch = match NativeLaunch::hold(qualification, request, permit, &mut refresh) {
            Ok(launch) => launch,
            Err(mut refusal) => {
                if refusal.creation() != StartCreation::NeverCreated {
                    let _ = refusal.settle_created(deadline).await;
                }
                return HelperSettlement::quarantined(Err(refusal.reason()));
            }
        };
        launch.run_helper(cancel, deadline, refresh, || true).await
    }
}

/// The checked dispatch edge for offline leases: a Mutate plan's lease must
/// name its exact held helper target. Snapshot inspection accepts only a
/// read-only plan with the exact held disk; mutation leases cannot inspect.
pub(super) fn check_helper_lease(
    kind: PlanKind,
    offline: Option<&OfflineLease>,
    workspace: &ProductionWorkspace,
    target: Option<&HelperTarget<'_>>,
    inputs: &[(ArtifactRole, crate::vm::paths::Artifact)],
) -> Result<(), Q> {
    match (kind, offline, target) {
        (PlanKind::Mutate, Some(lease), Some(target))
            if lease.binds(target) && target.binding() == workspace.binding() =>
        {
            Ok(())
        }
        (PlanKind::InspectWritable, Some(lease), None) => {
            let inspection = lease.inspection().ok_or(PATH)?;
            let slot = inspection.slot;
            let binding = workspace.binding();
            let [(ArtifactRole::QemuImg, _), (role, disk), backing @ ..] = inputs else {
                return Err(PATH);
            };
            if inspection.workspace != binding
                || slot.role != "disk"
                || slot.generation != binding.generation
                || !matches!(
                    role,
                    ArtifactRole::GuestDisk
                        | ArtifactRole::SnapshotDisk
                        | ArtifactRole::SnapshotStagingDisk
                )
                || !matches!(backing, [] | [(ArtifactRole::Golden, _)])
                || disk.pin().is_none()
                || disk.identity() != Some(inspection.identity)
                || disk.path_for(*role).map_err(|_| PATH)?
                    != &slot_path(&workspace.root.path, slot)?
            {
                return Err(PATH);
            }
            workspace.pin_slot(slot, inspection.identity, false)?;
            Ok(())
        }
        (PlanKind::Mutate | PlanKind::InspectWritable, ..) => Err(PATH),
        _ => Ok(()),
    }
}
