//! One consumed foreground permit per checked process creation.

use super::*;
use crate::vm::qualification_record::Hash;
use crate::vm::{ArtifactSlot, Id, PairSlots};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WorkspaceBinding {
    pub generation: Id,
    pub production: FileIdentity,
    pub workspace: FileIdentity,
    pub lease: FileIdentity,
}

/// Existing production workspace and a live exclusive lease on an enrolled
/// slot file. The issuer owns reservation and creation of that file.
pub(crate) struct ProductionWorkspace {
    binding: WorkspaceBinding,
    root: PathPin,
    lease: File,
}

impl ProductionWorkspace {
    pub(crate) fn binding(&self) -> WorkspaceBinding {
        self.binding
    }
}

impl ForegroundQualification<'_> {
    pub(crate) fn hold_workspace(
        &self,
        root: &LocalPath,
        lease_name: &str,
        generation: Id,
    ) -> Result<ProductionWorkspace, NativeRefusal> {
        hold_workspace_under(
            self.production_root(),
            self.held.token.account.0,
            root,
            lease_name,
            generation,
        )
    }

    fn launch_live(&self, memory: usize) -> Result<Instant, NativeRefusal> {
        let captured = Instant::now();
        self.fresh()?;
        let held = &self.held;
        if !current_console_idle(&held.token)? {
            return Err(NativeRefusal::OwnerActivityUnverified);
        }
        let (cpus, ram) = host_resources()?;
        if !launch_reserves_sufficient(cpus, ram, memory) || !hypervisor_present()? {
            return Err(NativeRefusal::ReservesUnverified);
        }
        let outer = capacity::held_free(&held.volumes[1].root)?;
        let scope = held
            .scope
            .as_ref()
            .ok_or(NativeRefusal::QualificationMissing)?;
        let inner = capacity::held_free(&scope.production.root.leaf)?;
        if outer.caller.min(outer.total_free) < OUTER_FLOOR
            || inner.total != scope.production.limits.inner_total_bytes
        {
            return Err(NativeRefusal::DiskCeilingUnverified);
        }
        Ok(captured)
    }
}

/// Host WHPX kernel overhead reserved above the Job memory limit, consistent
/// with the 7 GiB headroom check for a 6 GiB Job in `recipe_live`.
const WHPX_RAM_OVERHEAD: u64 = GIB;

fn launch_reserves_sufficient(cpus: u32, available_ram: u64, memory: usize) -> bool {
    let required = u64::try_from(memory)
        .ok()
        .and_then(|memory| RAM_RESERVE.checked_add(memory))
        .and_then(|total| total.checked_add(WHPX_RAM_OVERHEAD));
    cpus >= 2 && required.is_some_and(|required| available_ram >= required)
}

