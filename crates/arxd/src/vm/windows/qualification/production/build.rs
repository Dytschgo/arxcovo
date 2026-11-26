//! Closed TEST build composition. The primary release is kept through the
//! atomic seal. Transient media is retired through durable checked disposal.
//! Native launches are **unverified on real hardware**.
#![cfg_attr(
    not(test),
    allow(dead_code, reason = "the closed TEST installation lands in 7c 16")
)]

use super::super::recipe::{InstallerReservation, StagedInstaller};
use super::helper::{HelperOutcome, PreparedHelper};
use super::*;
use crate::store::vm::native;
use crate::vm::paths::Artifact;
use crate::vm::qualification_record::RecipeIdentity;
use crate::vm::runtime::{InstallerBudget, InstallerPhase, InstallerRecipe, fresh_observation};
use crate::vm::test_issuer::CopyAuthority;
use crate::vm::{
    BuildRequest, BuildSlots, Clock, FailureReason, PendingOperation, Status, VmController,
};
use std::cell::Cell;

type Q = NativeRefusal;
const PATH: Q = NativeRefusal::PathCustodyUnverified;
const PHASE: Q = NativeRefusal::InstallerPhaseMismatch;
const HEALTH: Q = NativeRefusal::GuestEndpointUnverified;
const SCRIPT_TIMEOUT: u32 = 30;

/// Only the completed build sequence can produce a seal. The release owns
/// both primary writer exclusions and borrows the still-held workspace.
pub(crate) struct BuildSeal<'a> {
    release: &'a ReleasedGeneration<'a>,
    pair: StartPair,
}

impl BuildSeal<'_> {
    pub(crate) fn evidence(
        &self,
        pair: PairSlots,
    ) -> anyhow::Result<(native::Scope, [ArtifactEvidence; 2])> {
        let binding = self.release.binding;
        // Both writer exclusions are rechecked through their live handles
        // inside the seal transaction.
        let held =
            self.pair.evidence.iter().all(|file| {
                self.release._held.1.iter().any(|guard| {
                    file_identity(guard, false).is_ok_and(|(id, _)| id == file.identity)
                })
            });
        anyhow::ensure!(
            self.pair.slots == pair
                && self.release.matches(
                    binding,
                    pair,
                    self.pair.evidence.map(|file| file.identity)
                )
                && self.release.stop_disposition == Some(StopDisposition::CleanRelease)
                && self.release._held.1.len() == 2
                && held
                && self.release._workspace.verify().is_ok(),
            "invalid held native build seal"
        );
        Ok((custody::scope(binding), self.pair.evidence))
    }
}

impl ReleasedGeneration<'_> {
    /// Keep the release's owned pins and exclusions, borrowing the workspace
    /// directly so the launch plan can go out of scope.
    fn for_build<'w>(
        self,
        workspace: &'w ProductionWorkspace,
        pair: StartPair,
    ) -> Result<ReleasedGeneration<'w>, Q> {
        if !self.matches(
            workspace.binding(),
            pair.slots,
            pair.evidence.map(|file| file.identity),
        ) || self.stop_disposition != Some(StopDisposition::CleanRelease)
        {
            return Err(PATH);
        }
        Ok(ReleasedGeneration {
            binding: self.binding,
            targets: self.targets,
            stop_disposition: self.stop_disposition,
            _workspace: WorkspaceCustody::production(&workspace.root, &workspace.lease)?,
            _held: (
                vec![
                    workspace.pin_slot(pair.slots.disk, pair.evidence[0].identity, false)?,
                    workspace.pin_slot(pair.slots.vars, pair.evidence[1].identity, false)?,
                ]
                .into(),
                self._held.1,
            ),
        })
    }

    fn capture_build_pair(
        &self,
        workspace: &ProductionWorkspace,
        mut pair: StartPair,
        limit: u64,
        deadline: Instant,
        cancel: &dyn Fn() -> bool,
    ) -> Result<StartPair, Q> {
        if !self.matches(
            workspace.binding(),
            pair.slots,
            pair.evidence.map(|file| file.identity),
        ) || self.stop_disposition != Some(StopDisposition::CleanRelease)
        {
            return Err(PATH);
        }
        self._workspace.verify()?;
        let mut remaining = limit;
        for ((slot, evidence), max) in [pair.slots.disk, pair.slots.vars]
            .into_iter()
            .zip(&mut pair.evidence)
            .zip(START_PAIR_LIMITS)
        {
            let _named = workspace.pin_slot(slot, evidence.identity, false)?;
            let guard = self
                ._held
                .1
                .iter()
                .find(|guard| {
                    file_identity(guard, false).is_ok_and(|(id, _)| id == evidence.identity)
                })
                .ok_or(PATH)?;
            let pin = launch::hash_held(guard, max.min(remaining), deadline, cancel)?;
            remaining = remaining.checked_sub(pin.size).ok_or(PATH)?;
            evidence.pin = pin;
        }
        Ok(pair)
    }
}

