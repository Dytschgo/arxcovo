//! One consumed foreground permit per checked process creation.

use super::recipe::{QualifiedBoot, TEST_JOB_MEMORY};
use super::*;
use crate::vm::qualification_record::Hash;
use crate::vm::{ArtifactSlot, Id, PairSlots};
use arx_guest::protocol::ScriptStatus;

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

    pub(crate) fn slot_path(&self, slot: ArtifactSlot) -> Result<LocalPath, NativeRefusal> {
        self.valid_slot(slot)?;
        slot_path(&self.root.path, slot)
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

    pub(crate) fn open_slot(
        &self,
        slot: ArtifactSlot,
        identity: FileIdentity,
        mutable: bool,
    ) -> Result<HeldSlot, NativeRefusal> {
        self.valid_slot(slot)?;
        Ok(HeldSlot {
            pin: self.pin_slot(slot, identity, mutable)?,
            directory: self.binding.workspace.file,
        })
    }

    pub(crate) fn create_slot(
        &self,
        slot: ArtifactSlot,
        bytes: &[u8],
        authority: crate::vm::runtime::LaunchAuthority,
    ) -> Result<HeldSlot, NativeRefusal> {
        let reason = NativeRefusal::PathCustodyUnverified;
        if bytes.len() > MAX_SETUP_VARS_BYTES as usize {
            return Err(reason);
        }
        let mut writer = self.create_writer(slot, authority)?;
        writer.write(0, bytes)?;
        writer.flush()
    }

    pub(crate) fn create_writer(
        &self,
        slot: ArtifactSlot,
        _authority: crate::vm::runtime::LaunchAuthority,
    ) -> Result<SlotWriter, NativeRefusal> {
        let reason = NativeRefusal::PathCustodyUnverified;
        self.valid_slot(slot)?;
        WorkspaceCustody::production(&self.root, &self.lease)?;
        let path = slot_path(&self.root.path, slot)?;
        let leaf = path.as_str().rsplit('\\').next().ok_or(reason)?;
        let file =
            open_relative(&self.root.leaf, leaf, Node::File, true, true).map_err(|_| reason)?;
        let (identity, _) = file_identity(&file, false).map_err(|_| reason)?;
        let held = self.open_slot(slot, identity, true)?;
        Ok(SlotWriter { held, file })
    }

    pub(crate) fn fresh_disk(
        &self,
        slot: ArtifactSlot,
        authority: crate::vm::runtime::LaunchAuthority,
    ) -> Result<(HeldSlot, crate::vm::runtime::OfflineLease), NativeRefusal> {
        if !matches!(slot.role, "disk" | "verification_overlay") {
            return Err(NativeRefusal::PathCustodyUnverified);
        }
        let pin = self.create_slot(slot, &[], authority)?;
        let lease = crate::vm::runtime::OfflineLease::new(self.binding, vec![pin.identity()]);
        Ok((pin, lease))
    }

    pub(crate) fn remove_released_slot(
        &self,
        slot: ArtifactSlot,
        identity: FileIdentity,
        directory: Id,
    ) -> Result<(), NativeRefusal> {
        let reason = NativeRefusal::PathCustodyUnverified;
        self.valid_slot(slot)?;
        if directory != self.binding.workspace.file {
            return Err(reason);
        }
        WorkspaceCustody::production(&self.root, &self.lease)?;
        let path = slot_path(&self.root.path, slot)?;
        let leaf = path.as_str().rsplit('\\').next().ok_or(reason)?;
        let file = open_relative_with_access(&self.root.leaf, leaf, Node::File, false, false, true)
            .map_err(|_| reason)?;
        if file_identity(&file, false).map_err(|_| reason)?.0 != identity {
            return Err(reason);
        }
        crate::vm::publish::windows::delete_on_close(&file).map_err(|_| reason)?;
        drop(file);
        match open_relative(&self.root.leaf, leaf, Node::File, false, false) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            _ => Err(reason),
        }
    }

    fn valid_slot(&self, slot: ArtifactSlot) -> Result<(), NativeRefusal> {
        if slot.generation != self.binding.generation
            || slot.key == [0; 16]
            || !matches!(slot.role, "disk" | "vars" | "seed" | "verification_overlay")
        {
            return Err(NativeRefusal::PathCustodyUnverified);
        }
        Ok(())
    }
}

pub(crate) struct HeldSlot {
    pin: PathPin,
    directory: Id,
}

pub(crate) struct SlotWriter {
    held: HeldSlot,
    file: File,
}

