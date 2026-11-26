//! The native TEST issuer (ADR 0054, 7c chunk 10).
//!
//! Selection opens one epoch over the exact selected seal, scope and recipe
//! pair. An intent follows durable admission: only a fresh admission yields
//! a worker, and each worker mints at most one intent. Every permit names one
//! durable effect and its exact target. Consumption rechecks cancellation,
//! the ledger admission, the worker deadline and a fresh epoch observation
//! before the authority exists. Recovery authority is separate and cannot
//! launch.
//!
//! No production path constructs an [`Installation`] before 7c 16, so no
//! configured input reaches an effect here. Native paths are **unverified on
//! real hardware**.
#![cfg_attr(
    not(test),
    allow(dead_code, reason = "the closed TEST installation lands in 7c 16")
)]

use super::{
    ArtifactSlot, Clock, ClockReading, Id, PairSlots, PendingOperation, RequestId, VmController,
    WEB_RECIPE_ID,
    paths::{ArtifactPin, ArtifactRole, FileIdentity, LocalPath},
    qemu_args::{CommandPlan, PlanKind},
    qualification::{
        ForegroundQualification, LaunchPermit, LaunchPermitBinding, LaunchTarget, StageTarget,
        WorkspaceBinding,
    },
    qualification_record::{Class, Hash, RecipeIdentity, TestRecipes, digest},
    runtime::{InstallerPhase, LaunchAuthority, NativeRefusal},
    selection_epoch::{Invalidation, Sample, SelectionEpoch},
};
use crate::{config::TestQualificationSelection, service::Shutdown, store::vm::native::Scope};
use serde_json::{Value, json};
use std::{
    sync::{Mutex, atomic::Ordering},
    time::Instant,
};

const REFUSED: NativeRefusal = NativeRefusal::QualificationMissing;
/// One worker's intent names at most this many distinct effects.
const MAX_EFFECTS: usize = 16;

/// Only this module can construct it, so only a consumed permit yields a
/// [`LaunchAuthority`].
pub(crate) struct Minted(());

fn authority() -> LaunchAuthority {
    LaunchAuthority {
        _issued: Minted(()),
    }
}

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
    /// The freshness and store-identity reads, then `advance`, each keeping
    /// its typed refusal class.
    fn sample(
        &self,
        advance: &dyn Fn() -> Result<ClockReading, Invalidation>,
    ) -> Result<Sample, Invalidation>;
}

/// The exact durable effect an admitted worker owns.
#[derive(Clone, Copy)]
pub(crate) struct NativeEffect<'a> {
    pub principal: &'a str,
    pub request: RequestId,
    pub operation: Id,
    pub action: i64,
    pub generation: Id,
    /// The unresolved writer epoch of an active generation, for effects on a
    /// running or starting VM. Build and clone staging have none.
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
            config.enrollment.as_str(),
            config.dossier.as_str(),
            &config.selection,
        )?;
        Self::open(installation, controller, source, config)
    }
}

impl<'a, S: SelectedSource, C: Clock> TestIssuer<'a, S, C> {
    /// T01/T02: the held seal, scope, class and recipe pair must match the
    /// configured selection exactly before the first epoch observation.
    pub(crate) fn open(
        _: &Installation,
        controller: &'a VmController<C>,
        source: S,
        config: &TestQualificationSelection,
    ) -> Result<Self, NativeRefusal> {
        let recipes = source.recipes();
        let windows = VmController::<C>::windows_recipe_id();
        if source.seal_sha256() != config.selection.seal_sha256
            || source.scope() != config.selection.scope
            || source.class() != Class::Test
            || recipes.windows.recipe_id != windows
            || [windows, WEB_RECIPE_ID, [0; 16]].contains(&recipes.ubuntu.recipe_id)
        {
            return Err(REFUSED);
        }
        let sample = source.sample(&|| controller.advance_native_clock())?;
        let id = crate::store::vm::new_id().map_err(|_| REFUSED)?;
        let epoch = SelectionEpoch::open(id, sample, Instant::now())?;
        Ok(Self {
            controller,
            source,
            epoch,
            seal_sha256: config.selection.seal_sha256,
            recipes,
        })
    }

    pub(crate) fn epoch(&self) -> &SelectionEpoch {
        &self.epoch
    }

    /// One bounded observation. Only confirmed drift revokes the epoch.
    fn refresh(&self) -> Result<Instant, NativeRefusal> {
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
    /// yields a worker, and a worker mints one intent, so a replay mints
    /// nothing.
    pub(crate) fn intent<'i>(
        &'i self,
        worker: &'i PendingOperation,
        effect: NativeEffect<'i>,
        cancel: &'i Shutdown,
    ) -> Result<Intent<'i, S, C>, NativeRefusal> {
        let intent = Intent {
            issuer: self,
            worker,
            effect,
            cancel,
            minted: Mutex::default(),
        };
        intent.verify(&[])?;
        if worker.intent_minted.swap(true, Ordering::AcqRel) {
            return Err(REFUSED);
        }
        Ok(intent)
    }

    fn selected(&self, recipe: RecipeIdentity, phase: Option<InstallerPhase>) -> bool {
        match phase {
            Some(InstallerPhase::UbuntuInstall) => recipe == self.recipes.ubuntu,
            Some(InstallerPhase::WindowsSetup | InstallerPhase::WindowsFinalize) => {
                recipe == self.recipes.windows
            }
            None => recipe == self.recipes.ubuntu || recipe == self.recipes.windows,
        }
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

#[derive(Clone, PartialEq)]
enum Effect {
    Launch(PlanKind, Vec<ArtifactSlot>),
    Stage(ArtifactSlot),
    Script,
}

/// One admitted worker's authority to mint permits for its durable effect.
pub(crate) struct Intent<'i, S, C> {
    issuer: &'i TestIssuer<'i, S, C>,
    worker: &'i PendingOperation,
    effect: NativeEffect<'i>,
    cancel: &'i Shutdown,
    minted: Mutex<Vec<Effect>>,
}

