//! One consumed foreground permit per checked process creation.

use super::recipe::{QualifiedBoot, TEST_JOB_MEMORY};
use super::*;
use crate::vm::qualification_record::Hash;
use crate::vm::{ArtifactEvidence, ArtifactSlot, Id, PairSlots};

const GENERATION_LEASE: &str = "generation.lease";
const REFUSAL_OBSERVATION_BUDGET: Duration = Duration::from_secs(20);
const START_PAIR_LIMITS: [u64; 2] = [41 * GIB, MAX_SETUP_VARS_BYTES];

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
    pub(super) root: PathPin,
    pub(super) lease: File,
}

impl ProductionWorkspace {
    pub(crate) fn binding(&self) -> WorkspaceBinding {
        self.binding
    }

    pub(super) fn pin_slot(
        &self,
        slot: ArtifactSlot,
        identity: FileIdentity,
        mutable: bool,
    ) -> Result<PathPin, NativeRefusal> {
        let reason = NativeRefusal::PathCustodyUnverified;
        let pin = PathPin::open(&slot_path(&self.root.path, slot)?, false, mutable)?;
        if slot.generation != self.binding.generation
            || pin.identity != identity
            || pin.identity.volume != self.root.identity.volume
            || !pin._ancestors.last().is_some_and(|parent| {
                file_identity(parent, true).is_ok_and(|(id, _)| id == self.root.identity)
            })
        {
            return Err(reason);
        }
        Ok(pin)
    }
}

impl ForegroundQualification<'_> {
    pub(crate) fn hold_workspace(
        &self,
        expected: WorkspaceBinding,
    ) -> Result<ProductionWorkspace, NativeRefusal> {
        hold_workspace_under(self.production_root(), self.held.token.account.0, expected)
    }

    pub(super) fn launch_live(&self, memory: usize) -> Result<Instant, NativeRefusal> {
        let captured = Instant::now();
        self.fresh()?;
        let held = &self.held;
        if !held.current_console_idle()? {
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

fn has_exact_leaf(file: &File, name: &str) -> bool {
    use windows_sys::Win32::Storage::FileSystem::{GetFinalPathNameByHandleW, VOLUME_NAME_GUID};

    // The normalized name from the held handle supplies NTFS's stored case.
    let mut path = vec![0; 32_768];
    let count = unsafe {
        GetFinalPathNameByHandleW(
            file.as_raw_handle(),
            path.as_mut_ptr(),
            path.len() as u32,
            VOLUME_NAME_GUID,
        )
    } as usize;
    count != 0
        && count < path.len()
        && path[..count]
            .rsplit(|unit| *unit == u16::from(b'\\'))
            .next()
            .is_some_and(|leaf| leaf.iter().copied().eq(name.encode_utf16()))
}

fn hold_workspace_under(
    production: &PathPin,
    owner: PSID,
    expected: WorkspaceBinding,
) -> Result<ProductionWorkspace, NativeRefusal> {
    let reason = NativeRefusal::PathCustodyUnverified;
    inventory::binding(production)?;
    if expected.generation == [0; 16]
        || expected.production != production.identity
        || file_identity(&production.leaf, true).map_err(|_| reason)?.0 != production.identity
    {
        return Err(reason);
    }
    let name = hex::encode(expected.generation);
    let path = production.path.join(&name).map_err(|_| reason)?;
    let leaf = open_relative(&production.leaf, &name, Node::Directory, false, false)
        .map_err(|_| reason)?;
    let (identity, allocated) = file_identity(&leaf, true).map_err(|_| reason)?;
    if identity != expected.workspace
        || identity.volume != production.identity.volume
        || !has_exact_leaf(&leaf, &name)
        || !sink::held_private(&leaf, owner)
    {
        return Err(reason);
    }
    let lease =
        open_relative(&leaf, GENERATION_LEASE, Node::File, false, false).map_err(|_| reason)?;
    let (lease_id, _) = file_identity(&lease, false).map_err(|_| reason)?;
    if lease_id != expected.lease
        || lease_id.volume != identity.volume
        || !has_exact_leaf(&lease, GENERATION_LEASE)
        || lease.metadata().map_err(|_| reason)?.len() != 0
        || !sink::held_private(&lease, owner)
    {
        return Err(reason);
    }
    let ancestors = production
        ._ancestors
        .iter()
        .chain(std::iter::once(&production.leaf))
        .map(File::try_clone)
        .collect::<io::Result<Vec<_>>>()
        .map_err(|_| reason)?;
    let root = PathPin {
        _ancestors: ancestors,
        leaf,
        identity,
        allocated,
        path,
        mutable: false,
        verified_content: None,
    };
    // One nonblocking lock attempt. The returned handle is the lock owner.
    lease.try_lock().map_err(|_| reason)?;
    WorkspaceCustody::production(&root, &lease)?;
    Ok(ProductionWorkspace {
        binding: expected,
        root,
        lease,
    })
}

/// A refused start keeps its original lease and creation custody available to
/// the consumer. Only a pre-acquire refusal may attempt to open a lease.
#[must_use = "settle the refused start while its generation custody is held"]
pub(crate) struct StartRefusal<'a> {
    workspace: Option<&'a ProductionWorkspace>,
    failure: HoldRefusal,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StartCreation {
    NeverCreated,
    Created,
    Unconfirmed,
}

pub(crate) struct HeldStartRefusal<'a> {
    workspace: &'a ProductionWorkspace,
    pair: PairSlots,
    release: Option<ReleasedGeneration<'a>>,
}

impl<'a> HeldStartRefusal<'a> {
    pub(crate) fn workspace(&self) -> &'a ProductionWorkspace {
        self.workspace
    }

    pub(crate) fn pair(&self) -> PairSlots {
        self.pair
    }

    pub(crate) fn release(&self) -> Option<&ReleasedGeneration<'a>> {
        self.release.as_ref()
    }
}

