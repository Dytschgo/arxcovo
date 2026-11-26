//! Offline TEST snapshot capture and restore through native custody (ADR
//! 0054, 7c chunk 15).
//!
//! A copy runs only on the exact released epoch its admission recorded. The
//! worker first holds every input it reads: the active pair, the selected
//! restore point and the sealed golden, each in its durable bound scope,
//! under its workspace lease and writer-excluded. Only then does it
//! revalidate the claim (pending copy work, live lease, exact released
//! epoch, reserves), so a stale release or a claim lost while the leases
//! were acquired refuses before the candidate workspace exists. Each staged
//! slot is written only after a consumed [`CopyAuthority`] names that exact
//! slot and source content, and after the ledger recorded its create intent
//! and identity. The bounded pair copy checks cancellation, the lease
//! cutoff and the worker deadline between chunks and revalidates the claim
//! at its checkpoints. Commit runs while every hold is live; the store
//! moves the active pointer and charges atomically and persists the old
//! generation's retirement. Any other outcome drops every hold first and
//! then settles the copy as failed, which abandons the candidate to the
//! shared resumable disposal route (7c 07): identified leaves retire through
//! their durable identity and an unidentified present leaf stays charged.
//! Static VMs, Static roles and backups never reach this adapter. Native
//! execution is **unverified on real hardware**.
#![cfg_attr(
    not(test),
    allow(dead_code, reason = "the closed TEST installation lands in 7c 16")
)]

use super::*;
use crate::store::vm::native::disposal::{GenerationDisposal, GenerationState};
use crate::store::vm::snapshot::{Pair, StagedLeaf, Work};
use crate::vm::paths::ArtifactPin;
use crate::vm::qemu_args::SnapshotInfo;
use crate::vm::snapshot::{
    self as copy, ActiveRelease, Compatibility, CopyBudget, CopyCustody, CopyPlan, Destination,
    FileRole, Golden, Leaf, Limits, Lineage, Observation, Side, StoredPair,
};
use crate::vm::test_issuer::CopyAuthority;
use crate::vm::{Clock, HostObservation, PendingOperation, Resources, VmController};
use std::os::windows::fs::FileExt;

type Q = NativeRefusal;
const PATH: Q = NativeRefusal::PathCustodyUnverified;
const MAX_FILE_BYTES: u64 = 41 * GIB;
const MAX_VIRTUAL_BYTES: u64 = 40 * GIB;
/// The admission reservation's fixed restore growth, staging and metadata.
const RESTORE_GROWTH_BYTES: u64 = 1 << 20;
const STAGING_BYTES: u64 = 65_536;
const METADATA_BYTES: u64 = 4_096;
/// The ledger checkpoint is the copy's most expensive custody call; between
/// two of them only the in-memory budget, cancellation and cutoff are
/// checked.
const CHECKPOINT_INTERVAL: Duration = Duration::from_millis(500);

/// The durable bound scope of a native generation that is not disposing.
pub(crate) fn bound_scope(
    ledger: &dyn DisposalCheckpoints,
    generation: Id,
) -> Result<WorkspaceBinding, Q> {
    match ledger.generation_state(generation)? {
        GenerationState::Bound {
            scope,
            disposal: GenerationDisposal::None,
        } => Ok(super::disposal::binding_from_scope(scope)),
        _ => Err(PATH),
    }
}

/// One stored pair in its bound scope: its workspace lease, both slots
/// pinned, writer-excluded, private and at their exact names.
pub(crate) struct HeldPair {
    workspace: ProductionWorkspace,
    slots: PairSlots,
    evidence: [ArtifactEvidence; 2],
    pins: [PathPin; 2],
    guards: [File; 2],
}

