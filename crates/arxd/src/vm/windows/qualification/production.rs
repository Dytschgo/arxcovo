//! One consumed foreground permit per checked process creation.

use super::recipe::{QualifiedBoot, TEST_JOB_MEMORY};
use super::*;
use crate::vm::qualification_record::Hash;
use crate::vm::{ArtifactEvidence, ArtifactSlot, Id, PairSlots};
use arx_guest::protocol::ScriptStatus;

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
    /// Requires the exact durable scope before taking the lease.
    pub(crate) fn hold_workspace(
        &self,
        expected: WorkspaceBinding,
        ledger: &dyn CustodyCheckpoints,
    ) -> Result<ProductionWorkspace, NativeRefusal> {
        ledger.confirm_bound(expected)?;
        hold_workspace_under(self.production_root(), self.held.token.account.0, expected)
    }

    pub(crate) fn hold_start_workspace(
        &self,
        expected: WorkspaceBinding,
    ) -> Result<ProductionWorkspace, NativeRefusal> {
        hold_workspace_under(self.production_root(), self.held.token.account.0, expected)
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

/// The requested pair and the durable evidence (identities, directory and
/// digests) a never-created start must still match to release cleanly.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct StartPair {
    pub slots: PairSlots,
    pub evidence: [ArtifactEvidence; 2],
}

/// A refused start keeps its original lease and creation custody available to
/// the consumer. Its scope is fixed at construction: the exact workspace
/// binding and requested pair, together with one observation cutoff. Only a
/// pre-acquire refusal carries a one-shot token for a single later attempt.
#[must_use = "settle the refused start while its generation custody is held"]
pub(crate) struct StartRefusal<'a> {
    workspace: Option<&'a ProductionWorkspace>,
    scope: Box<RefusalScope>,
    failure: HoldRefusal,
}

struct RefusalScope {
    binding: WorkspaceBinding,
    pair: Option<StartPair>,
    acquired: Option<ProductionWorkspace>,
    /// Spent by the first observation, whatever that attempt's outcome.
    acquisition: Option<()>,
    cutoff: Instant,
    child_settled: bool,
}

impl RefusalScope {
    fn new(binding: WorkspaceBinding, pair: Option<StartPair>, acquisition: bool) -> Box<Self> {
        Box::new(Self {
            binding,
            pair,
            acquired: None,
            acquisition: acquisition.then_some(()),
            cutoff: Instant::now() + REFUSAL_OBSERVATION_BUDGET,
            child_settled: false,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StartCreation {
    NeverCreated,
    Created,
    Unconfirmed,
}

pub(crate) struct HeldStartRefusal<'r> {
    workspace: &'r ProductionWorkspace,
    pair: Option<StartPair>,
    release: Option<ReleasedGeneration<'r>>,
}

impl<'r> HeldStartRefusal<'r> {
    pub(crate) fn workspace(&self) -> &'r ProductionWorkspace {
        self.workspace
    }

    pub(crate) fn pair(&self) -> Option<StartPair> {
        self.pair
    }

    pub(crate) fn release(&self) -> Option<&ReleasedGeneration<'r>> {
        self.release.as_ref()
    }
}

impl<'a> StartRefusal<'a> {
    pub(crate) fn before_create(
        workspace: Option<&'a ProductionWorkspace>,
        binding: WorkspaceBinding,
        pair: Option<StartPair>,
        reason: NativeRefusal,
    ) -> Self {
        Self {
            workspace,
            scope: RefusalScope::new(binding, pair, workspace.is_none()),
            failure: reason.into(),
        }
    }