impl<'a> StartRefusal<'a> {
    pub(crate) fn before_create(
        workspace: Option<&'a ProductionWorkspace>,
        reason: NativeRefusal,
    ) -> Self {
        Self {
            workspace,
            failure: reason.into(),
        }
    }

    fn while_held<T>(
        workspace: &'a ProductionWorkspace,
        attempt: impl FnOnce() -> Result<T, HoldRefusal>,
    ) -> Result<T, Self> {
        attempt().map_err(|failure| Self {
            workspace: Some(workspace),
            failure,
        })
    }

    pub(crate) fn reason(&self) -> NativeRefusal {
        self.failure.reason
    }

    pub(crate) fn creation(&self) -> StartCreation {
        match &self.failure.creation {
            LaunchCreation::NeverCreated => StartCreation::NeverCreated,
            LaunchCreation::Created(_) => StartCreation::Created,
            LaunchCreation::Unconfirmed { .. } => StartCreation::Unconfirmed,
        }
    }

    pub(crate) async fn settle_created(
        &mut self,
        deadline: tokio::time::Instant,
    ) -> Result<(), NativeRefusal> {
        let reason = NativeRefusal::JobUnverified;
        match &mut self.failure.creation {
            LaunchCreation::Created(child) => child.settle(deadline).await.map_err(|_| reason),
            LaunchCreation::Unconfirmed { job, .. } => {
                let _ = job.terminate();
                Err(reason)
            }
            LaunchCreation::NeverCreated => Err(reason),
        }
    }