impl HeldPair {
    /// T61: the exact recorded pair, under its scope, before any write.
    pub(crate) fn hold(
        production: &PathPin,
        owner: PSID,
        ledger: &dyn CustodyCheckpoints,
        scope: WorkspaceBinding,
        (slots, evidence): Pair,
        deadline: Instant,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self, Q> {
        launch::check_preparation(deadline, cancelled)?;
        if (slots.disk.role, slots.vars.role) != ("disk", "vars")
            || scope.generation != slots.disk.generation
        {
            return Err(PATH);
        }
        ledger.confirm_bound(scope)?;
        ledger.confirm_start_pair(scope, StartPair { slots, evidence })?;
        let workspace = hold_workspace_under(production, owner, scope)?;
        let pins = [
            workspace.pin_slot(slots.disk, evidence[0].identity, false)?,
            workspace.pin_slot(slots.vars, evidence[1].identity, false)?,
        ];
        let guard = |pin: &PathPin| pin.reopen_without_writer().map_err(|_| PATH);
        let guards = [guard(&pins[0])?, guard(&pins[1])?];
        for ((pin, slot), proof) in pins.iter().zip([slots.disk, slots.vars]).zip(evidence) {
            if !sink::held_private(&pin.leaf, owner)
                || !has_exact_leaf(&pin.leaf, &super::disposal::leaf_name(slot))
                || proof.directory != scope.workspace.file
            {
                return Err(PATH);
            }
        }
        WorkspaceCustody::production(&workspace.root, &workspace.lease)?;
        launch::check_preparation(deadline, cancelled)?;
        Ok(Self {
            workspace,
            slots,
            evidence,
            pins,
            guards,
        })
    }

    /// The same objects are still bound at their slots under the held lease.
    fn recheck(&self) -> Result<(), Q> {
        WorkspaceCustody::production(&self.workspace.root, &self.workspace.lease)?;
        self.workspace
            .pin_slot(self.slots.disk, self.evidence[0].identity, false)?;
        self.workspace
            .pin_slot(self.slots.vars, self.evidence[1].identity, false)?;
        Ok(())
    }

    fn path(&self, index: usize) -> LocalPath {
        self.pins[index].path.clone()
    }

    fn header(&self) -> Result<Vec<u8>, Q> {
        launch::header(&self.pins[0])
    }

    fn stored(&self, compatibility: &Compatibility) -> StoredPair {
        let leaf = |index: usize, slot: ArtifactSlot| Leaf {
            key: slot.key,
            path: self.path(index),
            identity: self.evidence[index].identity,
            pin: self.evidence[index].pin,
        };
        StoredPair {
            compatibility: compatibility.clone(),
            generation: self.slots.disk.generation,
            disk: leaf(0, self.slots.disk),
            vars: leaf(1, self.slots.vars),
        }
    }

    fn observation(&self, index: usize) -> Result<Observation, copy::Refusal> {
        let (identity, _) =
            file_identity(&self.guards[index], false).map_err(|_| copy::Refusal::Identity)?;
        let size = self.guards[index]
            .metadata()
            .map_err(|_| copy::Refusal::Io)?
            .len();
        Ok(Observation { identity, size })
    }
}

/// Every input of one copy, held from before the claim revalidation through
/// settlement.
pub(crate) struct Holds {
    pub active: HeldPair,
    /// The selected restore point; `None` for a capture.
    pub source: Option<HeldPair>,
    pub golden: HeldPair,
}

impl Holds {
    /// Holds the admitted active pair, restore source and golden in the
    /// scopes the ledger records for them.
    pub(crate) fn hold(
        production: &PathPin,
        owner: PSID,
        custody: &dyn CustodyCheckpoints,
        disposal: &dyn DisposalCheckpoints,
        work: &Work,
        deadline: Instant,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self, Q> {
        let golden = work.active.golden.ok_or(PATH)?;
        let pair = |pair: Pair| {
            HeldPair::hold(
                production,
                owner,
                custody,
                bound_scope(disposal, pair.0.disk.generation)?,
                pair,
                deadline,
                cancelled,
            )
        };
        let active = pair(work.active.pair)?;
        let source = (work.action == 6).then(|| pair(work.source)).transpose()?;
        let golden = pair(golden)?;
        Ok(Self {
            active,
            source,
            golden,
        })
    }

    fn recheck(&self) -> Result<(), Q> {
        self.active.recheck()?;
        if let Some(source) = &self.source {
            source.recheck()?;
        }
        self.golden.recheck()
    }

    /// The pair a copy reads: the restore point, or the active pair.
    fn source(&self) -> &HeldPair {
        self.source.as_ref().unwrap_or(&self.active)
    }
}

/// The ledger side of one admitted copy. Production uses
/// [`ControllerSnapshot`]; every call commits or refuses before it returns.
pub(crate) trait SnapshotLedger {
    /// Claim revalidation: the copy-phase work and its pending receipt, the
    /// live lease, the exact released epoch and the reserves.
    fn checkpoint(&self) -> anyhow::Result<()>;
    /// The staged leaf's verified content, after the native slot verified.
    fn leaf_verified(&self, slot: ArtifactSlot, evidence: ArtifactEvidence) -> anyhow::Result<()>;
    /// The atomic pointer and charge transaction.
    fn commit(&self, proof: [ArtifactEvidence; 2]) -> anyhow::Result<()>;
    /// Abandons the candidate to the shared disposal route.
    fn fail(&self, leaves: [StagedLeaf; 2]) -> anyhow::Result<()>;
}

/// The controller's snapshot transactions for one admitted worker.
pub(crate) struct ControllerSnapshot<'a, C> {
    pub controller: &'a VmController<C>,
    pub worker: &'a PendingOperation,
    pub work: &'a Work,
    pub host: &'a dyn Fn() -> HostObservation,
    pub resources: Resources,
}

impl<C: Clock> SnapshotLedger for ControllerSnapshot<'_, C> {
    fn checkpoint(&self) -> anyhow::Result<()> {
        self.controller
            .snapshot_checkpoint(self.worker, self.work, (self.host)(), self.resources)
    }