    fn while_held<T>(
        workspace: &'a ProductionWorkspace,
        pair: Option<StartPair>,
        attempt: impl FnOnce() -> Result<T, HoldRefusal>,
    ) -> Result<T, Self> {
        attempt().map_err(|failure| Self {
            workspace: Some(workspace),
            scope: RefusalScope::new(workspace.binding(), pair, false),
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

    /// Whether a stdio worker of a created child ended with an error. This is
    /// separate from settlement, which only needs every worker to have ended.
    pub(crate) fn worker_failed(&self) -> bool {
        matches!(&self.failure.creation, LaunchCreation::Created(child) if child.worker_failed)
    }

    pub(crate) async fn settle_created(
        &mut self,
        deadline: tokio::time::Instant,
    ) -> Result<(), NativeRefusal> {
        let reason = NativeRefusal::JobUnverified;
        match &mut self.failure.creation {
            LaunchCreation::Created(child) => {
                child.settle(deadline).await.map_err(|_| reason)?;
                self.scope.child_settled = true;
                Ok(())
            }
            LaunchCreation::Unconfirmed { job, .. } => {
                let _ = job.terminate();
                Err(reason)
            }
            LaunchCreation::NeverCreated => Err(reason),
        }
    }

    /// Whether a later observation could still prove this release. An
    /// unconfirmed creation never can. A never-created start needs a held or
    /// still acquirable workspace before its fixed cutoff. A created child
    /// keeps retry ownership until it settles and its held pair is proved.
    pub(crate) fn can_progress(&self) -> bool {
        let scope = &*self.scope;
        scope.pair.is_some()
            && match self.creation() {
                StartCreation::Unconfirmed => false,
                StartCreation::Created => true,
                StartCreation::NeverCreated => {
                    Instant::now() < scope.cutoff
                        && (self.workspace.is_some()
                            || scope.acquired.is_some()
                            || scope.acquisition.is_some())
                }
            }
    }

    /// Observes the bound scope only. The caller's deadline can shorten, never
    /// extend, the cutoff fixed when the refusal was constructed.
    pub(crate) fn with_observation<T>(
        &mut self,
        deadline: Instant,
        stop: &dyn Fn() -> bool,
        acquire: impl FnOnce(WorkspaceBinding) -> Result<ProductionWorkspace, NativeRefusal>,
        consume: impl FnOnce(Option<HeldStartRefusal<'_>>) -> T,
    ) -> T {
        consume(self.observe(deadline, stop, acquire))
    }

    pub(crate) fn observe(
        &mut self,
        deadline: Instant,
        stop: &dyn Fn() -> bool,
        acquire: impl FnOnce(WorkspaceBinding) -> Result<ProductionWorkspace, NativeRefusal>,
    ) -> Option<HeldStartRefusal<'_>> {
        let never_created = self.creation() == StartCreation::NeverCreated;
        let scope = &mut *self.scope;
        let deadline = deadline.min(scope.cutoff);
        // A pre-acquire refusal can make one later, bounded attempt. A held
        // refusal always uses its original lock, including after cancellation.
        if scope.acquisition.take().is_some()
            && never_created
            && launch::check_preparation(deadline, stop).is_ok()
        {
            scope.acquired = acquire(scope.binding)
                .ok()
                .filter(|workspace| workspace.binding() == scope.binding);
        }
        let (binding, pair) = (scope.binding, scope.pair);
        self.workspace
            .or(scope.acquired.as_ref())
            .filter(|workspace| workspace.binding() == binding)
            .map(|workspace| HeldStartRefusal {
                workspace,
                pair,
                release: pair.and_then(|pair| {
                    if never_created {
                        workspace.unchanged_pair(pair, deadline, stop).ok()
                    } else if scope.child_settled {
                        workspace.released_start_pair(pair).ok()
                    } else {
                        None
                    }
                }),
            })
    }
}

impl ProductionWorkspace {
    pub(crate) fn check_start_pair(
        &self,
        pair: StartPair,
        deadline: Instant,
        stop: &dyn Fn() -> bool,
    ) -> Result<(), NativeRefusal> {
        self.unchanged_pair(pair, deadline, stop).map(drop)
    }

    pub(crate) fn start_backing(
        &self,
        (slot, evidence): (ArtifactSlot, ArtifactEvidence),
        deadline: Instant,
        stop: &dyn Fn() -> bool,
    ) -> Result<crate::vm::paths::Artifact, NativeRefusal> {
        let reason = NativeRefusal::PathCustodyUnverified;
        if slot.role != "disk" || evidence.directory != self.binding.workspace.file {
            return Err(reason);
        }
        let mut pin = self.pin_slot(slot, evidence.identity, false)?;
        if launch::hash_at_rest_until(&mut pin, START_PAIR_LIMITS[0], deadline, stop)?
            != evidence.pin
        {
            return Err(reason);
        }
        launch::artifact(ArtifactRole::Golden, &pin)
    }

    fn released_start_pair(
        &self,
        pair: StartPair,
    ) -> Result<ReleasedGeneration<'_>, NativeRefusal> {
        let reason = NativeRefusal::PathCustodyUnverified;
        check_pair(self.binding, pair.slots)?;
        let ids = pair.evidence.map(|file| file.identity);
        let pins = [
            self.pin_slot(pair.slots.disk, ids[0], false)?,
            self.pin_slot(pair.slots.vars, ids[1], false)?,
        ];
        let writers = pins
            .iter()
            .map(PathPin::reopen_without_writer)
            .collect::<io::Result<Vec<_>>>()
            .map_err(|_| reason)?;
        let mut proof = ReleasedGeneration::checked(
            self.binding,
            ReleaseTargets::Pair {
                pair: pair.slots,
                ids,
            },
            WorkspaceCustody::production(&self.root, &self.lease)?,
            (std::sync::Arc::from(pins), writers),
        )
        .ok_or(reason)?;
        proof.stop_disposition = Some(StopDisposition::DirtyRelease);
        Ok(proof)
    }

    fn unchanged_pair(
        &self,
        StartPair { slots, evidence }: StartPair,
        deadline: Instant,
        stop: &dyn Fn() -> bool,
    ) -> Result<ReleasedGeneration<'_>, NativeRefusal> {
        let reason = NativeRefusal::PathCustodyUnverified;
        launch::check_preparation(deadline, stop)?;
        check_pair(self.binding, slots)?;
        let custody = WorkspaceCustody::production(&self.root, &self.lease)?;
        let [disk, vars] = evidence;
        if disk.directory != self.binding.workspace.file || vars.directory != disk.directory {
            return Err(reason);
        }
        let pins = [
            self.pin_slot(slots.disk, disk.identity, false)?,
            self.pin_slot(slots.vars, vars.identity, false)?,
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
            ReleaseTargets::Unchanged(StartPair { slots, evidence }),
            custody,
            (std::sync::Arc::from(pins), guards),
        )
        .ok_or(reason)
    }
}

/// The full issuance binding of one checked creation: the workspace, seal
/// and plan kind, the writer epoch, the exact pair or helper slot, and a
/// digest of the exact plan, working directory, installer stage, recipe and
/// limits. The adapter recomputes it from its own target, so an authority
/// issued for any other target cannot pass the handoff.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LaunchPermitBinding {
    pub workspace: WorkspaceBinding,
    pub seal_sha256: Hash,
    pub kind: PlanKind,
    pub writer_epoch: Id,
    pub pair: Option<PairSlots>,
    pub helper: Option<ArtifactSlot>,
    pub plan_sha256: Hash,
}

pub(crate) struct LaunchTarget<'a, 'plan> {
    pub plan: &'a CommandPlan<'plan>,
    pub writer_epoch: Id,
    pub cwd: &'a LocalPath,
    pub pair: Option<PairSlots>,
    pub installer: Option<InstallerStage>,
    pub recipe: Option<crate::vm::qualification_record::RecipeIdentity>,
    /// The ledger-identified slot a Mutate plan writes, and nothing for any
    /// other plan kind.
    pub helper: Option<ArtifactSlot>,
    pub memory: usize,
    pub cpu_rate: u32,
}

impl LaunchTarget<'_, '_> {
    /// This exact target's issuance binding. The plan digest is in-process
    /// only and never persisted.
    pub(crate) fn binding(
        &self,
        workspace: WorkspaceBinding,
        seal_sha256: Hash,
    ) -> Result<LaunchPermitBinding, NativeRefusal> {
        let reason = NativeRefusal::QualificationMissing;
        let plan = self.plan;
        let mut bytes = Vec::new();
        let mut put = |part: &[u8]| {
            bytes.extend_from_slice(&(part.len() as u64).to_le_bytes());
            bytes.extend_from_slice(part);
        };
        put(format!("{:?}", plan.kind()).as_bytes());
        put(plan.image().as_str().as_bytes());
        put(&(plan.args().len() as u64).to_le_bytes());
        for arg in plan.args() {
            put(arg.as_bytes());
        }
        put(&(plan.inputs().len() as u64).to_le_bytes());
        for (role, input) in plan.inputs() {
            put(format!("{role:?}").as_bytes());
            put(input
                .path_for(*role)
                .map_err(|_| reason)?
                .as_str()
                .as_bytes());
            put(format!("{:?}", input.identity()).as_bytes());
            put(format!("{:?}", input.pin()).as_bytes());
        }
        put(self.cwd.as_str().as_bytes());
        put(format!(
            "{:?}",
            self.installer
                .map(|stage| (stage.phase(), stage.deadline()))
        )
        .as_bytes());
        put(format!("{:?}", self.recipe).as_bytes());
        put(&(self.memory as u64).to_le_bytes());
        put(&self.cpu_rate.to_le_bytes());
        Ok(LaunchPermitBinding {
            workspace,
            seal_sha256,
            kind: plan.kind(),
            writer_epoch: self.writer_epoch,
            pair: self.pair,
            helper: self.helper,
            plan_sha256: crate::vm::qualification_record::digest(&bytes),
        })
    }
}

/// The checked launch adapter's handoff: consume the permit for exactly this
/// target, then require the returned authority's issuance binding to name
/// this workspace, seal, plan, slots and writer epoch.
pub(crate) fn consume_checked(
    permit: impl LaunchPermit,
    target: LaunchTarget<'_, '_>,
    workspace: WorkspaceBinding,
    seal_sha256: Hash,
) -> Result<crate::vm::runtime::LaunchAuthority, NativeRefusal> {
    let expected = target.binding(workspace, seal_sha256)?;
    let granted = permit.consume(target)?;
    check_permit(expected, granted.binding())?;
    Ok(granted)
}

/// Implemented by the 7c issuer. Consumption must check the exact sealed
/// plan, durable intent, charges, deadline and cancellation at create, and
/// return a `LaunchAuthority` that carries its issuance-time binding.
pub(crate) trait LaunchPermit {
    fn consume(
        self,
        target: LaunchTarget<'_, '_>,
    ) -> Result<crate::vm::runtime::LaunchAuthority, NativeRefusal>;
}

pub(crate) struct LaunchRequest<'a> {
    pub plan: CommandPlan<'a>,
    pub boot_inputs: Option<&'a QualifiedBoot<'a>>,
    pub cwd: &'a LocalPath,
    pub workspace: &'a ProductionWorkspace,
    /// Current start operation, or the reserved generation for staged work.
    pub writer_epoch: Id,
    /// The requested pair with its durable evidence; a refusal is bound to it.
    pub pair: Option<StartPair>,
    pub installer: Option<InstallerStage>,
    /// The ledger-identified empty target a Mutate plan writes, and nothing
    /// for any other plan kind.
    pub helper_target: Option<&'a HelperTarget<'a>>,
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
    /// The exact unresolved writer epoch this launch was created under.
    writer_epoch: Id,
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

fn finish_spawn_preparation(
    kind: PlanKind,
    prepare_by: Instant,
    cancelled: &dyn Fn() -> bool,
    record_writer: impl FnOnce() -> Result<(), NativeRefusal>,
) -> Result<(), NativeRefusal> {
    // The read-only inspection's pins exclude writes for the whole Job.
    if kind != PlanKind::InspectWritable {
        record_writer()?;
    }
    launch::check_preparation(prepare_by, cancelled)
}

impl<'a> NativeLaunch<'a> {
    pub(crate) fn hold(
        qualification: &'a ForegroundQualification<'a>,
        request: LaunchRequest<'a>,
        permit: impl LaunchPermit,
        refresh: impl FnOnce() -> Result<Instant, NativeRefusal>,
    ) -> Result<Self, StartRefusal<'a>> {
        let (workspace, pair) = (request.workspace, request.pair);
        StartRefusal::while_held(workspace, pair, || {
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
            writer_epoch,
            pair: start_pair,
            installer,
            helper_target,
            memory,
            cpu_rate,
            prepare_by,
            cancelled,
        } = request;
        launch::check_preparation(prepare_by, cancelled)?;
        let pair = start_pair.map(|pair| pair.slots);
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
        check_start_evidence(start_pair, bound_pair)?;
        check_helper_target(kind, plan.inputs(), workspace, helper_target)?;
        helper::check_helper_lease(kind, offline, workspace, helper_target, plan.inputs())?;
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
        let (core, io, authority) = CheckedLaunchCore::hold_checked(
            &qualification.held.token,
            inputs,
            |plan| {
                let live = qualification.launch_live(memory)?;
                let captured = refresh()?;
                let target = LaunchTarget {
                    plan,
                    writer_epoch,
                    cwd,
                    pair,
                    installer,
                    recipe: boot_inputs.map(QualifiedBoot::recipe_identity),
                    helper: helper_target.map(HelperTarget::slot),
                    memory,
                    cpu_rate,
                };
                let granted = consume_checked(
                    permit,
                    target,
                    workspace_binding,
                    qualification.seal_sha256(),
                )?;
                launch::check_preparation(prepare_by, cancelled)?;
                Ok((captured.min(live), granted))
            },
            || {
                finish_spawn_preparation(kind, prepare_by, cancelled, || {
                    let store = qualification.store;
                    let pins = recovery::StorePins::open(qualification.production_root())?;
                    let origin = pins.origin(store, boot::marker()?)?;
                    store
                        .try_native_transaction(|db| {
                            launch::check_preparation(prepare_by, cancelled)
                                .map_err(|_| anyhow::anyhow!("TEST creation expired"))?;
                            crate::store::vm::native::recovery::record_job(
                                db,
                                custody::scope(workspace_binding),
                                writer_epoch,
                                origin,
                            )
                        })
                        .map_err(|_| NativeRefusal::JobUnverified)
                })
            },
        )?;
        Ok(Self {
            core,
            qualification,
            binding: workspace_binding,
            writer_epoch,
            targets,
            serial: boot_inputs.map(QualifiedBoot::serial),
            _offline: offline,
            _authority: authority,
            memory,
            io: Some(io),
            live: None,
        })
    }