    pub(crate) fn with_observation<T>(
        &self,
        expected: WorkspaceBinding,
        (pair, evidence): (PairSlots, [ArtifactEvidence; 2]),
        deadline: Instant,
        stop: &dyn Fn() -> bool,
        acquire: impl FnOnce(WorkspaceBinding) -> Result<ProductionWorkspace, NativeRefusal>,
        consume: impl FnOnce(Option<HeldStartRefusal<'_>>) -> T,
    ) -> T {
        let deadline = deadline.min(Instant::now() + REFUSAL_OBSERVATION_BUDGET);
        // A pre-acquire refusal can make one later, bounded attempt. A held
        // refusal always uses its original lock, including after cancellation.
        let newly_held = if self.workspace.is_none()
            && self.creation() == StartCreation::NeverCreated
            && launch::check_preparation(deadline, stop).is_ok()
        {
            acquire(expected).ok()
        } else {
            None
        };
        let observation = self
            .workspace
            .or(newly_held.as_ref())
            .and_then(|workspace| {
                (workspace.binding() == expected && check_pair(expected, pair).is_ok()).then(|| {
                    let release = (self.creation() == StartCreation::NeverCreated)
                        .then(|| {
                            workspace
                                .unchanged_pair(pair, evidence, deadline, stop)
                                .ok()
                        })
                        .flatten();
                    HeldStartRefusal {
                        workspace,
                        pair,
                        release,
                    }
                })
            });
        consume(observation)
    }
}

impl ProductionWorkspace {
    fn unchanged_pair(
        &self,
        pair: PairSlots,
        evidence: [ArtifactEvidence; 2],
        deadline: Instant,
        stop: &dyn Fn() -> bool,
    ) -> Result<ReleasedGeneration<'_>, NativeRefusal> {
        let reason = NativeRefusal::PathCustodyUnverified;
        launch::check_preparation(deadline, stop)?;
        let custody = WorkspaceCustody::production(&self.root, &self.lease)?;
        let [disk, vars] = evidence;
        if disk.directory != self.binding.workspace.file || vars.directory != disk.directory {
            return Err(reason);
        }
        let pins = [
            self.pin_slot(pair.disk, disk.identity, false)?,
            self.pin_slot(pair.vars, vars.identity, false)?,
        ];
        // Both exclusions must be held before reading either file and survive
        // the caller's commit decision.
        let guards = pins
            .iter()
            .map(PathPin::reopen_without_writer)
            .collect::<io::Result<Vec<_>>>()
            .map_err(|_| reason)?;
        for ((guard, limit), expected) in guards.iter().zip(START_PAIR_LIMITS).zip(evidence) {
            if launch::hash_held(guard, limit, deadline, stop)? != expected.pin {
                return Err(reason);
            }
        }
        custody.verify()?;
        launch::check_preparation(deadline, stop)?;
        ReleasedGeneration::checked(
            self.binding,
            ReleaseTargets::Pair {
                pair,
                ids: evidence.map(|file| file.identity),
            },
            custody,
            (std::sync::Arc::from(pins), guards),
        )
        .ok_or(reason)
    }
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
    pub recipe: Option<crate::vm::qualification_record::RecipeIdentity>,
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
    pub boot_inputs: Option<&'a QualifiedBoot<'a>>,
    pub cwd: &'a LocalPath,
    pub workspace: &'a ProductionWorkspace,
    pub pair: Option<PairSlots>,
    pub installer: Option<InstallerStage>,
    pub memory: usize,
    pub cpu_rate: u32,
    pub prepare_by: Instant,
    pub cancelled: &'a dyn Fn() -> bool,
}

