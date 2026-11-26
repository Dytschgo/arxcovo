//! Linked clone staging and failed clone claim release (ADR 0054, 7c
//! chunk 11).
//!
//! The exact sealed golden is held without writers and checked against its
//! sealed evidence before any clone write, and stays held through
//! publication. The overlay is created by the shared checked helper; its
//! release proof stays held while the overlay header names exactly that
//! golden and its content is checkpointed as verified. VARS are copied from
//! the held golden under a consumed copy permit. Publication happens once,
//! under every hold, before the earliest cutoff. Any other outcome drops all
//! holds first and then settles the clone as failed, which abandons its
//! staging generation to the shared resumable disposal route (7c 07): known
//! objects retire through their durable identity, an unidentified present
//! leaf stays charged. Native execution is **unverified on real hardware**.
#![cfg_attr(
    not(test),
    allow(dead_code, reason = "the closed TEST installation lands in 7c 16")
)]

use super::helper::{HelperOutcome, PreparedHelper};
use super::*;
use crate::vm::paths::{Artifact, ArtifactPin};
use crate::vm::{FailureReason, Status};

type Q = NativeRefusal;
const PATH: Q = NativeRefusal::PathCustodyUnverified;
/// A fresh overlay is metadata over its backing golden.
const OVERLAY_LIMIT: u64 = 64 << 20;

/// The sealed golden a clone backs onto: its bound workspace lease, both
/// slots pinned and writer-excluded, content checked against the sealed
/// evidence.
pub(crate) struct HeldGolden {
    workspace: ProductionWorkspace,
    slots: PairSlots,
    evidence: [ArtifactEvidence; 2],
    pins: [PathPin; 2],
    _guards: [File; 2],
}

