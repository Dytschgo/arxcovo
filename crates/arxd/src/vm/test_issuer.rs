//! The native TEST issuer (ADR 0054, 7c chunk 10).
//!
//! Selection opens one epoch over the exact selected seal, scope, recipe
//! pair and configured owner. An intent follows durable admission: only a
//! fresh admission yields a worker, the controller fixes that worker's
//! admitted request, and each worker mints at most one intent. The admitted
//! request decides which effects the intent may mint, and the issuer derives
//! each effect's identity from its target. Every permit names one effect.
//! Consumption refuses any substitution, then checks cancellation, the ledger
//! and the deadline on both sides of a fresh epoch observation before the
//! effect's own authority type exists. Recovery authority is separate and
//! cannot launch.
//!
//! No production path constructs an [`Installation`] before 7c 16, so no
//! configured input reaches an effect here. Native paths are **unverified on
//! real hardware**.
#![cfg_attr(
    not(test),
    allow(dead_code, reason = "the closed TEST installation lands in 7c 16")
)]

use super::{
    AdmittedEffect, ArtifactSlot, Clock, ClockReading, Id, PairSlots, PendingOperation,
    VmController, WEB_RECIPE_ID,
    paths::{ArtifactPin, ArtifactRole, FileIdentity, LocalPath},
    qemu_args::{CommandPlan, PlanKind},
    qualification::{
        ForegroundQualification, LaunchPermit, LaunchPermitBinding, LaunchTarget, StageTarget,
        WorkspaceBinding, recipe_selected, stage_supported,
    },
    qualification_record::{Class, Hash, RecipeIdentity, TestRecipes, digest},
    runtime::{
        InstallerPhase, LaunchAuthority, NativeRefusal, ScriptAuthority, StageAuthority,
        fresh_observation,
    },
    selection_epoch::{Invalidation, Sample, SelectionEpoch},
};
use crate::{config::TestQualificationSelection, service::Shutdown, store::vm::native::Scope};
use serde_json::{Value, json};
use std::{sync::Mutex, time::Instant};

const REFUSED: NativeRefusal = NativeRefusal::QualificationMissing;
/// One worker's intent names at most this many distinct effects.
const MAX_EFFECTS: usize = 16;

/// Only this module can construct it, so only a consumed permit yields an
/// effect authority.
pub(crate) struct Minted(());

/// The single installation point for native TEST effects and recovery. Its
/// production constructor lands with activation in 7c 16. Until then only
/// fixtures hold one, so no profile, configured selection or filter can open
/// an issuer or mint recovery authority.
pub(crate) struct Installation(());

impl Installation {
    #[cfg(test)]
    pub(crate) fn fixture() -> Self {
        Self(())
    }
}

/// The held, independently measured TEST selection.
pub(crate) trait SelectedSource {
    fn seal_sha256(&self) -> Hash;
    fn class(&self) -> Class;
    fn scope(&self) -> Hash;
    fn recipes(&self) -> TestRecipes;
    /// The configured SID names the enrolled owner whose live token the
    /// source verified.
    fn owner_matches(&self, configured: &str) -> bool;
    /// The freshness and store-identity reads, then `advance`, each keeping
    /// its typed refusal class.
    fn sample(
        &self,
        advance: &dyn Fn() -> Result<ClockReading, Invalidation>,
    ) -> Result<Sample, Invalidation>;
}

/// The generation an admitted worker's effects target. The ledger confirms
/// that the admitted operation owns it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct NativeTarget {
    pub generation: Id,
    /// The unresolved writer epoch of an active generation: the start's own
    /// operation, or the running epoch a script uses. Staging has none.
    pub writer_epoch: Option<Id>,
}

pub(crate) struct TestIssuer<'a, S, C> {
    controller: &'a VmController<C>,
    source: S,
    epoch: SelectionEpoch,
    seal_sha256: Hash,
    recipes: TestRecipes,
}