/// The live Job, verified QMP and guest connection, and exact writable
/// input custody stay together until teardown reconciles this generation.
#[must_use = "a created native launch needs stop or teardown"]
pub(crate) struct NativeLaunch<'a> {
    core: CheckedLaunchCore<'a>,
    qualification: &'a ForegroundQualification<'a>,
    binding: WorkspaceBinding,
    targets: ReleaseTargets,
    serial: Option<&'a SerialName>,
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
    ) -> Result<Self, StartRefusal<'a>> {
        let workspace = request.workspace;
        StartRefusal::while_held(workspace, || {
            Self::hold_inner(qualification, request, permit, refresh)
        })
    }

    fn hold_inner(
        qualification: &'a ForegroundQualification<'a>,
        request: LaunchRequest<'a>,
        permit: impl LaunchPermit,
        refresh: impl FnOnce() -> Result<Instant, NativeRefusal>,
    ) -> Result<Self, HoldRefusal> {
        let LaunchRequest {
            plan,
            boot_inputs,
            cwd,
            workspace,
            pair,
            installer,
            memory,
            cpu_rate,
            prepare_by,
            cancelled,
        } = request;
        launch::check_preparation(prepare_by, cancelled)?;
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
        match (kind, boot_inputs) {
            (PlanKind::Boot | PlanKind::InstallerBoot(_), Some(inputs))
                if memory == TEST_JOB_MEMORY && cpu_rate == JOB_CPU_RATE =>
            {
                inputs.verify(
                    workspace_binding,
                    qualification.seal_sha256(),
                    qualification.evidence().recipes,
                    pair,
                    cwd,
                    &plan,
                )?
            }
            (PlanKind::Inspect | PlanKind::InspectWritable | PlanKind::Mutate, None) => {}
            _ => return Err(reason.into()),
        }
        let pins = hold_plan_inputs(&plan, prepare_by, cancelled)?;
        let targets = match bound_pair {
            Some((pair, ids)) => ReleaseTargets::Pair { pair, ids },
            None => ReleaseTargets::Helper {
                kind,
                inputs: plan
                    .inputs()
                    .iter()
                    .zip(&pins)
                    .map(|((role, _), pin)| (*role, pin.identity))
                    .collect(),
            },
        };
        let inputs = LaunchInputs {
            pins,
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
        let (core, io, authority) =
            CheckedLaunchCore::hold_checked(&qualification.held.token, inputs, |plan| {
                let live = qualification.launch_live(memory)?;
                let captured = refresh()?;
                let target = LaunchTarget {
                    plan,
                    cwd,
                    pair,
                    installer,
                    recipe: boot_inputs.map(QualifiedBoot::recipe_identity),
                    memory,
                    cpu_rate,
                };
                let (issued, granted) = permit.consume(target)?;
                check_permit(target_binding, issued)?;
                launch::check_preparation(prepare_by, cancelled)?;
                Ok((captured.min(live), granted))
            })?;
        Ok(Self {
            core,
            qualification,
            binding: workspace_binding,
            targets,
            serial: boot_inputs.map(QualifiedBoot::serial),
            _offline: offline,
            _authority: authority,
            memory,
            io: Some(io),
            live: None,
        })
    }

    pub(crate) async fn boot(
        &mut self,
        cancel: &Shutdown,
        mut refresh: impl FnMut() -> Result<Instant, NativeRefusal>,
    ) -> Result<(), NativeRefusal> {
        if self.core.kind != PlanKind::Boot {
            return Err(NativeRefusal::QualificationMissing);
        }
        let serial = self.serial.ok_or(NativeRefusal::QualificationMissing)?;
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
            Err(BootFailure::Installer(mut failure, held)) => {
                let released = held.and_then(|held| self.release_proof(held));
                if released.is_none() {
                    failure.recovery = StopDisposition::ChargedQuarantine;
                }
                Ok(InstallerSettlement::Failed { failure, released })
            }
            _ => Err(NativeRefusal::QualificationMissing),
        }
    }

    pub(crate) async fn run_helper(
        mut self,
        cancel: &Shutdown,
        deadline: tokio::time::Instant,
        mut refresh: impl FnMut() -> Result<Instant, NativeRefusal>,
        verify_release: impl FnOnce() -> bool + Send + 'static,
    ) -> HelperSettlement<'a> {
        if !matches!(
            self.core.kind,
            PlanKind::Inspect | PlanKind::InspectWritable | PlanKind::Mutate
        ) {
            return HelperSettlement::quarantined(Err(NativeRefusal::QualificationMissing));
        }
        let Some(io) = self.io.take() else {
            return HelperSettlement::quarantined(Err(NativeRefusal::QualificationMissing));
        };
        let (output, release) = self
            .core
            .helper_held(
                io,
                cancel,
                deadline,
                || Ok(self.qualification.launch_live(self.memory)?.min(refresh()?)),
                verify_release,
            )
            .await;
        let (disposition, held) = release;
        let released = held.and_then(|held| self.release_proof(held));
        match released {
            Some(released) => HelperSettlement {
                output,
                disposition,
                released: Some(released),
            },
            None => HelperSettlement::quarantined(output),
        }
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
        match held.and_then(|held| self.release_proof(held)) {
            Some(proof) => (disposition, Some(proof)),
            None => (StopDisposition::ChargedQuarantine, None),
        }
    }

    fn release_proof(&self, held: HeldWriters) -> Option<ReleasedGeneration<'a>> {
        ReleasedGeneration::checked(
            self.binding,
            self.targets.clone(),
            self.core.workspace,
            held,
        )
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

pub(super) fn check_pair(
    workspace: WorkspaceBinding,
    pair: PairSlots,
) -> Result<(), NativeRefusal> {
    (workspace.generation == pair.disk.generation
        && workspace.generation == pair.vars.generation
        && matches!(pair.disk.role, "disk" | "verification_overlay")
        && pair.vars.role == "vars"
        && pair.disk.key != pair.vars.key
        && ![pair.disk.key, pair.vars.key].contains(&[0; 16]))
    .then_some(())
    .ok_or(NativeRefusal::PathCustodyUnverified)
}