#[derive(Clone, Copy)]
enum Phase {
    Install(InstallerStage),
    Health { verification: bool },
}

struct Boot<'a> {
    workspace: &'a ProductionWorkspace,
    pair: StartPair,
    recipe: RecipeIdentity,
    phase: Phase,
    staged: Option<&'a StagedInstaller<'a>>,
    backing: Option<&'a Artifact>,
    cutoff: Instant,
}

enum Finalized {
    Native(FinalizedPair),
    #[cfg(test)]
    Fixture(bool),
}

impl Finalized {
    fn check(&self, pair: StartPair, deadline: Instant) -> Result<(), Q> {
        let generalized = match self {
            Self::Native(finalized) => {
                finalized.observe(pair.slots, pair.evidence, deadline)? == (true, true)
            }
            #[cfg(test)]
            Self::Fixture(generalized) => *generalized,
        };
        generalized.then_some(()).ok_or(PHASE)
    }
}

struct BootRelease<'w> {
    release: ReleasedGeneration<'w>,
    phase: Option<InstallerPhase>,
    finalized: Option<Finalized>,
}

trait Effects {
    fn fresh(&self) -> Result<Instant, Q>;
    fn stage<'w>(
        &self,
        reservation: InstallerReservation<'w>,
        ledger: &dyn CustodyCheckpoints,
    ) -> Result<StagedInstaller<'w>, Q>;
    fn image(&self) -> Result<Artifact, Q>;
    #[cfg(test)]
    fn inside_seal(&self) {}
    async fn helper<'w>(
        &'w self,
        prepared: &'w PreparedHelper<'w, '_>,
        image: &'w Artifact,
        backing: Option<(&Artifact, &[u8])>,
        size: u64,
        ceiling: u64,
    ) -> HelperSettlement<'w>;
    /// A consumed copy permit for the verification VARS. Only the issuer can
    /// mint the returned authority.
    fn authorize_copy(
        &self,
        pair: PairSlots,
        content: crate::vm::paths::ArtifactPin,
    ) -> Result<CopyAuthority, Q>;
    async fn boot<'w>(
        &self,
        request: Boot<'_>,
        workspace: &'w ProductionWorkspace,
    ) -> Result<BootRelease<'w>, Q>;
}

struct Build<'a, C> {
    controller: &'a VmController<C>,
    worker: &'a PendingOperation,
    request: BuildRequest<'a>,
    slots: BuildSlots,
    recipe: RecipeIdentity,
    ledger: &'a dyn CustodyCheckpoints,
    disposal: NativeDisposal<'a>,
    cancel: &'a Shutdown,
    size: u64,
    owner: PSID,
}