impl SlotWriter {
    pub(crate) fn held(&self) -> &HeldSlot {
        &self.held
    }

    pub(crate) fn write(&mut self, offset: u64, bytes: &[u8]) -> Result<(), NativeRefusal> {
        use std::io::{Seek, SeekFrom, Write};
        self.file
            .seek(SeekFrom::Start(offset))
            .and_then(|_| self.file.write_all(bytes))
            .map_err(|_| NativeRefusal::PathCustodyUnverified)
    }

    pub(crate) fn flush(self) -> Result<HeldSlot, NativeRefusal> {
        self.file
            .sync_all()
            .map_err(|_| NativeRefusal::PathCustodyUnverified)?;
        Ok(self.held)
    }
}

impl HeldSlot {
    pub(crate) fn identity(&self) -> FileIdentity {
        self.pin.identity
    }

    pub(crate) fn observation(&self) -> Result<crate::vm::snapshot::Observation, NativeRefusal> {
        let reason = NativeRefusal::PathCustodyUnverified;
        inventory::binding(&self.pin)?;
        let (identity, _) = file_identity(&self.pin.leaf, false).map_err(|_| reason)?;
        let size = self.pin.leaf.metadata().map_err(|_| reason)?.len();
        Ok(crate::vm::snapshot::Observation { identity, size })
    }

    pub(crate) fn read(&self, offset: u64, bytes: &mut [u8]) -> Result<(), NativeRefusal> {
        use std::io::{Seek, SeekFrom};
        let mut file = &self.pin.leaf;
        file.seek(SeekFrom::Start(offset))
            .and_then(|_| file.read_exact(bytes))
            .map_err(|_| NativeRefusal::PathCustodyUnverified)
    }

    pub(crate) fn prove_offline(
        &self,
        workspace: &ProductionWorkspace,
    ) -> Result<crate::vm::runtime::OfflineLease, NativeRefusal> {
        let reason = NativeRefusal::PathCustodyUnverified;
        if workspace.binding.workspace.file != self.directory {
            return Err(reason);
        }
        let writer_exclusion = self.pin.reopen_without_writer().map_err(|_| reason)?;
        Ok(
            crate::vm::runtime::OfflineLease::new(workspace.binding, vec![self.identity()])
                .holding(writer_exclusion),
        )
    }

    pub(crate) fn artifact(
        &self,
        role: ArtifactRole,
    ) -> Result<crate::vm::paths::Artifact, NativeRefusal> {
        launch::artifact(role, &self.pin)
    }

    pub(crate) fn header(&self) -> Result<Vec<u8>, NativeRefusal> {
        launch::header(&self.pin)
    }

    pub(crate) fn vars(&self) -> Result<Vec<u8>, NativeRefusal> {
        launch::held_bytes(&self.pin, MAX_SETUP_VARS_BYTES)
    }

    pub(crate) fn evidence(
        &mut self,
        limit: u64,
        deadline: Instant,
    ) -> Result<crate::vm::ArtifactEvidence, NativeRefusal> {
        let pin = launch::hash_at_rest(&mut self.pin, limit, deadline)?;
        Ok(crate::vm::ArtifactEvidence {
            identity: self.pin.identity,
            directory: self.directory,
            pin,
        })
    }
}

impl ForegroundQualification {
    pub(crate) fn selected_store_path(&self) -> &LocalPath {
        &self.production_root().path
    }
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

    pub(crate) fn generation_workspace(
        &self,
        generation: Id,
        reserve: bool,
    ) -> Result<ProductionWorkspace, NativeRefusal> {
        let reason = NativeRefusal::PathCustodyUnverified;
        if generation == [0; 16] {
            return Err(reason);
        }
        let production = self.production_root();
        let name = hex::encode(generation);
        let path = production.path.join(&name).map_err(|_| reason)?;
        let lease = "generation.lease";
        let created = if reserve {
            inventory::binding(production)?;
            let directory = open_relative(&production.leaf, &name, Node::Directory, true, true)
                .map_err(|_| reason)?;
            let (id, _) = file_identity(&directory, true).map_err(|_| reason)?;
            if id.volume != production.identity.volume
                || !sink::held_private(&directory, self.held.token.account.0)
            {
                return Err(reason);
            }
            open_relative(&directory, lease, Node::File, true, true)
                .and_then(|file| file.sync_all())
                .map_err(|_| reason)?;
            let read_pin = open_relative(&production.leaf, &name, Node::Directory, true, false)
                .map_err(|_| reason)?;
            if file_identity(&read_pin, true).map_err(|_| reason)?.0 != id {
                return Err(reason);
            }
            drop(directory);
            Some((read_pin, id))
        } else {
            None
        };
        let workspace = self.hold_workspace(&path, lease, generation)?;
        if created.is_some_and(|(_, id)| id != workspace.binding().workspace) {
            return Err(reason);
        }
        Ok(workspace)
    }