pub(super) fn slot_path(root: &LocalPath, slot: ArtifactSlot) -> Result<LocalPath, NativeRefusal> {
    root.join(&format!("{}-{}", slot.role, hex::encode(slot.key)))
        .map_err(|_| NativeRefusal::PathCustodyUnverified)
}

fn hold_plan_inputs(
    plan: &CommandPlan<'_>,
    deadline: Instant,
    stop: &dyn Fn() -> bool,
) -> Result<Vec<PathPin>, NativeRefusal> {
    use ArtifactRole::*;
    let reason = NativeRefusal::PathCustodyUnverified;
    plan.inputs()
        .iter()
        .map(|(role, artifact)| {
            launch::check_preparation(deadline, stop)?;
            let path = artifact.path_for(*role).map_err(|_| reason)?;
            let mut pin = PathPin::open(path, *role == Data, role.writable())?;
            if !matches!(role, Qemu | QemuImg | Data)
                && !role.writable()
                && launch::hash_at_rest_until(&mut pin, DISK_CEILING, deadline, stop)?
                    != artifact.pin().ok_or(reason)?
            {
                return Err(reason);
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
        let expected = slot_path(root, slot)?;
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

#[must_use = "the durable helper intent needs release or quarantine"]
pub(crate) struct HelperSettlement<'a> {
    pub output: Result<Vec<u8>, NativeRefusal>,
    pub disposition: StopDisposition,
    pub released: Option<ReleasedGeneration<'a>>,
}

impl HelperSettlement<'_> {
    fn quarantined(output: Result<Vec<u8>, NativeRefusal>) -> Self {
        Self {
            output: output.and(Err(NativeRefusal::PathCustodyUnverified)),
            disposition: StopDisposition::ChargedQuarantine,
            released: None,
        }
    }
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
pub(super) mod tests;

#[must_use = "settle only the generation and artifacts carried by this proof"]
pub(crate) struct ReleasedGeneration<'a> {
    binding: WorkspaceBinding,
    targets: ReleaseTargets,
    _workspace: WorkspaceCustody<'a>,
    _held: HeldWriters,
}

#[derive(Clone)]
enum ReleaseTargets {
    Pair {
        pair: PairSlots,
        ids: [FileIdentity; 2],
    },
    Helper {
        kind: PlanKind,
        inputs: Vec<(ArtifactRole, FileIdentity)>,
    },
}

impl<'a> ReleasedGeneration<'a> {
    fn checked(
        binding: WorkspaceBinding,
        targets: ReleaseTargets,
        workspace: WorkspaceCustody<'a>,
        held: HeldWriters,
    ) -> Option<Self> {
        let expected_writers: Vec<_> = match &targets {
            ReleaseTargets::Pair { ids, .. } => ids.to_vec(),
            ReleaseTargets::Helper { inputs, .. } => inputs
                .iter()
                .filter_map(|(role, id)| role.writable().then_some(*id))
                .collect(),
        };
        let guards = &held.1;
        if guards.len() != expected_writers.len()
            || expected_writers.iter().any(|id| {
                !guards
                    .iter()
                    .any(|guard| file_identity(guard, false).is_ok_and(|(held, _)| held == *id))
            })
        {
            return None;
        }
        Some(Self {
            binding,
            targets,
            _workspace: workspace,
            _held: held,
        })
    }

    pub(crate) fn matches(
        &self,
        workspace: WorkspaceBinding,
        pair: PairSlots,
        ids: [FileIdentity; 2],
    ) -> bool {
        self.binding == workspace
            && matches!(
                self.targets, ReleaseTargets::Pair { pair: held, ids: held_ids }
                    if held == pair && held_ids == ids
            )
    }

    pub(crate) fn matches_helper(
        &self,
        workspace: WorkspaceBinding,
        kind: PlanKind,
        inputs: &[(ArtifactRole, FileIdentity)],
    ) -> bool {
        self.binding == workspace
            && matches!(
                &self.targets, ReleaseTargets::Helper { kind: held, inputs: held_inputs }
                    if *held == kind && held_inputs == inputs
            )
    }
}