impl<'a, C: Clock> TestIssuer<'a, ForegroundQualification<'a>, C> {
    /// The startup path: hold the configured selection, then open it.
    pub(crate) fn open_configured(
        installation: &Installation,
        controller: &'a VmController<C>,
        config: &TestQualificationSelection,
    ) -> Result<Self, NativeRefusal> {
        let source = ForegroundQualification::open(
            controller.native_store(),
            config.enrollment().as_str(),
            config.dossier().as_str(),
            config.selection(),
        )?;
        Self::open(installation, controller, source, config)
    }
}

impl<'a, S: SelectedSource, C: Clock> TestIssuer<'a, S, C> {
    /// T01/T02: the held seal, scope, class, recipe pair and owner must match
    /// the configured selection exactly before the first epoch observation.
    pub(crate) fn open(
        _: &Installation,
        controller: &'a VmController<C>,
        source: S,
        config: &TestQualificationSelection,
    ) -> Result<Self, NativeRefusal> {
        let selection = config.selection();
        let recipes = source.recipes();
        let windows = VmController::<C>::windows_recipe_id();
        if source.seal_sha256() != selection.seal_sha256
            || source.scope() != selection.scope
            || source.class() != Class::Test
            || recipes.windows.recipe_id != windows
            || [windows, WEB_RECIPE_ID, [0; 16]].contains(&recipes.ubuntu.recipe_id)
        {
            return Err(REFUSED);
        }
        if !source.owner_matches(config.owner_sid()) {
            return Err(NativeRefusal::IdentityUnverified);
        }
        let sample = source.sample(&|| controller.advance_native_clock())?;
        let id = crate::store::vm::new_id().map_err(|_| REFUSED)?;
        let epoch = SelectionEpoch::open(id, sample, Instant::now())?;
        Ok(Self {
            controller,
            source,
            epoch,
            seal_sha256: selection.seal_sha256,
            recipes,
        })
    }

    pub(crate) fn epoch(&self) -> &SelectionEpoch {
        &self.epoch
    }

    /// One bounded observation. Only confirmed drift revokes the epoch.
    pub(crate) fn refresh(&self) -> Result<Instant, NativeRefusal> {
        self.epoch
            .refresh(
                || {
                    self.source
                        .sample(&|| self.controller.advance_native_clock())
                },
                Instant::now,
            )
            .map_err(NativeRefusal::from)
    }

    /// T05: the selected seal and both recipe pins after a fresh epoch
    /// observation, before any owner-idle wait. Effects check idleness.
    pub(crate) fn catalog(&self) -> Result<Value, NativeRefusal> {
        self.refresh()?;
        Ok(catalog(self.seal_sha256, self.recipes))
    }

    /// I05: an intent follows durable admission. Only a fresh admission
    /// yields a worker with an admitted request, and a worker mints one
    /// intent, so a replay mints nothing.
    pub(crate) fn intent<'i>(
        &'i self,
        worker: &'i PendingOperation,
        target: NativeTarget,
        cancel: &'i Shutdown,
    ) -> Result<Intent<'i, S, C>, NativeRefusal> {
        let admitted = worker.admitted().ok_or(REFUSED)?;
        let consistent = match admitted.effect {
            AdmittedEffect::Start => target.writer_epoch == Some(admitted.operation),
            AdmittedEffect::Script { .. } => target.writer_epoch.is_some(),
            AdmittedEffect::Clone { .. } => target.writer_epoch.is_none(),
            AdmittedEffect::Build { recipe_id, digest } => {
                let recipe = RecipeIdentity {
                    recipe_id,
                    recipe_sha256: digest,
                };
                target.writer_epoch.is_none()
                    && admitted.build.is_some_and(|(slots, _, _)| {
                        [
                            slots.pair.disk.generation,
                            slots.verification.disk.generation,
                        ]
                        .contains(&target.generation)
                    })
                    && recipe_selected(recipe, None, self.recipes)
            }
        };
        if !consistent {
            return Err(REFUSED);
        }
        let intent = Intent {
            issuer: self,
            worker,
            effect: admitted.effect,
            target,
            cancel,
            minted: Mutex::default(),
        };
        intent.verify(&[])?;
        if !worker.claim_intent() {
            return Err(REFUSED);
        }
        Ok(intent)
    }
}