fn hold_workspace_under(
    production: &PathPin,
    owner: PSID,
    root: &LocalPath,
    lease_name: &str,
    generation: Id,
) -> Result<ProductionWorkspace, NativeRefusal> {
    let reason = NativeRefusal::PathCustodyUnverified;
    if production.path.as_str().eq_ignore_ascii_case(root.as_str())
        || !production.path.contains(root)
        || generation == [0; 16]
    {
        return Err(reason);
    }
    root.join(lease_name).map_err(|_| reason)?;
    let root = PathPin::open(root, true, false)?;
    if !root
        ._ancestors
        .iter()
        .any(|parent| file_identity(parent, true).is_ok_and(|(id, _)| id == production.identity))
        || !sink::held_private(&root.leaf, owner)
    {
        return Err(reason);
    }
    let lease =
        open_relative(&root.leaf, lease_name, Node::File, false, false).map_err(|_| reason)?;
    let (lease_id, _) = file_identity(&lease, false).map_err(|_| reason)?;
    if !sink::held_private(&lease, owner) {
        return Err(reason);
    }
    lease.try_lock().map_err(|_| reason)?;
    WorkspaceCustody::production(&root, &lease)?;
    Ok(ProductionWorkspace {
        binding: WorkspaceBinding {
            generation,
            production: production.identity,
            workspace: root.identity,
            lease: lease_id,
        },
        root,
        lease,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LaunchPermitBinding {
    pub workspace: WorkspaceBinding,
    pub seal_sha256: Hash,
    pub kind: PlanKind,
}

pub(crate) struct LaunchTarget<'a, 'plan> {
    pub plan: &'a CommandPlan<'plan>,
    pub cwd: &'a LocalPath,
    pub pair: Option<PairSlots>,
    pub installer: Option<InstallerStage>,
    pub memory: usize,
    pub cpu_rate: u32,
}

/// Implemented by the 7c issuer. Consumption must check the exact sealed
/// plan, durable intent, charges, deadline and cancellation at create, and
/// return the issuance-time binding together with `LaunchAuthority`.
pub(crate) trait LaunchPermit {
    fn consume(
        self,
        target: LaunchTarget<'_, '_>,
    ) -> Result<(LaunchPermitBinding, crate::vm::runtime::LaunchAuthority), NativeRefusal>;
}

pub(crate) struct LaunchRequest<'a> {
    pub plan: CommandPlan<'a>,
    pub cwd: &'a LocalPath,
    pub workspace: &'a ProductionWorkspace,
    pub pair: Option<PairSlots>,
    pub installer: Option<InstallerStage>,
    pub memory: usize,
    pub cpu_rate: u32,
}

/// The live Job, verified QMP and guest connection, and exact writable
/// input custody stay together until teardown reconciles this generation.
#[must_use = "a created native launch needs stop or teardown"]
pub(crate) struct NativeLaunch<'a> {
    core: CheckedLaunchCore<'a>,
    qualification: &'a ForegroundQualification<'a>,
    binding: WorkspaceBinding,
    pair: Option<(PairSlots, [FileIdentity; 2])>,
    _offline: Option<&'a crate::vm::runtime::OfflineLease>,
    _authority: crate::vm::runtime::LaunchAuthority,
    memory: usize,
    io: Option<BootIo>,
    live: Option<Box<(PipeQmp, SerialGuest)>>,
}

struct ValidatedLaunch<'a> {
    workspace_binding: WorkspaceBinding,
    kind: PlanKind,
    offline: Option<&'a crate::vm::runtime::OfflineLease>,
    bound_pair: Option<(PairSlots, [FileIdentity; 2])>,
    finalization_slots: Option<PairSlots>,
}

fn check_plan_gate(
    kind: PlanKind,
    has_pair: bool,
    has_offline_lease: bool,
) -> Result<(), NativeRefusal> {
    match (kind, has_pair, has_offline_lease) {
        (PlanKind::Boot | PlanKind::InstallerBoot(_), true, false)
        | (PlanKind::Inspect, false, false)
        | (PlanKind::InspectWritable | PlanKind::Mutate, false, true) => Ok(()),
        _ => Err(NativeRefusal::QualificationMissing),
    }
}

fn validate_launch_request<'a>(
    production: FileIdentity,
    workspace: &ProductionWorkspace,
    plan: &CommandPlan<'a>,
    pair: Option<PairSlots>,
    memory: usize,
    cpu_rate: u32,
) -> Result<ValidatedLaunch<'a>, NativeRefusal> {
    let reason = NativeRefusal::QualificationMissing;
    let workspace_binding = workspace.binding();
    if workspace_binding.production != production
        || !(1..=6 * GIB as usize).contains(&memory)
        || !(1..=JOB_CPU_RATE).contains(&cpu_rate)
    {
        return Err(reason);
    }
    let kind = plan.kind();
    let offline = plan.offline_lease();
    check_plan_gate(kind, pair.is_some(), offline.is_some())?;
    let bound_pair = match (kind, pair) {
        (PlanKind::Boot | PlanKind::InstallerBoot(_), Some(pair)) => {
            check_pair(workspace_binding, pair)?;
            Some((pair, pair_ids(plan, &workspace.root.path, pair)?))
        }
        (PlanKind::Inspect | PlanKind::InspectWritable | PlanKind::Mutate, None) => None,
        _ => return Err(reason),
    };
    let finalization_slots = (kind
        == PlanKind::InstallerBoot(crate::vm::runtime::InstallerPhase::WindowsFinalize))
    .then_some(pair)
    .flatten();
    Ok(ValidatedLaunch {
        workspace_binding,
        kind,
        offline,
        bound_pair,
        finalization_slots,
    })
}