impl HeldGolden {
    /// T25: the exact bound golden and its sealed pair before any write.
    #[allow(clippy::too_many_arguments, reason = "one held golden's scope")]
    pub(super) fn hold(
        production: &PathPin,
        owner: PSID,
        ledger: &dyn CustodyCheckpoints,
        scope: WorkspaceBinding,
        slots: PairSlots,
        evidence: [ArtifactEvidence; 2],
        deadline: Instant,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self, Q> {
        launch::check_preparation(deadline, cancelled)?;
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
        for ((guard, limit), expected) in guards.iter().zip(START_PAIR_LIMITS).zip(evidence) {
            if launch::hash_held(guard, limit, deadline, cancelled)? != expected.pin {
                return Err(PATH);
            }
        }
        WorkspaceCustody::production(&workspace.root, &workspace.lease)?;
        Ok(Self {
            workspace,
            slots,
            evidence,
            pins,
            _guards: guards,
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

impl ForegroundQualification<'_> {
    pub(crate) fn hold_golden(
        &self,
        ledger: &dyn CustodyCheckpoints,
        scope: WorkspaceBinding,
        slots: PairSlots,
        evidence: [ArtifactEvidence; 2],
        deadline: Instant,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<HeldGolden, Q> {
        HeldGolden::hold(
            self.production_root(),
            self.held.token.account.0,
            ledger,
            scope,
            slots,
            evidence,
            deadline,
            cancelled,
        )
    }
}

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

/// One checked overlay creation against the prepared target. Production
/// uses [`CheckedOverlay`]; fixtures stand in for the helper child.
pub(crate) trait OverlayHelper {
    /// The pinned qemu-img identity the release proof must name.
    fn image(&self) -> FileIdentity;

    async fn create<'a>(
        self,
        prepared: &'a PreparedHelper<'a, 'a>,
        golden: &'a HeldGolden,
        size: u64,
    ) -> HelperSettlement<'a>
    where
        Self: 'a;
}

/// The production overlay helper: the sealed qemu-img create plan, one
/// permit minted for exactly that plan, and the checked creation.
/// **Unverified on real hardware.**
pub(crate) struct CheckedOverlay<'q, M, R> {
    pub qualification: &'q ForegroundQualification<'q>,
    pub image: &'q Artifact,
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
    fn image(&self) -> FileIdentity {
        self.image.identity().unwrap_or(FileIdentity {
            volume: 0,
            file: [0; 16],
        })
    }

    async fn create<'a>(
        self,
        prepared: &'a PreparedHelper<'a, 'a>,
        golden: &'a HeldGolden,
        size: u64,
    ) -> HelperSettlement<'a>
    where
        Self: 'a,
    {
        let plan = match (golden.backing(), golden.header(), prepared.staging()) {
            (Ok(backing), Ok(header), Ok(staging)) => crate::vm::qemu_img::create(
                prepared.lease(),
                self.image,
                &staging,
                Some((&backing, &header)),
                size,
                size.saturating_add(GIB),
            )
            .map_err(|_| PATH),
            _ => Err(PATH),
        };
        let permit = plan.and_then(|plan| {
            let binding = prepared.workspace().binding();
            let permit = (self.mint)(binding, prepared.launch_target(&plan, self.cwd))?;
            Ok((plan, permit))
        });
        match permit {
            Ok((plan, permit)) => {
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

/// The golden VARS, copied into the identified clone slot under a permit
/// consumed at the write, then verified and writer-excluded.
fn copy_vars(
    workspace: &ProductionWorkspace,
    golden: &HeldGolden,
    ledger: &dyn CustodyCheckpoints,
    request: &CloneRequest<'_>,
    authorize: impl FnOnce(ArtifactSlot, ArtifactPin) -> Result<(), Q>,
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
    authorize(slot, content)?;
    let mut writer = workspace.create_slot(slot, ledger)?;
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
/// I18, I24). `settle` receives the verified pair while every hold is
/// live, or the failure after every hold has been dropped. A settlement
/// error is returned without a second settlement; the durable pending
/// receipt stays visible to recovery.
pub(crate) async fn stage_linked_clone(
    hold_golden: impl FnOnce() -> Result<HeldGolden, Q>,
    reserve: impl FnOnce() -> Result<ProductionWorkspace, Q>,
    ledger: &dyn CustodyCheckpoints,
    request: &CloneRequest<'_>,
    helper: impl OverlayHelper,
    authorize_vars: impl FnOnce(ArtifactSlot, ArtifactPin) -> Result<(), Q>,
    settle: impl FnOnce(Result<[ArtifactEvidence; 2], FailureReason>) -> anyhow::Result<Status>,
) -> anyhow::Result<Status> {
    let mut settle = Some(settle);
    let staged = async {
        let slots = request.slots;
        if (slots.disk.role, slots.vars.role) != ("disk", "vars")
            || slots.disk.generation != slots.vars.generation
            || slots.disk.key == slots.vars.key
        {
            return Err(PATH);
        }
        request.check()?;
        let golden = hold_golden()?;
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
            (ArtifactRole::QemuImg, helper.image()),
            (ArtifactRole::Staging, prepared.identity()),
            (ArtifactRole::Golden, golden.identity()),
        ];
        let settlement = helper.create(&prepared, &golden, request.size).await;
        let outcome = prepared.accept(settlement, &inputs, request.cancelled);
        let (disk, _overlay) = verify_overlay(&prepared, &golden, ledger, outcome, request)?;
        let (vars, _vars) = copy_vars(&workspace, &golden, ledger, request, authorize_vars)?;
        request.check()?;
        golden.recheck()?;
        WorkspaceCustody::production(&workspace.root, &workspace.lease)?;
        let settle = settle.take().ok_or(PATH)?;
        Ok(settle(Ok([disk, vars])))
    }
    .await;
    match (staged, settle) {
        (Ok(published), _) => published,
        (Err(_), Some(settle)) => settle(Err(FailureReason::CloneFailed)),
        (Err(_), None) => anyhow::bail!("clone settlement already attempted"),
    }
}

#[cfg(test)]
#[path = "clone_tests.rs"]
mod tests;