    pub(crate) const START_MEMORY: usize = TEST_JOB_MEMORY;
    pub(crate) const START_CPU_RATE: u32 = JOB_CPU_RATE;

    pub(crate) async fn boot(
        &mut self,
        cancel: &Shutdown,
        boot_by: Instant,
        mut refresh: impl FnMut() -> Result<Instant, NativeRefusal>,
    ) -> Result<(), NativeRefusal> {
        match (self.core.kind, self.serial, self.io.take()) {
            (PlanKind::Boot, Some(serial), Some(io)) => {
                match self
                    .core
                    .boot_until(
                        Some((&self.qualification.held.token, serial, boot_by.into())),
                        io,
                        cancel,
                        || Ok(self.qualification.launch_live(self.memory)?.min(refresh()?)),
                        || false,
                    )
                    .await
                {
                    Ok(BootOutcome::Installed(live)) => {
                        self.live = Some(live);
                        Ok(())
                    }
                    Err(BootFailure::Native(reason)) => Err(reason),
                    _ => Err(NativeRefusal::QualificationMissing),
                }
            }
            _ => Err(NativeRefusal::QualificationMissing),
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

    /// A failed exchange returns before teardown so the owner can withdraw
    /// the public witness while the same launch still owns every handle.
    pub(crate) async fn ping(
        &mut self,
        cancel: &Shutdown,
        refresh: impl FnMut() -> Result<Instant, NativeRefusal>,
    ) -> Result<(), NativeRefusal> {
        self.guest_exchange(GuestRequest::Ping, cancel, refresh)
            .await
            .map(|_| ())
    }

    /// One guest script, only under an authority consumed for exactly these
    /// bytes, this timeout and this launch's generation and writer epoch.
    /// A typed guest result, including a nonzero exit or the guest's own
    /// timeout, is returned as is. Any transport failure returns before
    /// teardown, like [`Self::ping`], so the owner withdraws the witness
    /// first and settles the same launch.
    pub(crate) async fn run_script(
        &mut self,
        authority: crate::vm::runtime::ScriptAuthority,
        script: String,
        timeout_s: u32,
        cancel: &Shutdown,
        refresh: impl FnMut() -> Result<Instant, NativeRefusal>,
    ) -> Result<(ScriptStatus, Option<u32>), NativeRefusal> {
        if !authority.names(
            self.binding.generation,
            self.writer_epoch,
            &script,
            timeout_s,
        ) {
            return Err(NativeRefusal::QualificationMissing);
        }
        self.guest_exchange(GuestRequest::Script(script, timeout_s), cancel, refresh)
            .await?
            .ok_or(NativeRefusal::GuestEndpointUnverified)
    }

    async fn guest_exchange(
        &mut self,
        request: GuestRequest,
        cancel: &Shutdown,
        mut refresh: impl FnMut() -> Result<Instant, NativeRefusal>,
    ) -> Result<Option<(ScriptStatus, Option<u32>)>, NativeRefusal> {
        let unavailable = NativeRefusal::GuestEndpointUnverified;
        let seconds = match &request {
            GuestRequest::Ping => 5,
            GuestRequest::Script(_, timeout) => {
                u64::from(*timeout) + crate::vm::guest::SCRIPT_EXCHANGE_SLACK.as_secs()
            }
        };
        let deadline = tokio::time::Instant::now() + Duration::from_secs(seconds);
        match self.live.as_deref_mut() {
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
            Some(mut proof) => {
                proof.stop_disposition = Some(disposition);
                (disposition, Some(proof))
            }
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

impl custody::TestRelease<'_> {
    /// Store contention leaves the caller's writer exclusions in place.
    pub(crate) fn try_commit_stop(
        &self,
        store: &crate::store::vm::VmStore,
        stop: Option<&crate::store::vm::native::release::StopReceipt>,
        clock: impl FnOnce() -> crate::vm::ClockReading,
    ) -> anyhow::Result<()> {
        store.try_native_transaction(|db| {
            crate::store::vm::native::release::commit(db, self.observed(), stop, clock)?;
            Ok(())
        })
    }
}

enum GuestRequest {
    Ping,
    Script(String, u32),
}

fn check_permit(
    actual: LaunchPermitBinding,
    issued: LaunchPermitBinding,
) -> Result<(), NativeRefusal> {
    (actual == issued)
        .then_some(())
        .ok_or(NativeRefusal::QualificationMissing)
}

/// The evidence a refusal is bound to must name the plan's exact pair files.
fn check_start_evidence(
    start: Option<StartPair>,
    bound: Option<(PairSlots, [FileIdentity; 2])>,
) -> Result<(), NativeRefusal> {
    match (start, bound) {
        (None, None) => Ok(()),
        (Some(start), Some((slots, ids)))
            if start.slots == slots && start.evidence.map(|file| file.identity) == ids =>
        {
            Ok(())
        }
        _ => Err(NativeRefusal::PathCustodyUnverified),
    }
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
            let inspection = plan.reads_snapshot_disk(*role);
            let writable = role.writable() && !inspection;
            let mut pin = PathPin::open(path, *role == Data, writable)?;
            if !matches!(role, Qemu | QemuImg | Data)
                && !writable
                && !inspection
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

mod clone;
mod custody;
mod disposal;
mod helper;
mod recovery;
mod snapshot;

#[allow(unused_imports, reason = "consumed by the 7c foreground issuer")]
pub(crate) use custody::LedgerCheckpoints;
use custody::check_helper_target;
#[cfg(test)]
use custody::reserve_workspace_under;
pub(crate) use custody::{CustodyCheckpoints, HelperTarget};
#[cfg(test)]
pub(super) use custody::{WRITE_FAULT, WriteFault};
#[allow(
    unused_imports,
    reason = "consumed by the 7c foreground issuer and reaper"
)]
pub(crate) use disposal::{DisposalCheckpoints, GenerationGuards, LedgerDisposal, NativeDisposal};
#[allow(
    unused_imports,
    reason = "the production TEST recovery adapter is installed in chunk 16"
)]
pub(crate) use recovery::NativeRecovery;

#[cfg(test)]
pub(super) mod tests;

#[must_use = "settle only the generation and artifacts carried by this proof"]
pub(crate) struct ReleasedGeneration<'a> {
    binding: WorkspaceBinding,
    targets: ReleaseTargets,
    stop_disposition: Option<StopDisposition>,
    _workspace: WorkspaceCustody<'a>,
    _held: HeldWriters,
}

#[derive(Clone)]
enum ReleaseTargets {
    Pair {
        pair: PairSlots,
        ids: [FileIdentity; 2],
    },
    /// A never-created start's pair, proven against its original evidence.
    Unchanged(StartPair),
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
            ReleaseTargets::Unchanged(pair) => pair.evidence.map(|file| file.identity).to_vec(),
            ReleaseTargets::Helper {
                kind: PlanKind::InspectWritable,
                ..
            } => Vec::new(),
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
            stop_disposition: None,
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

    /// The consumer-facing check for a refused start compares the full pair
    /// evidence, including both digests, not only slots and file IDs.
    pub(crate) fn matches_start(&self, workspace: WorkspaceBinding, pair: StartPair) -> bool {
        self.binding == workspace
            && matches!(self.targets, ReleaseTargets::Unchanged(held) if held == pair)
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

#[cfg(test)]
mod release_tests;