impl<'a> NativeLaunch<'a> {
    pub(crate) fn hold(
        qualification: &'a ForegroundQualification<'a>,
        request: LaunchRequest<'a>,
        permit: impl LaunchPermit,
        refresh: impl FnOnce() -> Result<Instant, NativeRefusal>,
    ) -> Result<Self, NativeRefusal> {
        let LaunchRequest {
            plan,
            cwd,
            workspace,
            pair,
            installer,
            memory,
            cpu_rate,
        } = request;
        let reason = NativeRefusal::QualificationMissing;
        let ValidatedLaunch {
            workspace_binding,
            kind,
            offline,
            bound_pair,
            finalization_slots,
        } = validate_launch_request(
            qualification.production_root().identity,
            workspace,
            &plan,
            pair,
            memory,
            cpu_rate,
        )?;
        let inputs = LaunchInputs {
            pins: hold_plan_inputs(&plan)?,
            plan,
            cwd,
            workspace: WorkspaceCustody::production(&workspace.root, &workspace.lease)?,
            bundle: qualification.held.bundle.as_ref().ok_or(reason)?,
            memory,
            cpu_rate,
            installer,
            finalization_slots,
        };
        let target_binding = LaunchPermitBinding {
            workspace: workspace_binding,
            seal_sha256: qualification.seal_sha256(),
            kind,
        };
        let mut authority = None;
        let (core, read, write) =
            CheckedLaunchCore::hold_checked(&qualification.held.token, inputs, |plan| {
                let live = qualification.launch_live(memory)?;
                let captured = refresh()?;
                let target = LaunchTarget {
                    plan,
                    cwd,
                    pair,
                    installer,
                    memory,
                    cpu_rate,
                };
                let (issued, granted) = permit.consume(target)?;
                check_permit(target_binding, issued)?;
                authority = Some(granted);
                Ok(captured.min(live))
            })?;
        let _authority = authority.ok_or(reason)?;
        Ok(Self {
            core,
            qualification,
            binding: workspace_binding,
            pair: bound_pair,
            _offline: offline,
            _authority,
            memory,
            io: Some((read, write)),
            live: None,
        })
    }

    pub(crate) async fn boot(
        &mut self,
        serial: &SerialName,
        cancel: &Shutdown,
        mut refresh: impl FnMut() -> Result<Instant, NativeRefusal>,
    ) -> Result<(), NativeRefusal> {
        if self.core.kind != PlanKind::Boot {
            return Err(NativeRefusal::QualificationMissing);
        }
        let io = self.io.take().ok_or(NativeRefusal::QualificationMissing)?;
        let outcome = self
            .core
            .boot(
                Some((&self.qualification.held.token, serial)),
                io,
                cancel,
                || Ok(self.qualification.launch_live(self.memory)?.min(refresh()?)),
                || false,
            )
            .await;
        match outcome {
            Ok(BootOutcome::Installed(live)) => {
                self.live = Some(live);
                Ok(())
            }
            Err(BootFailure::Native(reason)) => Err(reason),
            _ => Err(NativeRefusal::QualificationMissing),
        }
    }

    pub(crate) fn check_live(
        &self,
        refresh: impl FnOnce() -> Result<Instant, NativeRefusal>,
    ) -> Result<(), NativeRefusal> {
        let captured = self.qualification.launch_live(self.memory)?;
        self.core
            .stock
            .recheck_all(&self.core.pins, Some(&self.core.workspace))?;
        self.core
            .child
            .verify_live()
            .map_err(|_| NativeRefusal::JobUnverified)?;
        fresh_observation(
            captured.min(refresh()?),
            Instant::now(),
            NativeRefusal::ReservesUnverified,
        )
    }