/// The catalog formatter: exactly the selected seal and its two recipes.
pub(crate) fn catalog(seal_sha256: Hash, recipes: TestRecipes) -> Value {
    let recipe = |guest, identity: RecipeIdentity| {
        json!({
            "guest": guest,
            "recipe_id": hex::encode(identity.recipe_id),
            "recipe_sha256": hex::encode(identity.recipe_sha256),
        })
    };
    json!({
        "available": true,
        "selected_test_qualification": {
            "sha256": hex::encode(seal_sha256),
            "recipes": [recipe("ubuntu", recipes.ubuntu), recipe("windows", recipes.windows)],
        }
    })
}

fn distinct_pair(pair: PairSlots) -> bool {
    pair.disk.generation == pair.vars.generation
        && pair.disk.key != pair.vars.key
        && ![pair.disk.key, pair.vars.key].contains(&[0; 16])
}

/// The ledger slots a checked creation touches, derived from its target:
/// the exact pair of a boot (with `verification_overlay` only for a build's
/// verification boot), the identified helper target of a Mutate plan, and
/// none for a read-only or offline inspection.
fn launch_slots(
    effect: AdmittedEffect,
    kind: PlanKind,
    pair: Option<PairSlots>,
    helper: Option<ArtifactSlot>,
) -> Result<Vec<ArtifactSlot>, NativeRefusal> {
    let is_build = matches!(effect, AdmittedEffect::Build { .. });
    match (kind, pair, helper) {
        (PlanKind::Boot, Some(pair), None)
            if pair.vars.role == "vars"
                && (pair.disk.role == "disk"
                    || (is_build && pair.disk.role == "verification_overlay"))
                && distinct_pair(pair) =>
        {
            Ok(vec![pair.disk, pair.vars])
        }
        (PlanKind::InstallerBoot(_), Some(pair), None)
            if (pair.disk.role, pair.vars.role) == ("disk", "vars") && distinct_pair(pair) =>
        {
            Ok(vec![pair.disk, pair.vars])
        }
        (PlanKind::Mutate, None, Some(slot))
            if slot.key != [0; 16]
                && (slot.role == "disk" || (is_build && slot.role == "verification_overlay")) =>
        {
            Ok(vec![slot])
        }
        (PlanKind::Inspect | PlanKind::InspectWritable, None, None) => Ok(Vec::new()),
        _ => Err(REFUSED),
    }
}

/// Installer boots must carry their matching live `InstallerStage`; other
/// plan kinds must carry none.
fn check_installer(
    kind: PlanKind,
    installer: Option<(InstallerPhase, tokio::time::Instant)>,
) -> Result<(), NativeRefusal> {
    match (kind, installer) {
        (PlanKind::InstallerBoot(phase), Some((stage_phase, deadline)))
            if phase == stage_phase && tokio::time::Instant::now() < deadline =>
        {
            Ok(())
        }
        (
            PlanKind::Boot | PlanKind::Inspect | PlanKind::InspectWritable | PlanKind::Mutate,
            None,
        ) => Ok(()),
        _ => Err(REFUSED),
    }
}

/// One durable effect's identity, derived by the issuer from its target.
#[derive(Clone, PartialEq)]
enum Effect {
    Boot(PlanKind, PairSlots),
    Mutate(ArtifactSlot),
    Inspect(PlanKind, Vec<(ArtifactRole, Option<FileIdentity>)>),
    Stage(ArtifactSlot),
    Copy(ArtifactSlot),
    Script,
}

/// One admitted worker's authority to mint permits for its durable effect.
pub(crate) struct Intent<'i, S, C> {
    issuer: &'i TestIssuer<'i, S, C>,
    worker: &'i PendingOperation,
    effect: AdmittedEffect,
    target: NativeTarget,
    cancel: &'i Shutdown,
    minted: Mutex<Vec<Effect>>,
}

