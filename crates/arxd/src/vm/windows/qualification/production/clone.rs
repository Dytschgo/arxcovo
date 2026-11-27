//! Linked clone staging and failed clone claim release (ADR 0054, 7c
//! chunk 11).
//!
//! The worker stages only the sealed golden its admission verified: the
//! controller's [`SealedGolden`] re-reads that exact golden from the active
//! sealed ledger. It is held without writers and checked against its sealed
//! evidence before any clone write, and stays held through publication. The
//! overlay plan is built here and bounded by the admitted claim before any
//! dispatch; the shared checked helper runs it, and its release proof stays
//! held while the overlay header names exactly that golden and its content
//! is checkpointed as verified. VARS are copied from the held golden only
//! with a consumed [`CopyAuthority`] naming the exact slot and content.
//! Cancellation and the cutoff are rechecked after every observation that
//! precedes an effect, and the controller's publication transaction decides
//! on them again. Any other outcome drops all holds first and then settles
//! the clone as failed, which abandons its staging generation to the shared
//! resumable disposal route (7c 07): known objects retire through their
//! durable identity, an unidentified present leaf stays charged. Native
//! execution is **unverified on real hardware**.
#![cfg_attr(
    not(test),
    allow(dead_code, reason = "the closed TEST installation lands in 7c 16")
)]

use super::helper::{HelperOutcome, PreparedHelper};
use super::*;
use crate::vm::paths::{Artifact, ArtifactPin};
use crate::vm::test_issuer::CopyAuthority;
use crate::vm::{ClonePublication, FailureReason, SealedGolden, Status};

type Q = NativeRefusal;
const PATH: Q = NativeRefusal::PathCustodyUnverified;
/// A fresh overlay is metadata over its backing golden.
const OVERLAY_LIMIT: u64 = 64 << 20;

/// The admitted sealed golden a clone backs onto: its bound workspace lease,
/// both slots pinned and writer-excluded, content checked against the sealed
/// evidence.
pub(crate) struct HeldGolden {
    workspace: ProductionWorkspace,
    slots: PairSlots,
    evidence: [ArtifactEvidence; 2],
    pins: [PathPin; 2],
    guards: [File; 2],
}