    pub(crate) fn guest(
        &mut self,
        refresh: impl FnOnce() -> Result<Instant, NativeRefusal>,
    ) -> Result<&mut SerialGuest, NativeRefusal> {
        self.check_live(refresh)?;
        self.live
            .as_mut()
            .map(|live| &mut live.1)
            .ok_or(NativeRefusal::GuestEndpointUnverified)
    }

    pub(crate) async fn run_installer(
        &mut self,
        cancel: &Shutdown,
        mut refresh: impl FnMut() -> Result<Instant, NativeRefusal>,
        verify_release: impl FnOnce() -> bool + Send + 'static,
    ) -> Result<InstallerSettlement<'a>, NativeRefusal> {
        if !matches!(self.core.kind, PlanKind::InstallerBoot(_)) {
            return Err(NativeRefusal::QualificationMissing);
        }
        let io = self.io.take().ok_or(NativeRefusal::QualificationMissing)?;
        match self
            .core
            .boot(
                None,
                io,
                cancel,
                || Ok(self.qualification.launch_live(self.memory)?.min(refresh()?)),
                verify_release,
            )
            .await
        {
            Ok(BootOutcome::InstallerReleased(finalization, held)) => {
                Ok(InstallerSettlement::Released {
                    generation: self
                        .release_proof(held)
                        .ok_or(NativeRefusal::PathCustodyUnverified)?,
                    finalization: finalization.map(FinalizedPair),
                })
            }
            Err(BootFailure::Installer(failure, held)) => Ok(InstallerSettlement::Failed {
                failure,
                released: held.and_then(|held| self.release_proof(held)),
            }),
            _ => Err(NativeRefusal::QualificationMissing),
        }
    }

    pub(crate) async fn run_helper(
        &mut self,
        cancel: &Shutdown,
        deadline: tokio::time::Instant,
        mut refresh: impl FnMut() -> Result<Instant, NativeRefusal>,
        verify_release: impl FnOnce() -> bool + Send + 'static,
    ) -> (Result<Vec<u8>, NativeRefusal>, StopDisposition) {
        if !matches!(
            self.core.kind,
            PlanKind::Inspect | PlanKind::InspectWritable | PlanKind::Mutate
        ) {
            return (
                Err(NativeRefusal::QualificationMissing),
                StopDisposition::ChargedQuarantine,
            );
        }
        let Some(io) = self.io.take() else {
            return (
                Err(NativeRefusal::QualificationMissing),
                StopDisposition::ChargedQuarantine,
            );
        };
        self.core
            .helper(
                io,
                cancel,
                deadline,
                || Ok(self.qualification.launch_live(self.memory)?.min(refresh()?)),
                verify_release,
            )
            .await
    }

    /// Stop remains available across admission closure. Uncertain settlement
    /// leaves the generation charged. A confirmed proof keeps both writer
    /// exclusions and the workspace lease through its consumer's decision.
    pub(crate) async fn stop(
        mut self,
        cancel: &Shutdown,
        verify_release: impl FnOnce() -> bool + Send + 'static,
    ) -> (StopDisposition, Option<ReleasedGeneration<'a>>) {
        let qmp = self.live.take().map(|live| live.0);
        drop(self.io.take());
        let (disposition, held) = self.core.stop_held(qmp, cancel, verify_release).await;
        let proof = held.and_then(|held| self.release_proof(held));
        (disposition, proof)
    }

    fn release_proof(&self, held: HeldWriters) -> Option<ReleasedGeneration<'a>> {
        let (pair, ids) = self.pair?;
        Some(ReleasedGeneration {
            binding: self.binding,
            pair,
            ids,
            _workspace: self.core.workspace,
            _held: held,
        })
    }
}

fn check_permit(
    actual: LaunchPermitBinding,
    issued: LaunchPermitBinding,
) -> Result<(), NativeRefusal> {
    (actual == issued)
        .then_some(())
        .ok_or(NativeRefusal::QualificationMissing)
}