impl<'i, S: SelectedSource, C: Clock> Intent<'i, S, C> {
    /// Cancellation, then the durable admission, deadline, target and slots.
    fn guard(&self, slots: &[ArtifactSlot]) -> Result<(), NativeRefusal> {
        let target = match slots.first() {
            Some(slot) if matches!(self.effect, AdmittedEffect::Build { .. }) => NativeTarget {
                generation: slot.generation,
                writer_epoch: None,
            },
            _ => self.target,
        };
        if self.cancel.is_cancelled()
            || !self
                .issuer
                .controller
                .native_effect_admitted(self.worker, target, slots)
                .map_err(|_| REFUSED)?
        {
            return Err(REFUSED);
        }
        Ok(())
    }

    /// The guard on both sides of a fresh epoch observation, so a refresh
    /// that outlives cancellation, the deadline or the pending row refuses,
    /// and the authority is issued within that observation's freshness.
    fn verify(&self, slots: &[ArtifactSlot]) -> Result<(), NativeRefusal> {
        self.guard(slots)?;
        let captured = self.issuer.refresh()?;
        self.guard(slots)?;
        fresh_observation(captured, Instant::now(), REFUSED)
    }

    /// One permit per durable effect.
    fn mint(&self, effect: Effect, slots: &[ArtifactSlot]) -> Result<(), NativeRefusal> {
        self.verify(slots)?;
        let mut minted = self.minted.lock().map_err(|_| REFUSED)?;
        if minted.len() >= MAX_EFFECTS || minted.contains(&effect) {
            return Err(REFUSED);
        }
        minted.push(effect);
        Ok(())
    }

    pub(crate) fn build_recipe(&self) -> Option<RecipeIdentity> {
        match self.effect {
            AdmittedEffect::Build { recipe_id, digest } => Some(RecipeIdentity {
                recipe_id,
                recipe_sha256: digest,
            }),
            _ => None,
        }
    }

    /// The admitted request authorizes this kind of creation with this
    /// recipe: a start boots its pair, a build boots and runs installers for
    /// its own recipe, and builds and clones run the checked helper.
    fn admits_launch(&self, kind: PlanKind, recipe: Option<RecipeIdentity>) -> bool {
        let recipes = self.issuer.recipes;
        match (self.effect, kind) {
            (AdmittedEffect::Start, PlanKind::Boot) => {
                recipe.is_some_and(|recipe| recipe_selected(recipe, None, recipes))
            }
            (AdmittedEffect::Build { .. }, PlanKind::Boot) => {
                recipe.is_some() && recipe == self.build_recipe()
            }
            (AdmittedEffect::Build { .. }, PlanKind::InstallerBoot(phase)) => {
                recipe.is_some_and(|recipe| {
                    Some(recipe) == self.build_recipe()
                        && recipe_selected(recipe, Some(phase), recipes)
                })
            }
            (
                AdmittedEffect::Build { .. } | AdmittedEffect::Clone { .. },
                PlanKind::Inspect | PlanKind::InspectWritable | PlanKind::Mutate,
            ) => recipe.is_none(),
            _ => false,
        }
    }