    fn leaf_verified(&self, slot: ArtifactSlot, evidence: ArtifactEvidence) -> anyhow::Result<()> {
        self.controller
            .snapshot_leaf_complete(self.worker, self.work, slot, evidence)
    }

    fn commit(&self, proof: [ArtifactEvidence; 2]) -> anyhow::Result<()> {
        self.controller.finish_snapshot_copy(
            self.worker,
            self.work,
            proof,
            None,
            (self.host)(),
            self.resources,
        )
    }

    fn fail(&self, leaves: [StagedLeaf; 2]) -> anyhow::Result<()> {
        self.controller
            .fail_snapshot_copy(self.worker, self.work, leaves)
    }
}

/// The disk a sealed read-only inspection names, held by this copy.
pub(crate) struct InspectTarget<'a> {
    pub workspace: &'a ProductionWorkspace,
    pub slot: ArtifactSlot,
    pub identity: FileIdentity,
    /// The writer-excluded handle the copy holds on that disk.
    pub file: &'a File,
}

/// The sealed read-only `qemu-img info` for one held disk. Production
/// requires a checked writable-inspection launch, which stays closed until
/// its consumer installs it; fixtures stand in for the child.
pub(crate) trait SnapshotInspector {
    fn inspect(
        &mut self,
        target: InspectTarget<'_>,
        info: &SnapshotInfo<'_>,
        output: &mut [u8],
        budget: &CopyBudget<'_>,
    ) -> Result<usize, copy::Refusal>;
}

/// One admitted copy's scope.
pub(crate) struct SnapshotRequest<'a> {
    pub work: &'a Work,
    pub worker: &'a PendingOperation,
    /// The enrolled production root the plan's paths live under.
    pub production: &'a PathPin,
    pub owner: PSID,
    pub firmware: ArtifactPin,
    /// The lease expiry on the monotonic clock.
    pub effect_cutoff: Instant,
    pub cancelled: &'a dyn Fn() -> bool,
}