fn check_pair(workspace: WorkspaceBinding, pair: PairSlots) -> Result<(), NativeRefusal> {
    (workspace.generation == pair.disk.generation
        && workspace.generation == pair.vars.generation
        && matches!(pair.disk.role, "disk" | "verification_overlay")
        && pair.vars.role == "vars"
        && pair.disk.key != pair.vars.key)
        .then_some(())
        .ok_or(NativeRefusal::PathCustodyUnverified)
}

fn hold_plan_inputs(plan: &CommandPlan<'_>) -> Result<Vec<PathPin>, NativeRefusal> {
    use ArtifactRole::*;
    let reason = NativeRefusal::PathCustodyUnverified;
    plan.inputs()
        .iter()
        .map(|(role, artifact)| {
            let path = artifact.path_for(*role).map_err(|_| reason)?;
            let mut pin = PathPin::open(path, *role == Data, role.writable())?;
            if !matches!(role, Qemu | QemuImg | Data) && !role.writable() {
                pin.verify_content(artifact.pin().ok_or(reason)?, DISK_CEILING)?;
            }
            Ok(pin)
        })
        .collect()
}

fn pair_ids(
    plan: &CommandPlan<'_>,
    root: &LocalPath,
    pair: PairSlots,
) -> Result<[FileIdentity; 2], NativeRefusal> {
    let reason = NativeRefusal::PathCustodyUnverified;
    let find = |role, slot: ArtifactSlot| {
        let expected = root
            .join(&format!("{}-{}", slot.role, hex::encode(slot.key)))
            .map_err(|_| reason)?;
        let artifact = plan
            .inputs()
            .iter()
            .find(|(input, _)| *input == role)
            .map(|(_, artifact)| artifact)
            .ok_or(reason)?;
        if artifact.path_for(role).map_err(|_| reason)? != &expected {
            return Err(reason);
        }
        artifact.identity().ok_or(reason)
    };
    use crate::vm::runtime::InstallerPhase;
    let (disk, vars) = match plan.kind() {
        PlanKind::Boot => (ArtifactRole::GuestDisk, ArtifactRole::Vars),
        PlanKind::InstallerBoot(InstallerPhase::WindowsSetup) => (
            ArtifactRole::TestInstallDisk,
            ArtifactRole::WindowsSetupVars,
        ),
        PlanKind::InstallerBoot(_) => (ArtifactRole::TestInstallDisk, ArtifactRole::Vars),
        _ => return Err(reason),
    };
    Ok([find(disk, pair.disk)?, find(vars, pair.vars)?])
}

pub(crate) enum InstallerSettlement<'a> {
    Released {
        generation: ReleasedGeneration<'a>,
        finalization: Option<FinalizedPair>,
    },
    Failed {
        failure: InstallerFailure,
        released: Option<ReleasedGeneration<'a>>,
    },
}

pub(crate) struct FinalizedPair(offline_read::ReleasedPair);

impl FinalizedPair {
    pub(crate) fn observe(
        &self,
        slots: PairSlots,
        files: [crate::vm::ArtifactEvidence; 2],
        deadline: Instant,
    ) -> Result<(bool, bool), NativeRefusal> {
        self.0
            .observe(slots, files, deadline)
            .map_err(|_| NativeRefusal::InstallerPhaseMismatch)
    }
}

#[cfg(test)]
mod tests;

#[must_use = "settle only the generation and artifacts carried by this proof"]
pub(crate) struct ReleasedGeneration<'a> {
    binding: WorkspaceBinding,
    pair: PairSlots,
    ids: [FileIdentity; 2],
    _workspace: WorkspaceCustody<'a>,
    _held: HeldWriters,
}

impl ReleasedGeneration<'_> {
    pub(crate) fn matches(
        &self,
        workspace: WorkspaceBinding,
        pair: PairSlots,
        ids: [FileIdentity; 2],
    ) -> bool {
        self.binding == workspace && self.pair == pair && self.ids == ids
    }
}