    /// One checked creation for exactly this plan. The effect identity and
    /// its ledger slots come from the target, never from the caller.
    pub(crate) fn launch(
        &self,
        workspace: WorkspaceBinding,
        target: LaunchTarget<'_, '_>,
    ) -> Result<Permit<'_, S, C>, NativeRefusal> {
        let expected = EffectPlan::from_target(&target)?;
        let binding = target.binding(workspace, self.issuer.seal_sha256)?;
        let slots = launch_slots(self.effect, expected.kind, target.pair, target.helper)?;
        check_installer(expected.kind, expected.installer)?;
        if let Some((build, _, _)) = self.worker.admitted().and_then(|admitted| admitted.build) {
            let exact = match (expected.kind, target.pair, target.helper) {
                (PlanKind::Boot, Some(pair), _) => {
                    pair == build.verification
                        || (pair == build.pair
                            && self.build_recipe() == Some(self.issuer.recipes.windows))
                }
                (PlanKind::InstallerBoot(_), Some(pair), _) => pair == build.pair,
                (PlanKind::Mutate, _, Some(slot)) => {
                    slot == build.pair.disk || slot == build.verification.disk
                }
                (PlanKind::Inspect | PlanKind::InspectWritable, None, None) => true,
                _ => false,
            };
            if !exact {
                return Err(REFUSED);
            }
        }
        let verification = matches!(self.effect, AdmittedEffect::Build { .. })
            && slots.first().is_some_and(|slot| {
                slot.role == "verification_overlay" && slot.generation == workspace.generation
            });
        let expected_generation = if verification {
            workspace.generation
        } else {
            self.target.generation
        };
        let expected_epoch = self.target.writer_epoch.unwrap_or(expected_generation);
        if workspace.generation != expected_generation
            || slots
                .iter()
                .any(|slot| slot.generation != workspace.generation)
            || expected.writer_epoch != expected_epoch
            || !self.admits_launch(expected.kind, target.recipe)
        {
            return Err(REFUSED);
        }
        let effect = match (expected.kind, target.pair, target.helper) {
            (PlanKind::Boot | PlanKind::InstallerBoot(_), Some(pair), None) => {
                Effect::Boot(expected.kind, pair)
            }
            (PlanKind::Mutate, None, Some(slot)) => Effect::Mutate(slot),
            (PlanKind::Inspect | PlanKind::InspectWritable, None, None) => Effect::Inspect(
                expected.kind,
                expected
                    .inputs
                    .iter()
                    .map(|input| (input.role, input.identity))
                    .collect(),
            ),
            _ => return Err(REFUSED),
        };
        self.mint(effect, &slots)?;
        Ok(Permit {
            intent: self,
            binding,
            expected,
            slots,
        })
    }

    fn check_stage(&self, target: &StageTarget) -> Result<(), NativeRefusal> {
        if target.workspace.generation != self.target.generation
            || target.slot.generation != self.target.generation
            || target.seal_sha256 != self.issuer.seal_sha256
            || !stage_supported(target.slot, target.phase)
            || !self
                .worker
                .admitted()
                .and_then(|admitted| admitted.build)
                .is_some_and(|(slots, _, _)| {
                    target.slot == slots.seed || target.slot == slots.pair.vars
                })
            || self.build_recipe() != Some(target.recipe)
            || !recipe_selected(target.recipe, Some(target.phase), self.issuer.recipes)
            || Instant::now() >= target.deadline
        {
            return Err(REFUSED);
        }
        Ok(())
    }

    /// One staged write of exactly this content into this slot, for the
    /// admitted build's recipe and phase. Only the staging slots (seed and
    /// VARS) in a staging phase qualify; the guest disk and Windows
    /// finalization never do.
    pub(crate) fn stage(
        &self,
        target: StageTarget,
    ) -> Result<StagePermit<'_, S, C>, NativeRefusal> {
        self.check_stage(&target)?;
        self.mint(Effect::Stage(target.slot), &[target.slot])?;
        Ok(StagePermit {
            intent: self,
            target,
        })
    }

    /// One copy of exactly this content into the admitted clone's own VARS
    /// slot (7c 11).
    pub(crate) fn copy(
        &self,
        slot: ArtifactSlot,
        content: ArtifactPin,
    ) -> Result<CopyPermit<'_, S, C>, NativeRefusal> {
        if !matches!(self.effect, AdmittedEffect::Clone { .. })
            || self.target.writer_epoch.is_some()
            || slot.role != "vars"
            || slot.key == [0; 16]
            || slot.generation != self.target.generation
        {
            return Err(REFUSED);
        }
        self.mint(Effect::Copy(slot), &[slot])?;
        Ok(CopyPermit {
            intent: self,
            slot,
            content,
            verification: None,
        })
    }

    /// One detached build verification VARS copy, bound to both reserved slots.
    pub(crate) fn copy_verification(
        &self,
        pair: PairSlots,
        content: ArtifactPin,
    ) -> Result<CopyPermit<'_, S, C>, NativeRefusal> {
        if !self
            .worker
            .admitted()
            .and_then(|admitted| admitted.build)
            .is_some_and(|(slots, _, _)| pair == slots.verification)
            || !distinct_pair(pair)
            || (pair.disk.role, pair.vars.role) != ("verification_overlay", "vars")
        {
            return Err(REFUSED);
        }
        self.mint(Effect::Copy(pair.vars), &[pair.disk, pair.vars])?;
        Ok(CopyPermit {
            intent: self,
            slot: pair.vars,
            content,
            verification: Some(pair),
        })
    }

    /// One guest script, only with the exact bytes and timeout the admitted
    /// request carried, on the live writer epoch.
    pub(crate) fn script(
        &self,
        script: &str,
        timeout_s: u32,
    ) -> Result<ScriptPermit<'_, S, C>, NativeRefusal> {
        let AdmittedEffect::Script {
            digest: admitted,
            timeout_s: admitted_timeout,
        } = self.effect
        else {
            return Err(REFUSED);
        };
        let script = digest(script.as_bytes());
        let writer_epoch = self.target.writer_epoch.ok_or(REFUSED)?;
        if script != admitted || u64::from(timeout_s) != admitted_timeout {
            return Err(REFUSED);
        }
        self.mint(Effect::Script, &[])?;
        Ok(ScriptPermit {
            intent: self,
            generation: self.target.generation,
            writer_epoch,
            script,
            timeout_s,
        })
    }
}