impl SnapshotRequest<'_> {
    fn cutoff(&self) -> Instant {
        self.worker.deadline().min(self.effect_cutoff)
    }

    fn stopped(&self) -> bool {
        (self.cancelled)() || Instant::now() >= self.effect_cutoff
    }

    fn check(&self) -> Result<(), Q> {
        launch::check_preparation(self.cutoff(), self.cancelled)
    }
}

/// The copy's custody over real handles. Reads use the held, writer-excluded
/// handles; writes go only to the identified staging writers.
struct NativeCopy<'a, 'w> {
    holds: &'a Holds,
    destination: &'w ProductionWorkspace,
    slots: PairSlots,
    owner: PSID,
    staging: [Option<custody::SlotWriter<'w>>; 2],
    written: [u64; 2],
    flushed: [Option<(PathPin, File)>; 2],
    ledger: &'a dyn SnapshotLedger,
    inspector: &'a mut dyn SnapshotInspector,
    last_check: Option<Instant>,
}

fn index(file: FileRole) -> usize {
    usize::from(file == FileRole::Vars)
}

fn read_exact_at(file: &File, offset: u64, bytes: &mut [u8]) -> Result<(), copy::Refusal> {
    let mut done = 0;
    while done < bytes.len() {
        let at = offset
            .checked_add(done as u64)
            .ok_or(copy::Refusal::Bounds)?;
        match file.seek_read(&mut bytes[done..], at) {
            Ok(0) | Err(_) => return Err(copy::Refusal::Io),
            Ok(count) => done += count,
        }
    }
    Ok(())
}

impl NativeCopy<'_, '_> {
    fn held(&self, side: Side) -> Result<&HeldPair, copy::Refusal> {
        match side {
            Side::Active => Ok(&self.holds.active),
            Side::Source => Ok(self.holds.source()),
            Side::Staging => Err(copy::Refusal::Custody),
        }
    }

    fn flushed(&self, file: usize) -> Result<&(PathPin, File), copy::Refusal> {
        self.flushed[file].as_ref().ok_or(copy::Refusal::Custody)
    }
}

impl CopyCustody for NativeCopy<'_, '_> {
    fn checkpoint(
        &mut self,
        _: &CopyPlan,
        _: &PendingOperation,
        budget: &CopyBudget<'_>,
    ) -> Result<(), copy::Refusal> {
        budget.check()?;
        if self
            .last_check
            .is_some_and(|last| last.elapsed() < CHECKPOINT_INTERVAL)
        {
            return Ok(());
        }
        self.holds.recheck().map_err(|_| copy::Refusal::Custody)?;
        WorkspaceCustody::production(&self.destination.root, &self.destination.lease)
            .map_err(|_| copy::Refusal::Custody)?;
        self.ledger
            .checkpoint()
            .map_err(|_| copy::Refusal::Custody)?;
        self.last_check = Some(Instant::now());
        budget.check()
    }

    fn observe(
        &mut self,
        side: Side,
        budget: &CopyBudget<'_>,
    ) -> Result<[Observation; 2], copy::Refusal> {
        budget.check()?;
        if side != Side::Staging {
            let held = self.held(side)?;
            return Ok([held.observation(0)?, held.observation(1)?]);
        }
        let observe = |file: usize| -> Result<Observation, copy::Refusal> {
            if let Some((pin, guard)) = &self.flushed[file] {
                let (identity, _) =
                    file_identity(guard, false).map_err(|_| copy::Refusal::Identity)?;
                if identity != pin.identity {
                    return Err(copy::Refusal::Identity);
                }
                let size = guard.metadata().map_err(|_| copy::Refusal::Io)?.len();
                return Ok(Observation { identity, size });
            }
            let writer = self.staging[file].as_ref().ok_or(copy::Refusal::Custody)?;
            Ok(Observation {
                identity: writer.identity(),
                size: writer.len().map_err(|_| copy::Refusal::Io)?,
            })
        };
        Ok([observe(0)?, observe(1)?])
    }