impl<C: Clock> Build<'_, C> {
    fn check(&self, cutoff: Instant) -> Result<(), Q> {
        launch::check_preparation(cutoff, &|| self.cancel.is_cancelled())
    }

    async fn boot<'w>(
        &self,
        effects: &impl Effects,
        workspace: &'w ProductionWorkspace,
        request: Boot<'_>,
    ) -> Result<BootRelease<'w>, Q> {
        let expected = match request.phase {
            Phase::Install(stage) => Some(stage.phase()),
            Phase::Health { .. } => None,
        };
        let pair = request.pair;
        let cutoff = request.cutoff;
        self.check(cutoff)?;
        self.ledger.confirm_start_pair(workspace.binding(), pair)?;
        let result = effects.boot(request, workspace).await?;
        self.check(cutoff)?;
        if result.phase != expected
            || result.finalized.is_some() != (expected == Some(InstallerPhase::WindowsFinalize))
            || !result.release.matches(
                workspace.binding(),
                pair.slots,
                pair.evidence.map(|file| file.identity),
            )
            || result.release.stop_disposition != Some(StopDisposition::CleanRelease)
        {
            return Err(PHASE);
        }
        Ok(result)
    }

    async fn make_disk(
        &self,
        effects: &impl Effects,
        workspace: &ProductionWorkspace,
        slot: ArtifactSlot,
        backing: Option<(&Artifact, &[u8])>,
        cutoff: Instant,
        ceiling: u64,
    ) -> Result<ArtifactEvidence, Q> {
        let cancelled = || self.cancel.is_cancelled();
        let image = effects.image()?;
        let prepared = workspace.prepare_helper(
            slot,
            self.ledger,
            self.worker.deadline(),
            cutoff,
            &cancelled,
        )?;
        let mut inputs = vec![
            (ArtifactRole::QemuImg, image.identity().ok_or(PATH)?),
            (ArtifactRole::Staging, prepared.identity()),
        ];
        let backing_path = if let Some((golden, _)) = backing {
            inputs.push((ArtifactRole::Golden, golden.identity().ok_or(PATH)?));
            Some(golden.path_for(ArtifactRole::Golden).map_err(|_| PATH)?)
        } else {
            None
        };
        let settlement = effects
            .helper(&prepared, &image, backing, self.size, ceiling)
            .await;
        let _held = match prepared.accept(settlement, &inputs, &cancelled) {
            HelperOutcome::Accepted { proof, .. } => proof,
            HelperOutcome::Refused { reason, .. } | HelperOutcome::Uncertain(reason) => {
                return Err(reason);
            }
        };
        let pin = workspace.pin_slot(slot, prepared.identity(), false)?;
        if crate::vm::qemu_img::inspect_header(&launch::header(&pin)?, backing_path)
            != Ok(self.size)
            || !sink::held_private(&pin.leaf, self.owner)
        {
            return Err(PATH);
        }
        // The overlay stays identified while guests write to it. Its digest
        // is committed once by `capture` after its final writable release.
        Ok(ArtifactEvidence {
            identity: pin.identity,
            directory: workspace.binding().workspace.file,
            pin: launch::hash_held(&pin.leaf, ceiling, prepared.deadline(), &cancelled)?,
        })
    }

    /// Hash the released, writer-excluded pair and commit its single durable
    /// digest after every check on the observed bytes has passed.
    #[allow(clippy::too_many_arguments, reason = "one released pair's checks")]
    fn capture(
        &self,
        workspace: &ProductionWorkspace,
        release: &ReleasedGeneration<'_>,
        pair: StartPair,
        backing: Option<&LocalPath>,
        limit: u64,
        finalized: Option<&Finalized>,
        cutoff: Instant,
    ) -> Result<StartPair, Q> {
        let pair = release.capture_build_pair(workspace, pair, limit, cutoff, &|| {
            self.cancel.is_cancelled()
        })?;
        let disk = workspace.pin_slot(pair.slots.disk, pair.evidence[0].identity, false)?;
        let vars = workspace.pin_slot(pair.slots.vars, pair.evidence[1].identity, false)?;
        if crate::vm::qemu_img::inspect_header(&launch::header(&disk)?, backing) != Ok(self.size)
            || !sink::held_private(&disk.leaf, self.owner)
            || !sink::held_private(&vars.leaf, self.owner)
        {
            return Err(PATH);
        }
        if let Some(finalized) = finalized {
            finalized.check(pair, cutoff)?;
        }
        self.check(cutoff)?;
        self.controller
            .native_store()
            .transact(|db| {
                for (slot, evidence) in [pair.slots.disk, pair.slots.vars]
                    .into_iter()
                    .zip(pair.evidence)
                {
                    native::verify_build_slot(
                        db,
                        custody::scope(workspace.binding()),
                        slot,
                        evidence,
                    )?;
                }
                Ok(())
            })
            .map_err(|_| PATH)?;
        Ok(pair)
    }

    /// Runs the admitted build. Every hold the build takes, including the
    /// primary lease and both released primary writer exclusions, has ended
    /// before a failure is settled.
    async fn run(
        &self,
        effects: &impl Effects,
        primary: ProductionWorkspace,
        reserve_verification: impl FnOnce() -> Result<ProductionWorkspace, Q>,
    ) -> anyhow::Result<Status> {
        anyhow::ensure!(
            self.worker.admitted().is_some_and(|admitted| admitted.build
                == Some((self.slots, self.request.retained, self.request.transient))
                && admitted.principal == self.request.principal
                && admitted.request == self.request.key)
                && self.request.recipe == hex::encode(self.recipe.recipe_id)
                && self.request.digest == self.recipe.recipe_sha256,
            "invalid native build request"
        );
        let stage = Cell::new(FailureReason::StagingFailed);
        let result = self
            .drive(effects, &primary, reserve_verification, &stage)
            .await;
        drop(primary);
        match result {
            Ok(status) => Ok(status),
            Err(error) => {
                self.controller.fail_build_with(
                    self.worker,
                    self.slots,
                    false,
                    self.cause(stage.get()),
                )?;
                Err(error)
            }
        }
    }

    /// Cancellation and the worker deadline have no proven guest-side cause.
    fn cause(&self, stage: FailureReason) -> Option<FailureReason> {
        (!self.cancel.is_cancelled() && Instant::now() < self.worker.deadline()).then_some(stage)
    }

    /// All holds taken here, including the staged installer, each boot's
    /// release and the verification workspace, end when this returns.
    async fn drive(
        &self,
        effects: &impl Effects,
        primary: &ProductionWorkspace,
        reserve_verification: impl FnOnce() -> Result<ProductionWorkspace, Q>,
        stage: &Cell<FailureReason>,
    ) -> anyhow::Result<Status> {
        let run = |error: Q| anyhow::anyhow!("TEST build refused: {error:?}");
        let slots = self.slots;
        let deadline = self.worker.deadline();
        let windows = self.request.recipe == hex::encode(VmController::<C>::windows_recipe_id());
        let (recipe, install) = if windows {
            (InstallerRecipe::Windows, InstallerPhase::WindowsSetup)
        } else {
            (InstallerRecipe::Ubuntu, InstallerPhase::UbuntuInstall)
        };
        self.check(deadline).map_err(run)?;
        let staged = effects
            .stage(
                InstallerReservation {
                    workspace: primary,
                    recipe: self.recipe,
                    phase: install,
                    pair: slots.pair,
                    seed: slots.seed,
                    deadline,
                },
                self.ledger,
            )
            .map_err(run)?;
        let [(seed, seed_evidence), (_, vars)] = staged.artifacts();
        self.controller
            .build_evidence(self.worker, slots.id, &[(seed, Some(seed_evidence))])?;
        let disk_max = self
            .request
            .retained
            .images
            .checked_sub(vars.pin.size)
            .ok_or_else(|| run(PATH))?;
        let disk = self
            .make_disk(effects, primary, slots.pair.disk, None, deadline, disk_max)
            .await
            .map_err(run)?;
        let original = StartPair {
            slots: slots.pair,
            evidence: [disk, vars],
        };
        let budget = InstallerBudget::start(recipe);
        let cutoff = budget
            .clip(deadline.saturating_duration_since(Instant::now()))
            .into_std()
            .min(deadline);
        let installer = |phase| {
            budget
                .stage(phase, cutoff.saturating_duration_since(Instant::now()))
                .ok_or(PHASE)
        };
        let start = |phase| Boot {
            workspace: primary,
            pair: original,
            recipe: self.recipe,
            phase,
            staged: None,
            backing: None,
            cutoff,
        };
        stage.set(FailureReason::InstallFailed);
        let installed = self
            .boot(
                effects,
                primary,
                Boot {
                    staged: Some(&staged),
                    ..start(Phase::Install(installer(install).map_err(run)?))
                },
            )
            .await
            .map_err(run)?;
        drop(staged);
        let released = if windows {
            drop(installed);
            stage.set(FailureReason::HealthFailed);
            self.boot(
                effects,
                primary,
                start(Phase::Health {
                    verification: false,
                }),
            )
            .await
            .map_err(run)?;
            stage.set(FailureReason::InstallFailed);
            self.boot(
                effects,
                primary,
                start(Phase::Install(
                    installer(InstallerPhase::WindowsFinalize).map_err(run)?,
                )),
            )
            .await
            .map_err(run)?
        } else {
            installed
        };
        // The primary pair's last writable phase has released. Its single
        // durable digest is committed only after generalization is observed.
        stage.set(FailureReason::SealFailed);
        let golden = self
            .capture(
                primary,
                &released.release,
                original,
                None,
                self.request.retained.images,
                released.finalized.as_ref(),
                cutoff,
            )
            .map_err(run)?;
        self.verify(
            effects,
            reserve_verification().map_err(run)?,
            primary,
            golden,
            cutoff,
            stage,
        )
        .await
        .map_err(run)?;
        stage.set(FailureReason::SealFailed);
        let seal = BuildSeal {
            release: &released.release,
            pair: golden,
        };
        // A cancelled or expired build leaves its transient media to the
        // ordinary sweep instead of starting new disposal work.
        self.check(cutoff).map_err(run)?;
        self.disposal
            .dispose_build_transient_held(self.worker.activity_id(), slots, cutoff, primary)
            .map_err(run)?;
        self.check(cutoff).map_err(run)?;
        let captured = effects.fresh().map_err(run)?;
        fresh_observation(captured, Instant::now(), NativeRefusal::ReservesUnverified)
            .map_err(run)?;
        self.check(cutoff).map_err(run)?;
        self.controller
            .finish_native_build(self.worker, slots, self.request, &seal, || {
                #[cfg(test)]
                effects.inside_seal();
                self.check(cutoff).is_ok()
            })
    }

    async fn verify(
        &self,
        effects: &impl Effects,
        workspace: ProductionWorkspace,
        primary: &ProductionWorkspace,
        golden: StartPair,
        cutoff: Instant,
        stage: &Cell<FailureReason>,
    ) -> Result<(), Q> {
        stage.set(FailureReason::StagingFailed);
        let pair = self.slots.verification;
        let [disk, vars] = golden.evidence;
        let disk = primary.pin_slot(golden.slots.disk, disk.identity, false)?;
        let vars = primary.pin_slot(golden.slots.vars, vars.identity, false)?;
        let backing =
            launch::artifact(ArtifactRole::Golden, &disk)?.with_pin(golden.evidence[0].pin);
        let bytes = launch::held_bytes(&vars, MAX_SETUP_VARS_BYTES)?;
        let content = golden.evidence[1].pin;
        if bytes.len() as u64 != content.size
            || crate::vm::qualification_record::digest(&bytes) != content.sha256
        {
            return Err(PATH);
        }
        let ceiling = self
            .request
            .transient
            .images
            .checked_sub(content.size)
            .ok_or(PATH)?;
        let disk = self
            .make_disk(
                effects,
                &workspace,
                pair.disk,
                Some((&backing, &launch::header(&disk)?)),
                cutoff,
                ceiling,
            )
            .await?;
        // The consumed permit names this exact slot and content and is held
        // through the write.
        let authority = effects.authorize_copy(pair, content)?;
        if !authority.names(pair.vars, content) {
            return Err(PATH);
        }
        self.check(cutoff)?;
        let mut writer = workspace.create_slot(pair.vars, self.ledger)?;
        for chunk in bytes.chunks(256 << 10) {
            self.check(cutoff)?;
            writer.write_all(chunk)?;
        }
        self.check(cutoff)?;
        writer.sync()?;
        self.check(cutoff)?;
        let mut pin = writer.into_pin(false)?;
        pin.verify_content(content, MAX_SETUP_VARS_BYTES)?;
        if !sink::held_private(&pin.leaf, self.owner) {
            return Err(PATH);
        }
        drop(authority);
        // The copy stays identified while the detached boot may write it.
        let vars = ArtifactEvidence {
            identity: pin.identity,
            directory: workspace.binding().workspace.file,
            pin: content,
        };
        drop(pin);
        let pair = StartPair {
            slots: pair,
            evidence: [disk, vars],
        };
        stage.set(FailureReason::HealthFailed);
        let verified = self
            .boot(
                effects,
                &workspace,
                Boot {
                    workspace: &workspace,
                    pair,
                    recipe: self.recipe,
                    phase: Phase::Health { verification: true },
                    staged: None,
                    backing: Some(&backing),
                    cutoff,
                },
            )
            .await?;
        self.capture(
            &workspace,
            &verified.release,
            pair,
            Some(backing.path_for(ArtifactRole::Golden).map_err(|_| PATH)?),
            self.request.transient.images,
            None,
            cutoff,
        )?;
        Ok(())
    }
}

mod checked;

#[cfg(test)]
#[path = "build_tests.rs"]
mod tests;