impl HeldGolden {
    /// T25: the exact admitted golden, in its bound scope, before any write.
    pub(super) fn hold(
        production: &PathPin,
        owner: PSID,
        ledger: &dyn CustodyCheckpoints,
        scope: WorkspaceBinding,
        golden: &SealedGolden,
        deadline: Instant,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self, Q> {
        let held = Self::identify(
            production, owner, ledger, scope, golden, deadline, cancelled,
        )?;
        for ((guard, limit), expected) in
            held.guards.iter().zip(START_PAIR_LIMITS).zip(held.evidence)
        {
            if launch::hash_held(guard, limit, deadline, cancelled)? != expected.pin {
                return Err(PATH);
            }
        }
        WorkspaceCustody::production(&held.workspace.root, &held.workspace.lease)?;
        Ok(held)
    }

    /// The custody of [`HeldGolden::hold`] without hashing the content: the
    /// sealed ledger pair, its bound workspace lease and both slots pinned at
    /// their sealed identities and writer-excluded. Every clone and start
    /// still hashes its held golden before any write.
    pub(super) fn identify(
        production: &PathPin,
        owner: PSID,
        ledger: &dyn CustodyCheckpoints,
        scope: WorkspaceBinding,
        golden: &SealedGolden,
        deadline: Instant,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self, Q> {
        launch::check_preparation(deadline, cancelled)?;
        let (slots, evidence) = (golden.slots(), golden.evidence());
        if slots.disk.role != "disk" {
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

    pub(crate) fn identity(&self) -> FileIdentity {
        self.evidence[0].identity
    }

    /// The overlay's backing input, pinned to the sealed content.
    pub(crate) fn backing(&self) -> Result<Artifact, Q> {
        Ok(launch::artifact(ArtifactRole::Golden, &self.pins[0])?.with_pin(self.evidence[0].pin))
    }

    pub(crate) fn header(&self) -> Result<Vec<u8>, Q> {
        launch::header(&self.pins[0])
    }
}

impl ForegroundQualification {
    pub(crate) fn hold_golden(
        &self,
        ledger: &dyn CustodyCheckpoints,
        scope: WorkspaceBinding,
        golden: &SealedGolden,
        deadline: Instant,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<HeldGolden, Q> {
        HeldGolden::hold(
            self.production_root(),
            self.held.token.account.0,
            ledger,
            scope,
            golden,
            deadline,
            cancelled,
        )
    }

    /// Build reuse under the foreground gate: the sealed pair's custody as
    /// [`HeldGolden::identify`], bounded and cancellable, without hashing up
    /// to 40 GiB while every other dispatch waits on the gate.
    pub(crate) fn confirm_golden<C: crate::vm::Clock>(
        &self,
        controller: &crate::vm::VmController<C>,
        golden: &SealedGolden,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), Q> {
        let ledger = LedgerCheckpoints {
            store: controller.native_store(),
            origin: crate::store::vm::native::Origin::Clone,
        };
        let scope = controller
            .native_workspace_scope(golden.slots().disk.generation)
            .map_err(|_| PATH)?;
        HeldGolden::identify(
            self.production_root(),
            self.held.token.account.0,
            &ledger,
            binding_from_scope(scope),
            golden,
            Instant::now() + GOLDEN_CUSTODY_BUDGET,
            cancelled,
        )?;
        Ok(())
    }
}

/// Bounds the identity-only golden check made under the foreground gate.
pub(super) const GOLDEN_CUSTODY_BUDGET: Duration = Duration::from_secs(2);

/// One clone's staging scope: its planned pair, overlay size and cutoffs.
pub(crate) struct CloneRequest<'a> {
    pub slots: PairSlots,
    pub size: u64,
    pub worker_deadline: Instant,
    /// The lease expiry on the monotonic clock.
    pub effect_cutoff: Instant,
    pub owner: PSID,
    pub cancelled: &'a dyn Fn() -> bool,
}

impl CloneRequest<'_> {
    fn cutoff(&self) -> Instant {
        self.worker_deadline.min(self.effect_cutoff)
    }

    fn check(&self) -> Result<(), Q> {
        launch::check_preparation(self.cutoff(), self.cancelled)
    }
}

/// One checked creation of the stage's overlay plan against the prepared
/// target. Production uses [`CheckedOverlay`]; fixtures stand in for the
/// helper child.
pub(crate) trait OverlayHelper {
    async fn create<'a>(
        self,
        prepared: &'a PreparedHelper<'a, 'a>,
        plan: CommandPlan<'a>,
    ) -> HelperSettlement<'a>
    where
        Self: 'a;
}

/// The production overlay helper: one permit minted for exactly the stage's
/// plan, then the checked creation. **Unverified on real hardware.**
pub(crate) struct CheckedOverlay<'q, M, R> {
    pub qualification: &'q ForegroundQualification,
    pub cwd: &'q LocalPath,
    pub cancel: &'q Shutdown,
    pub cancelled: &'q dyn Fn() -> bool,
    /// Mints the Mutate permit for the exact launch target.
    pub mint: M,
    pub refresh: R,
}

impl<M, R, P> OverlayHelper for CheckedOverlay<'_, M, R>
where
    M: FnOnce(WorkspaceBinding, LaunchTarget<'_, '_>) -> Result<P, Q>,
    P: LaunchPermit,
    R: FnMut() -> Result<Instant, NativeRefusal>,
{
    async fn create<'a>(
        self,
        prepared: &'a PreparedHelper<'a, 'a>,
        plan: CommandPlan<'a>,
    ) -> HelperSettlement<'a>
    where
        Self: 'a,
    {
        let binding = prepared.workspace().binding();
        match (self.mint)(binding, prepared.launch_target(&plan, self.cwd)) {
            Ok(permit) => {
                prepared
                    .run(
                        self.qualification,
                        plan,
                        self.cwd,
                        permit,
                        self.cancel,
                        self.cancelled,
                        self.refresh,
                    )
                    .await
            }
            Err(reason) => HelperSettlement::quarantined(Err(reason)),
        }
    }
}