    fn read(
        &mut self,
        side: Side,
        file: FileRole,
        offset: u64,
        bytes: &mut [u8],
        budget: &CopyBudget<'_>,
    ) -> Result<(), copy::Refusal> {
        budget.check()?;
        let handle = match side {
            Side::Staging => &self.flushed(index(file))?.1,
            side => &self.held(side)?.guards[index(file)],
        };
        read_exact_at(handle, offset, bytes)
    }

    fn write_staging(
        &mut self,
        file: FileRole,
        offset: u64,
        bytes: &[u8],
        budget: &CopyBudget<'_>,
    ) -> Result<(), copy::Refusal> {
        budget.check()?;
        let at = index(file);
        if offset != self.written[at] {
            return Err(copy::Refusal::Io);
        }
        self.staging[at]
            .as_mut()
            .ok_or(copy::Refusal::Custody)?
            .write_all(bytes)
            .map_err(|_| copy::Refusal::Io)?;
        self.written[at] = offset
            .checked_add(bytes.len() as u64)
            .ok_or(copy::Refusal::Bounds)?;
        Ok(())
    }

    fn golden_header(
        &mut self,
        golden: &Golden,
        header: &mut [u8],
        budget: &CopyBudget<'_>,
    ) -> Result<(), copy::Refusal> {
        budget.check()?;
        let held = &self.holds.golden;
        if held.observation(0)?.identity != golden.identity
            || held.evidence[0].identity != golden.identity
            || held.evidence[0].pin != golden.pin
            || held.path(0) != golden.path
            || held.slots.disk.key != golden.object
        {
            return Err(copy::Refusal::Identity);
        }
        read_exact_at(&held.guards[0], 0, header)
    }

    fn inspect(
        &mut self,
        info: &SnapshotInfo<'_>,
        output: &mut [u8],
        budget: &CopyBudget<'_>,
    ) -> Result<usize, copy::Refusal> {
        budget.check()?;
        let named = info.disk().identity();
        let (workspace, slot, identity, file) = if let Some((pin, guard)) = &self.flushed[0]
            && *info.path() == pin.path
        {
            (self.destination, self.slots.disk, pin.identity, guard)
        } else {
            let source = self.holds.source();
            let active = &self.holds.active;
            let held = [source, active]
                .into_iter()
                .find(|held| *info.path() == held.pins[0].path)
                .ok_or(copy::Refusal::Inspection)?;
            (
                &held.workspace,
                held.slots.disk,
                held.evidence[0].identity,
                &held.guards[0],
            )
        };
        if named != Some(identity) {
            return Err(copy::Refusal::Identity);
        }
        let len = self.inspector.inspect(
            InspectTarget {
                workspace,
                slot,
                identity,
                file,
            },
            info,
            output,
            budget,
        )?;
        workspace
            .pin_slot(slot, identity, false)
            .map_err(|_| copy::Refusal::Custody)?;
        budget.check()?;
        Ok(len)
    }