#[must_use]
pub(crate) struct Permit<'p, S, C> {
    intent: &'p Intent<'p, S, C>,
    binding: LaunchPermitBinding,
    expected: EffectPlan,
    slots: Vec<ArtifactSlot>,
}

impl<S: SelectedSource, C: Clock> LaunchPermit for Permit<'_, S, C> {
    fn consume(self, target: LaunchTarget<'_, '_>) -> Result<LaunchAuthority, NativeRefusal> {
        if self.expected != EffectPlan::from_target(&target)? {
            return Err(REFUSED);
        }
        check_installer(self.expected.kind, self.expected.installer)?;
        self.intent.verify(&self.slots)?;
        check_installer(self.expected.kind, self.expected.installer)?;
        Ok(LaunchAuthority::new(Minted(()), self.binding))
    }
}

#[must_use]
pub(crate) struct StagePermit<'p, S, C> {
    intent: &'p Intent<'p, S, C>,
    target: StageTarget,
}

impl<S: SelectedSource, C: Clock> StagePermit<'_, S, C> {
    /// At the staged write: the exact target, still in phase, still fresh.
    pub(crate) fn consume(self, target: StageTarget) -> Result<StageAuthority, NativeRefusal> {
        if self.target != target {
            return Err(REFUSED);
        }
        self.intent.check_stage(&target)?;
        self.intent.verify(&[target.slot])?;
        self.intent.check_stage(&target)?;
        Ok(StageAuthority::new(Minted(()), target))
    }
}

#[must_use]
pub(crate) struct CopyPermit<'p, S, C> {
    intent: &'p Intent<'p, S, C>,
    slot: ArtifactSlot,
    content: ArtifactPin,
    verification: Option<PairSlots>,
}

impl<S: SelectedSource, C: Clock> CopyPermit<'_, S, C> {
    /// At the write: the exact slot and content, still admitted and fresh.
    pub(crate) fn consume(
        self,
        slot: ArtifactSlot,
        content: ArtifactPin,
    ) -> Result<CopyAuthority, NativeRefusal> {
        if (self.slot, self.content) != (slot, content) {
            return Err(REFUSED);
        }
        match self.verification {
            Some(pair) => self.intent.verify(&[pair.disk, slot])?,
            None => self.intent.verify(&[slot])?,
        }
        Ok(CopyAuthority {
            _minted: Minted(()),
            slot,
            content,
        })
    }
}

/// Authority for one clone VARS copy; only a consumed permit yields one.
#[must_use]
pub(crate) struct CopyAuthority {
    _minted: Minted,
    slot: ArtifactSlot,
    content: ArtifactPin,
}

impl CopyAuthority {
    pub(crate) fn names(&self, slot: ArtifactSlot, content: ArtifactPin) -> bool {
        (self.slot, self.content) == (slot, content)
    }
}