    pub(crate) fn read_sealed_pair(
        &self,
        pair: PairSlots,
        deadline: Instant,
    ) -> Result<[crate::vm::ArtifactEvidence; 2], NativeRefusal> {
        let workspace = self.generation_workspace(pair.disk.generation, false)?;
        let reason = NativeRefusal::PathCustodyUnverified;
        let observe = |slot, limit| {
            workspace.valid_slot(slot)?;
            let mut held = HeldSlot {
                pin: PathPin::open(&slot_path(&workspace.root.path, slot)?, false, false)?,
                directory: workspace.binding.workspace.file,
            };
            if held.identity().volume != workspace.binding.workspace.volume {
                return Err(reason);
            }
            held.evidence(limit, deadline)
        };
        Ok([
            observe(pair.disk, 41 * GIB)?,
            observe(pair.vars, MAX_SETUP_VARS_BYTES)?,
        ])
    }

    pub(super) fn launch_live(&self, memory: usize) -> Result<Instant, NativeRefusal> {
        self.observe_host(memory).map(|(captured, _)| captured)
    }

    /// Admission uses the current free-space bounds from the same held
    /// volumes as the monitor. The controller applies its durable charges.
    pub(crate) fn observe_host(
        &self,
        memory: usize,
    ) -> Result<(Instant, crate::vm::HostObservation), NativeRefusal> {
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
        let inner_free = inner.caller.min(inner.total_free);
        if outer.caller.min(outer.total_free) < OUTER_FLOOR
            || inner.total != scope.production.limits.inner_total_bytes
            || inner_free == 0
        {
            return Err(NativeRefusal::DiskCeilingUnverified);
        }
        let used = inner
            .total
            .checked_sub(inner.total_free)
            .ok_or(NativeRefusal::DiskCeilingUnverified)?;
        let capacity = crate::vm::runtime::capacity(
            used,
            inner_free,
            outer.caller.min(outer.total_free),
            inner.total,
            DISK_CEILING,
            OUTER_FLOOR,
        )?;
        fresh_observation(captured, Instant::now(), NativeRefusal::ReservesUnverified)?;
        Ok((
            captured,
            crate::vm::HostObservation {
                owner_idle: Some(true),
                logical_cpus: Some(cpus),
                available_ram: Some(ram),
                capacity: Some(capacity),
            },
        ))
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
}

/// The live Job, verified QMP and guest connection, and exact writable
/// input custody stay together until teardown reconciles this generation.
#[must_use = "a created native launch needs stop or teardown"]
pub(crate) struct NativeLaunch<'a> {
    core: CheckedLaunchCore<'a>,
    qualification: &'a ForegroundQualification,
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
    if offline.is_some_and(|lease| !lease.matches(workspace_binding, plan)) {
        return Err(NativeRefusal::PathCustodyUnverified);
    }
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
        qualification: &'a ForegroundQualification,
        request: LaunchRequest<'a>,
        permit: impl LaunchPermit,
        refresh: impl FnOnce() -> Result<Instant, NativeRefusal>,
    ) -> Result<Self, NativeRefusal> {
        let LaunchRequest {
            plan,
            boot_inputs,
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
            _ => return Err(reason),
        }
        let pins = hold_plan_inputs(&plan)?;
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
                    recipe: boot_inputs.map(QualifiedBoot::recipe_identity),
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
            targets,
            serial: boot_inputs.map(QualifiedBoot::serial),
            _offline: offline,
            _authority,
            memory,
            io: Some((read, write)),
            live: None,
        })
    }

    pub(crate) async fn boot(
        &mut self,
        cancel: &Shutdown,
        mut refresh: impl FnMut() -> Result<Instant, NativeRefusal>,
        verify_release: impl FnOnce() -> bool + Send + 'static,
    ) -> Result<(), LaunchFailure<'a>> {
        let outcome = match (self.core.kind, self.serial, self.io.take()) {
            (PlanKind::Boot, Some(serial), Some(io)) => {
                self.core
                    .boot(
                        Some((&self.qualification.held.token, serial)),
                        io,
                        cancel,
                        || Ok(self.qualification.launch_live(self.memory)?.min(refresh()?)),
                        || false,
                    )
                    .await
            }
            _ => Err(BootFailure::Native(NativeRefusal::QualificationMissing)),
        };
        match outcome {
            Ok(BootOutcome::Installed(live)) => {
                self.live = Some(live);
                Ok(())
            }
            Err(BootFailure::Native(reason)) => {
                Err(self.settle_failure(reason, cancel, verify_release).await)
            }
            _ => Err(self
                .settle_failure(NativeRefusal::QualificationMissing, cancel, verify_release)
                .await),
        }
    }

    pub(crate) fn check_live(
        &self,
        refresh: impl FnOnce() -> Result<Instant, NativeRefusal>,
    ) -> Result<(), NativeRefusal> {
        self.core.check_running(
            || Ok(self.qualification.launch_live(self.memory)?.min(refresh()?)),
            false,
        )
    }

    pub(crate) async fn ping(
        &mut self,
        cancel: &Shutdown,
        refresh: impl FnMut() -> Result<Instant, NativeRefusal>,
        verify_release: impl FnOnce() -> bool + Send + 'static,
    ) -> Result<(), LaunchFailure<'a>> {
        self.guest_exchange(GuestRequest::Ping, cancel, refresh, verify_release)
            .await
            .map(|_| ())
    }

    pub(crate) async fn run_script(
        &mut self,
        _authority: crate::vm::runtime::LaunchAuthority,
        script: String,
        timeout_s: u32,
        cancel: &Shutdown,
        refresh: impl FnMut() -> Result<Instant, NativeRefusal>,
        verify_release: impl FnOnce() -> bool + Send + 'static,
    ) -> Result<(ScriptStatus, Option<u32>), LaunchFailure<'a>> {
        self.guest_exchange(
            GuestRequest::Script(script, timeout_s),
            cancel,
            refresh,
            verify_release,
        )
        .await
        .map(|result| result.expect("script request returns a script result"))
    }

    async fn guest_exchange(
        &mut self,
        request: GuestRequest,
        cancel: &Shutdown,
        mut refresh: impl FnMut() -> Result<Instant, NativeRefusal>,
        verify_release: impl FnOnce() -> bool + Send + 'static,
    ) -> Result<Option<(ScriptStatus, Option<u32>)>, LaunchFailure<'a>> {
        let unavailable = NativeRefusal::GuestEndpointUnverified;
        let seconds = match &request {
            GuestRequest::Ping => 5,
            GuestRequest::Script(_, timeout) => u64::from(*timeout) + 5,
        };
        let deadline = tokio::time::Instant::now() + Duration::from_secs(seconds);
        let outcome = match self.live.as_deref_mut() {
            Some((_, guest)) if !cancel.is_cancelled() => {
                monitor_running(
                    async {
                        match request {
                            GuestRequest::Ping => guest.ping(cancel).await.map(|()| None),
                            GuestRequest::Script(script, timeout) => {
                                guest.run_script(cancel, script, timeout).await.map(Some)
                            }
                        }
                        .map_err(|_| unavailable)
                    },
                    || {
                        self.core.check_running(
                            || Ok(self.qualification.launch_live(self.memory)?.min(refresh()?)),
                            false,
                        )
                    },
                    cancel,
                    deadline,
                    |_| unavailable,
                )
                .await
            }
            _ => Err(unavailable),
        };
        match outcome {
            Ok(result) => Ok(result),
            Err(reason) => Err(self.settle_failure(reason, cancel, verify_release).await),
        }
    }

    async fn settle_failure(
        &mut self,
        reason: NativeRefusal,
        cancel: &Shutdown,
        verify_release: impl FnOnce() -> bool + Send + 'static,
    ) -> LaunchFailure<'a> {
        let (disposition, released) = self.stop_generation(cancel, verify_release).await;
        LaunchFailure {
            reason,
            disposition,
            released: released.map(Box::new),
        }
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
        self.stop_generation(cancel, verify_release).await
    }

    pub(crate) async fn stop_generation(
        &mut self,
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

enum GuestRequest {
    Ping,
    Script(String, u32),
}

#[must_use = "settle the failed effect with its held proof or retain its charge"]
pub(crate) struct LaunchFailure<'a> {
    pub reason: NativeRefusal,
    pub disposition: StopDisposition,
    pub released: Option<Box<ReleasedGeneration<'a>>>,
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