    fn flush_staging(&mut self, budget: &CopyBudget<'_>) -> Result<(), copy::Refusal> {
        for file in 0..2 {
            budget.check()?;
            let writer = self.staging[file].take().ok_or(copy::Refusal::Custody)?;
            writer.sync().map_err(|_| copy::Refusal::Io)?;
            let pin = writer.into_pin(false).map_err(|_| copy::Refusal::Custody)?;
            let guard = pin
                .reopen_without_writer()
                .map_err(|_| copy::Refusal::Custody)?;
            if !sink::held_private(&pin.leaf, self.owner) {
                return Err(copy::Refusal::Custody);
            }
            self.flushed[file] = Some((pin, guard));
        }
        Ok(())
    }
}

/// The plan for exactly this admitted work over the held inputs.
fn plan(request: &SnapshotRequest<'_>, holds: &Holds) -> Result<CopyPlan, Q> {
    let work = request.work;
    let slots = work.candidate.ok_or(PATH)?;
    let golden = &holds.golden;
    let backing = golden.path(0);
    let size = crate::vm::qemu_img::inspect_header(&holds.active.header()?, Some(&backing))
        .map_err(|_| PATH)?;
    if crate::vm::qemu_img::inspect_header(&golden.header()?, None) != Ok(size)
        || crate::vm::qemu_img::inspect_header(&holds.source().header()?, Some(&backing))
            != Ok(size)
    {
        return Err(PATH);
    }
    let compatibility = Compatibility {
        host: work.active.host,
        vm: work.vm,
        lineage: Lineage::Test {
            template: work.active.template,
            golden: Golden {
                object: golden.slots.disk.key,
                path: backing,
                identity: golden.evidence[0].identity,
                pin: golden.evidence[0].pin,
            },
        },
        recipe: work.active.recipe,
        firmware: request.firmware,
        virtual_disk_bytes: size,
        vars_bytes: holds.active.evidence[1].pin.size,
    };
    let snapshot = work.target;
    let action = match work.action {
        5 => copy::Action::Capture { snapshot },
        6 => copy::Action::Restore { snapshot },
        _ => return Err(PATH),
    };
    let [vm_points, host_points] = work.points;
    let plan = CopyPlan::new(
        copy::Intent {
            activity_id: work.activity,
            action,
            active: holds.active.stored(&compatibility),
            active_writer_epoch: work.active.epoch,
            release: ActiveRelease {
                generation: work.active.pair.0.disk.generation,
                writer_epoch: work.active.epoch,
                disposition: if work.active.clean {
                    StopDisposition::CleanRelease
                } else {
                    StopDisposition::DirtyRelease
                },
                // Both active slots are writer-excluded under their lease.
                endpoints_released: true,
            },
            selected: holds
                .source
                .as_ref()
                .map(|source| (snapshot, source.stored(&compatibility))),
            destination: Destination {
                generation: slots.disk.generation,
                disk_key: slots.disk.key,
                vars_key: slots.vars.key,
            },
        },
        &request.production.path,
        request.production.identity.volume,
        Limits {
            vm_points,
            host_points,
            max_file_bytes: MAX_FILE_BYTES,
            max_virtual_bytes: MAX_VIRTUAL_BYTES,
            restore_growth_bytes: if work.action == 6 {
                RESTORE_GROWTH_BYTES
            } else {
                0
            },
            staging_bytes: STAGING_BYTES,
            metadata_bytes: METADATA_BYTES,
        },
    )
    .map_err(|_| PATH)?;
    if plan.additional_claim().total() != work.claim.total() {
        return Err(PATH);
    }
    Ok(plan)
}

/// I22: only an admitted TEST capture or restore into its own candidate.
fn check_work(work: &Work, worker: &PendingOperation) -> Result<PairSlots, Q> {
    let slots = work.candidate.ok_or(PATH)?;
    let restore_source = work.source.0.disk.generation;
    if work.is_static
        || !matches!(work.action, 5 | 6)
        || worker.activity_id() != work.activity
        || (slots.disk.role, slots.vars.role) != ("disk", "vars")
        || slots.disk.generation != slots.vars.generation
        || slots.disk.key == slots.vars.key
        || slots.disk.generation == work.active.pair.0.disk.generation
        || (work.action == 6) == (restore_source == work.active.pair.0.disk.generation)
        || (work.action == 5 && work.source != work.active.pair)
    {
        return Err(PATH);
    }
    Ok(slots)
}

/// Copies one admitted offline snapshot and settles it exactly once (T61–T64,
/// I11, I16–I19, I22–I24). `hold` acquires every input's lease and writer
/// exclusion, `reserve` creates the candidate's bound workspace through
/// `custody`, and `authorize` consumes one copy permit per staged slot. On
/// success the commit runs while every hold is live; on any failure all
/// holds and handles are dropped before the failed settlement. A settlement
/// error is returned without a second settlement; the durable pending
/// receipt stays visible to recovery.
#[allow(clippy::too_many_arguments, reason = "one copy's custody inputs")]
pub(crate) fn copy_offline_snapshot(
    request: &SnapshotRequest<'_>,
    hold: impl FnOnce() -> Result<Holds, Q>,
    reserve: impl FnOnce() -> Result<ProductionWorkspace, Q>,
    custody: &dyn CustodyCheckpoints,
    mut authorize: impl FnMut(ArtifactSlot, ArtifactPin) -> Result<CopyAuthority, Q>,
    inspector: &mut dyn SnapshotInspector,
    ledger: &dyn SnapshotLedger,
) -> anyhow::Result<()> {
    let q = |reason: Q| anyhow::anyhow!("native snapshot custody refused: {reason:?}");
    let mut leaves = [StagedLeaf::Untouched; 2];
    let staged = (|| -> anyhow::Result<()> {
        let slots = check_work(request.work, request.worker).map_err(q)?;
        request.check().map_err(q)?;
        let holds = hold().map_err(q)?;
        // The claim may have moved while the leases were acquired.
        ledger.checkpoint()?;
        let plan = plan(request, &holds).map_err(q)?;
        request.check().map_err(q)?;
        let workspace = reserve().map_err(q)?;
        let directory = workspace.binding().workspace.file;
        if workspace.binding().generation != slots.disk.generation {
            return Err(q(PATH));
        }
        let source = holds.source();
        let mut writers = [None, None];
        for (at, slot) in [slots.disk, slots.vars].into_iter().enumerate() {
            let content = source.evidence[at].pin;
            request.check().map_err(q)?;
            let authority = authorize(slot, content).map_err(q)?;
            if !authority.names(slot, content) {
                return Err(q(PATH));
            }
            request.check().map_err(q)?;
            let writer = workspace.create_slot(slot, custody).map_err(q)?;
            leaves[at] = StagedLeaf::Identified(writer.identity(), directory);
            writers[at] = Some(writer);
        }
        let ids = [0, 1].map(|at| writers[at].as_ref().map(custody::SlotWriter::identity));
        let [Some(disk_id), Some(vars_id)] = ids else {
            return Err(q(PATH));
        };
        let mut native = NativeCopy {
            holds: &holds,
            destination: &workspace,
            slots,
            owner: request.owner,
            staging: writers,
            written: [0; 2],
            flushed: [None, None],
            ledger,
            inspector,
            last_check: None,
        };
        let stopped = || request.stopped();
        let mut verified = copy::copy_pair(
            &plan,
            request.worker,
            &mut native,
            [disk_id, vars_id],
            &stopped,
        )
        .map_err(|reason| anyhow::anyhow!("snapshot copy refused: {reason:?}"))?;
        let pair = verified.pair();
        let proof = [&pair.disk, &pair.vars].map(|leaf| ArtifactEvidence {
            identity: leaf.identity,
            directory,
            pin: leaf.pin,
        });
        for (slot, evidence) in [slots.disk, slots.vars].into_iter().zip(proof) {
            custody
                .slot_verified(workspace.binding(), slot, evidence)
                .map_err(q)?;
            ledger.leaf_verified(slot, evidence)?;
        }
        verified
            .recheck()
            .map_err(|reason| anyhow::anyhow!("snapshot recheck refused: {reason:?}"))?;
        drop(verified);
        holds.recheck().map_err(q)?;
        WorkspaceCustody::production(&workspace.root, &workspace.lease).map_err(q)?;
        request.check().map_err(q)?;
        // Every input and both staged leaves stay held through the commit.
        let committed = ledger.commit(proof);
        drop(native);
        drop(workspace);
        drop(holds);
        committed
    })();
    match staged {
        Ok(()) => Ok(()),
        Err(error) => {
            // Every hold and handle was dropped with the closure's frame.
            ledger.fail(leaves)?;
            Err(error)
        }
    }
}

#[cfg(test)]
#[path = "snapshot_tests.rs"]
mod tests;