#[must_use]
pub(crate) struct ScriptPermit<'p, S, C> {
    intent: &'p Intent<'p, S, C>,
    generation: Id,
    writer_epoch: Id,
    script: Hash,
    timeout_s: u32,
}

impl<S: SelectedSource, C: Clock> ScriptPermit<'_, S, C> {
    /// At channel dispatch: the live generation and writer epoch, and the
    /// exact script bytes and timeout the permit was minted for.
    pub(crate) fn consume(
        self,
        generation: Id,
        writer_epoch: Id,
        script: &str,
        timeout_s: u32,
    ) -> Result<ScriptAuthority, NativeRefusal> {
        if (self.generation, self.writer_epoch, self.timeout_s)
            != (generation, writer_epoch, timeout_s)
            || self.script != digest(script.as_bytes())
        {
            return Err(REFUSED);
        }
        self.intent.verify(&[])?;
        Ok(ScriptAuthority::new(
            Minted(()),
            self.script,
            timeout_s,
            generation,
            writer_epoch,
        ))
    }
}

/// Settlement authority over exact durable TEST custody. It needs no fresh
/// selection, so a revoked or absent epoch cannot strand recovery, and no
/// path converts it into an effect authority.
#[must_use]
pub(crate) struct RecoveryAuthority {
    scope: WorkspaceBinding,
    writer_epoch: Option<Id>,
}

impl RecoveryAuthority {
    pub(crate) fn mint<C: Clock>(
        _: &Installation,
        controller: &VmController<C>,
        scope: WorkspaceBinding,
        writer_epoch: Option<Id>,
    ) -> Result<Self, NativeRefusal> {
        let durable = Scope {
            generation: scope.generation,
            production: scope.production,
            workspace: scope.workspace,
            lease: scope.lease,
        };
        if !controller
            .native_recovery_bound(durable, writer_epoch)
            .map_err(|_| NativeRefusal::PathCustodyUnverified)?
        {
            return Err(NativeRefusal::PathCustodyUnverified);
        }
        Ok(Self {
            scope,
            writer_epoch,
        })
    }

    pub(crate) fn scope(&self) -> WorkspaceBinding {
        self.scope
    }

    pub(crate) fn writer_epoch(&self) -> Option<Id> {
        self.writer_epoch
    }
}

#[derive(Eq, PartialEq)]
struct EffectInput {
    role: ArtifactRole,
    path: LocalPath,
    identity: Option<FileIdentity>,
    pin: Option<ArtifactPin>,
}

/// Everything a checked creation consumes, compared whole at consumption.
#[derive(Eq, PartialEq)]
struct EffectPlan {
    kind: PlanKind,
    writer_epoch: Id,
    image: LocalPath,
    argv: Vec<String>,
    inputs: Vec<EffectInput>,
    cwd: LocalPath,
    pair: Option<PairSlots>,
    helper: Option<ArtifactSlot>,
    installer: Option<(InstallerPhase, tokio::time::Instant)>,
    recipe: Option<RecipeIdentity>,
    memory: usize,
    cpu_rate: u32,
}

impl EffectPlan {
    fn from_target(target: &LaunchTarget<'_, '_>) -> Result<Self, NativeRefusal> {
        let plan: &CommandPlan<'_> = target.plan;
        let inputs = plan
            .inputs()
            .iter()
            .map(|(role, input)| {
                Ok(EffectInput {
                    role: *role,
                    path: input.path_for(*role).map_err(|_| REFUSED)?.clone(),
                    identity: input.identity(),
                    pin: input.pin(),
                })
            })
            .collect::<Result<_, NativeRefusal>>()?;
        Ok(Self {
            kind: plan.kind(),
            writer_epoch: target.writer_epoch,
            image: plan.image().clone(),
            argv: plan.args().to_vec(),
            inputs,
            cwd: target.cwd.clone(),
            pair: target.pair,
            helper: target.helper,
            installer: target
                .installer
                .map(|stage| (stage.phase(), stage.deadline())),
            recipe: target.recipe,
            memory: target.memory,
            cpu_rate: target.cpu_rate,
        })
    }
}

#[cfg(test)]
pub(crate) mod tests;