/// I18/T25: the overlay's sealed create plan, bounded by the admitted claim.
/// Its peak must fit the retained bytes left after the VARS copy, or nothing
/// is dispatched.
fn overlay_plan<'a>(
    prepared: &'a PreparedHelper<'a, 'a>,
    image: &'a Artifact,
    golden: &HeldGolden,
    sealed: &SealedGolden,
    size: u64,
) -> Result<CommandPlan<'a>, Q> {
    let ceiling = sealed
        .retained()
        .checked_sub(sealed.evidence()[1].pin.size)
        .ok_or(PATH)?;
    crate::vm::qemu_img::create(
        prepared.lease(),
        image,
        &prepared.staging()?,
        Some((&golden.backing()?, &golden.header()?)),
        size,
        ceiling,
    )
    .map_err(|_| PATH)
}

/// T20/T26: the accepted overlay names exactly the held golden as its
/// backing, at the requested size, while its release proof stays held.
fn verify_overlay<'a>(
    prepared: &PreparedHelper<'_, '_>,
    golden: &HeldGolden,
    ledger: &dyn CustodyCheckpoints,
    outcome: HelperOutcome<'a>,
    request: &CloneRequest<'_>,
) -> Result<(ArtifactEvidence, ReleasedGeneration<'a>), Q> {
    let proof = match outcome {
        HelperOutcome::Accepted { proof, .. } => proof,
        HelperOutcome::Refused { reason, .. } | HelperOutcome::Uncertain(reason) => {
            return Err(reason);
        }
    };
    let workspace = prepared.workspace();
    let pin = workspace.pin_slot(prepared.slot(), prepared.identity(), false)?;
    let backing = golden.backing()?;
    let backing = backing.path_for(ArtifactRole::Golden).map_err(|_| PATH)?;
    if crate::vm::qemu_img::inspect_header(&launch::header(&pin)?, Some(backing))
        != Ok(request.size)
        || !sink::held_private(&pin.leaf, request.owner)
    {
        return Err(PATH);
    }
    let content = launch::hash_held(
        &pin.leaf,
        OVERLAY_LIMIT,
        request.cutoff(),
        request.cancelled,
    )?;
    let evidence = ArtifactEvidence {
        identity: pin.identity,
        directory: workspace.binding().workspace.file,
        pin: content,
    };
    ledger.slot_verified(workspace.binding(), prepared.slot(), evidence)?;
    Ok((evidence, proof))
}

/// The golden VARS, copied into the identified clone slot only with a
/// consumed copy authority naming this exact slot and content. Cancellation
/// and the cutoff are rechecked after the authority, after the identity
/// checkpoint and before the write.
fn copy_vars(
    workspace: &ProductionWorkspace,
    golden: &HeldGolden,
    ledger: &dyn CustodyCheckpoints,
    request: &CloneRequest<'_>,
    authorize: impl FnOnce(ArtifactSlot, ArtifactPin) -> Result<CopyAuthority, Q>,
) -> Result<(ArtifactEvidence, File), Q> {
    let slot = request.slots.vars;
    let bytes = launch::held_bytes(&golden.pins[1], MAX_SETUP_VARS_BYTES)?;
    let content = ArtifactPin {
        size: bytes.len() as u64,
        sha256: crate::vm::qualification_record::digest(&bytes),
    };
    if content != golden.evidence[1].pin {
        return Err(PATH);
    }
    request.check()?;
    let authority = authorize(slot, content)?;
    if !authority.names(slot, content) {
        return Err(PATH);
    }
    request.check()?;
    let mut writer = workspace.create_slot(slot, ledger)?;
    request.check()?;
    writer.write_all(&bytes)?;
    writer.sync()?;
    let pin = writer.into_pin(false)?;
    let guard = pin.reopen_without_writer().map_err(|_| PATH)?;
    if launch::hash_held(
        &guard,
        MAX_SETUP_VARS_BYTES,
        request.cutoff(),
        request.cancelled,
    )? != content
        || !sink::held_private(&pin.leaf, request.owner)
    {
        return Err(PATH);
    }
    let evidence = ArtifactEvidence {
        identity: pin.identity,
        directory: workspace.binding().workspace.file,
        pin: content,
    };
    ledger.slot_verified(workspace.binding(), slot, evidence)?;
    Ok((evidence, guard))
}