impl<'i, S: SelectedSource, C: Clock> Intent<'i, S, C> {
    /// Cancellation, then the durable admission, deadline and target, then a
    /// fresh epoch observation. Mint and every consumption run this.
    fn verify(&self, slots: &[ArtifactSlot]) -> Result<(), NativeRefusal> {
        if self.cancel.is_cancelled()
            || !self
                .issuer
                .controller
                .native_effect_admitted(self.worker, &self.effect, slots)
                .map_err(|_| REFUSED)?
        {
            return Err(REFUSED);
        }
        self.issuer.refresh().map(drop)
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

    /// One checked process creation for exactly this plan and these slots.
    pub(crate) fn launch<'p>(
        &'p self,
        workspace: WorkspaceBinding,
        target: LaunchTarget<'_, '_>,
        slots: &'p [ArtifactSlot],
    ) -> Result<Permit<'p, S, C>, NativeRefusal> {
        let expected = EffectPlan::from_target(&target)?;
        if workspace.generation != self.effect.generation
            || target
                .recipe
                .is_some_and(|recipe| !self.issuer.selected(recipe, expected.phase()))
        {
            return Err(REFUSED);
        }
        self.mint(Effect::Launch(expected.kind, slots.to_vec()), slots)?;
        Ok(Permit {
            intent: self,
            binding: LaunchPermitBinding {
                workspace,
                seal_sha256: self.issuer.seal_sha256,
                kind: expected.kind,
            },
            expected,
            slots,
        })
    }

    fn check_stage(&self, target: &StageTarget) -> Result<(), NativeRefusal> {
        if target.workspace.generation != self.effect.generation
            || target.slot.generation != self.effect.generation
            || target.seal_sha256 != self.issuer.seal_sha256
            || !self.issuer.selected(target.recipe, Some(target.phase))
            || Instant::now() >= target.deadline
        {
            return Err(REFUSED);
        }
        Ok(())
    }

    /// One staged write of exactly this content into this slot and phase.
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

    /// One guest script on the live writer epoch, bound to its exact bytes
    /// and timeout.
    pub(crate) fn script(
        &self,
        script: &str,
        timeout_s: u32,
    ) -> Result<ScriptPermit<'_, S, C>, NativeRefusal> {
        let writer_epoch = self.effect.writer_epoch.ok_or(REFUSED)?;
        self.mint(Effect::Script, &[])?;
        Ok(ScriptPermit {
            intent: self,
            generation: self.effect.generation,
            writer_epoch,
            script: digest(script.as_bytes()),
            timeout_s,
        })
    }
}

#[must_use]
pub(crate) struct Permit<'p, S, C> {
    intent: &'p Intent<'p, S, C>,
    binding: LaunchPermitBinding,
    expected: EffectPlan,
    slots: &'p [ArtifactSlot],
}

impl<S: SelectedSource, C: Clock> LaunchPermit for Permit<'_, S, C> {
    fn consume(
        self,
        target: LaunchTarget<'_, '_>,
    ) -> Result<(LaunchPermitBinding, LaunchAuthority), NativeRefusal> {
        if self.expected != EffectPlan::from_target(&target)? {
            return Err(REFUSED);
        }
        self.intent.verify(self.slots)?;
        Ok((self.binding, authority()))
    }
}

#[must_use]
pub(crate) struct StagePermit<'p, S, C> {
    intent: &'p Intent<'p, S, C>,
    target: StageTarget,
}

impl<S: SelectedSource, C: Clock> StagePermit<'_, S, C> {
    /// At the staged write: the exact target, still in phase, still fresh.
    pub(crate) fn consume(self, target: StageTarget) -> Result<LaunchAuthority, NativeRefusal> {
        if self.target != target {
            return Err(REFUSED);
        }
        self.intent.check_stage(&target)?;
        self.intent.verify(&[target.slot])?;
        Ok(authority())
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
    ) -> Result<LaunchAuthority, NativeRefusal> {
        if (self.generation, self.writer_epoch, self.timeout_s)
            != (generation, writer_epoch, timeout_s)
            || self.script != digest(script.as_bytes())
        {
            return Err(REFUSED);
        }
        self.intent.verify(&[])?;
        Ok(authority())
    }
}

/// Settlement authority over exact durable TEST custody. It needs no fresh
/// selection, so a revoked or absent epoch cannot strand recovery, and no
/// path converts it into a [`LaunchAuthority`].
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
    image: LocalPath,
    argv: Vec<String>,
    inputs: Vec<EffectInput>,
    cwd: LocalPath,
    pair: Option<PairSlots>,
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
            image: plan.image().clone(),
            argv: plan.args().to_vec(),
            inputs,
            cwd: target.cwd.clone(),
            pair: target.pair,
            installer: target
                .installer
                .map(|stage| (stage.phase(), stage.deadline())),
            recipe: target.recipe,
            memory: target.memory,
            cpu_rate: target.cpu_rate,
        })
    }

    fn phase(&self) -> Option<InstallerPhase> {
        match self.kind {
            PlanKind::InstallerBoot(phase) => Some(phase),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;