/// Stages one linked clone and settles it exactly once (T25–T27, I15,
/// I18, I24). `sealed` is the controller's admitted golden. `settle`
/// receives the verified pair while every hold is live, or the failure
/// after every hold has been dropped, with the publication gate the
/// controller decides on in its transaction. A settlement error is returned
/// without a second settlement; the durable pending receipt stays visible
/// to recovery.
#[allow(clippy::too_many_arguments, reason = "one clone's staging steps")]
pub(crate) async fn stage_linked_clone(
    sealed: impl FnOnce() -> Result<SealedGolden, Q>,
    hold_golden: impl FnOnce(&SealedGolden) -> Result<HeldGolden, Q>,
    reserve: impl FnOnce() -> Result<ProductionWorkspace, Q>,
    ledger: &dyn CustodyCheckpoints,
    request: &CloneRequest<'_>,
    image: &Artifact,
    helper: impl OverlayHelper,
    authorize_vars: impl FnOnce(ArtifactSlot, ArtifactPin) -> Result<CopyAuthority, Q>,
    settle: impl FnOnce(
        Result<[ArtifactEvidence; 2], FailureReason>,
        ClonePublication<'_>,
    ) -> anyhow::Result<Status>,
) -> anyhow::Result<Status> {
    let mut settle = Some(settle);
    let sealed = sealed();
    let gate = |golden| ClonePublication {
        golden,
        effect_cutoff: request.effect_cutoff,
        cancelled: request.cancelled,
    };
    let staged = async {
        let slots = request.slots;
        if (slots.disk.role, slots.vars.role) != ("disk", "vars")
            || slots.disk.generation != slots.vars.generation
            || slots.disk.key == slots.vars.key
        {
            return Err(PATH);
        }
        request.check()?;
        let sealed = sealed.as_ref().map_err(|reason| *reason)?;
        let golden = hold_golden(sealed)?;
        let workspace = reserve()?;
        if workspace.binding().generation != slots.disk.generation {
            return Err(PATH);
        }
        let prepared = workspace.prepare_helper(
            slots.disk,
            ledger,
            request.worker_deadline,
            request.effect_cutoff,
            request.cancelled,
        )?;
        let inputs = [
            (ArtifactRole::QemuImg, image.identity().ok_or(PATH)?),
            (ArtifactRole::Staging, prepared.identity()),
            (ArtifactRole::Golden, golden.identity()),
        ];
        let plan = overlay_plan(&prepared, image, &golden, sealed, request.size)?;
        request.check()?;
        let settlement = helper.create(&prepared, plan).await;
        let outcome = prepared.accept(settlement, &inputs, request.cancelled);
        let (disk, _overlay) = verify_overlay(&prepared, &golden, ledger, outcome, request)?;
        let (vars, _vars) = copy_vars(&workspace, &golden, ledger, request, authorize_vars)?;
        request.check()?;
        golden.recheck()?;
        WorkspaceCustody::production(&workspace.root, &workspace.lease)?;
        request.check()?;
        let settle = settle.take().ok_or(PATH)?;
        Ok(settle(Ok([disk, vars]), gate(Some(sealed))))
    }
    .await;
    match (staged, settle) {
        (Ok(published), _) => published,
        (Err(_), Some(settle)) => {
            settle(Err(FailureReason::CloneFailed), gate(sealed.as_ref().ok()))
        }
        (Err(_), None) => anyhow::bail!("clone settlement already attempted"),
    }
}

#[cfg(test)]
#[path = "clone_tests.rs"]
pub(super) mod tests;
