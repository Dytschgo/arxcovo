mod media;
mod ubuntu;
pub mod ubuntu_compose;
pub mod ubuntu_web;
pub(in crate::vm) mod windows;

#[cfg(all(windows, target_arch = "x86_64"))]
pub(in crate::vm) use ubuntu::network_witness_seed;
pub use ubuntu::{AssetPin, RecipeError, UbuntuAssets, UbuntuRecipe};
#[cfg(all(windows, target_arch = "x86_64"))]
pub(in crate::vm) use ubuntu::{
    DETACHED_HEALTH as UBUNTU_DETACHED_HEALTH, installed_witness_script,
};

pub(super) fn windows_recipe_id() -> super::Id {
    windows::catalog_id()
}

/// The built-in Ubuntu web catalog entry, never a TEST recipe.
pub(crate) const WEB_RECIPE_ID: super::Id = [0xff; 16];

pub(super) const WINDOWS_BUILD_LIMIT: std::time::Duration = windows::BUILD_LIMIT;

#[allow(
    dead_code,
    reason = "foreground lifecycle composition is deferred; native admission is closed"
)]
mod lifecycle {
    use super::{RecipeError, UbuntuAssets, UbuntuRecipe, ubuntu::DETACHED_HEALTH, windows};
    use crate::vm::{
        ArtifactEvidence, BuildAdmission, BuildRequest, BuildSlots, Clock, CloneAdmission,
        FailureReason, HostObservation, Id, OperationRef, PairSlots, PendingOperation, Refusal,
        Request, Resources, Status, VmController,
        paths::ArtifactPin,
        runtime::{CandidateRuntime, InstallerFailure, NativeRefusal, StopDisposition},
    };
    use arx_guest::protocol::ScriptStatus;
    use sha2::{Digest, Sha256};
    use std::time::Duration;

    const HEALTH_TIMEOUT_S: u32 = 30;
    const INSTALL_TIMEOUT: Duration = Duration::from_secs(60 * 60);

    enum Seed {
        Ubuntu(Vec<u8>),
        Windows(Vec<u8>, windows::PreparedWindows),
    }

    impl Seed {
        fn prepare(
            recipe: &UbuntuRecipe,
            assets: UbuntuAssets<'_>,
            id: Id,
            guest: &[u8],
        ) -> Result<Self, RecipeError> {
            let mut bytes = Vec::new();
            recipe.write_seed(assets, id, guest, &mut bytes)?;
            Ok(Self::Ubuntu(bytes))
        }

        fn bytes(&self) -> &[u8] {
            match self {
                Self::Ubuntu(bytes) | Self::Windows(bytes, _) => bytes,
            }
        }

        fn check_binding(&self, spec: &BuildRequest<'_>) -> Result<(), Failure> {
            match self {
                Self::Windows(_, prepared)
                    if spec.recipe == hex::encode(windows::catalog_id())
                        && spec.digest == prepared.digest =>
                {
                    Ok(())
                }
                Self::Ubuntu(_) if spec.recipe != hex::encode(windows::catalog_id()) => Ok(()),
                _ => Err(Failure::Seed(RecipeError::Unqualified)),
            }
        }

        fn prepare_windows(
            recipe: &windows::WindowsRecipe<'_>,
            assets: windows::WindowsAssets<'_>,
            guest: &[u8],
        ) -> Result<Self, RecipeError> {
            let mut bytes = Vec::new();
            let prepared = recipe.write_test_seed(assets, guest, &mut bytes)?;
            Ok(Self::Windows(bytes, prepared))
        }

        fn pin(&self) -> ArtifactPin {
            ArtifactPin {
                size: self.bytes().len() as u64,
                sha256: Sha256::digest(self.bytes()).into(),
            }
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(super) struct Pair {
        disk: ArtifactEvidence,
        vars: ArtifactEvidence,
    }

    impl Pair {
        fn fits(self, images: u64) -> bool {
            self.disk.pin.size > 0
                && self.vars.pin.size > 0
                && self.disk.identity != self.vars.identity
                && self
                    .disk
                    .pin
                    .size
                    .checked_add(self.vars.pin.size)
                    .is_some_and(|size| size <= images)
        }

        fn evidence(self) -> [ArtifactEvidence; 2] {
            [self.disk, self.vars]
        }
    }

    #[derive(Debug)]
    pub(super) struct Golden {
        id: Id,
        slots: PairSlots,
        files: Pair,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(super) enum LinkedSlot {
        Verification(PairSlots),
        Clone(Id, PairSlots),
    }

    #[derive(Debug)]
    pub(super) struct Linked {
        slot: LinkedSlot,
        files: Pair,
        backing: ArtifactEvidence,
    }

    impl Linked {
        fn matches(&self, golden: &Golden, slot: LinkedSlot, images: u64) -> bool {
            self.slot == slot
                && self.backing == golden.files.disk
                && self.files.fits(images)
                && self.files.vars.pin == golden.files.vars.pin
                && [self.files.disk.identity, self.files.vars.identity]
                    .iter()
                    .all(|id| {
                        *id != golden.files.disk.identity && *id != golden.files.vars.identity
                    })
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(super) struct Boot(Id);

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(super) enum Readiness {
        ScriptChannel,
        PingOnly,
        Timeout,
        WrongPeer,
    }

    impl Failure {
        /// The closed cause a failed staged build records. Store, policy and
        /// interruption failures have no proven guest-side cause.
        fn cause(&self, stage: FailureReason) -> Option<FailureReason> {
            match self {
                Self::Stop(_) => Some(FailureReason::DirtyStop),
                Self::Health | Self::Readiness(_) | Self::ReusedBoot => {
                    Some(FailureReason::HealthFailed)
                }
                Self::Seed(_) => Some(FailureReason::StagingFailed),
                Self::Media
                | Self::ChangedPair
                | Self::Installer { .. }
                | Self::InstallerDeadline(_) => Some(stage),
                Self::GeneralizationUnverified => Some(FailureReason::SealFailed),
                Self::Native(_) | Self::Refused(_) | Self::Store(_) | Self::Interrupted => None,
            }
        }
    }

    #[derive(Debug)]
    pub(super) enum Failure {
        Native(NativeRefusal),
        Refused(Refusal),
        Store(anyhow::Error),
        Seed(RecipeError),
        Media,
        ChangedPair,
        ReusedBoot,
        Readiness(Readiness),
        Health,
        GeneralizationUnverified,
        Installer {
            report: InstallerFailure,
            observed_at: tokio::time::Instant,
        },
        InstallerDeadline(InstallerFailure),
        Stop(StopDisposition),
        Interrupted,
    }

    impl Failure {
        /// Preserve installer recovery evidence while projecting the existing
        /// public refusal for absolute expiry.
        fn native_refusal(&self) -> Option<NativeRefusal> {
            match self {
                Self::Native(reason) => Some(*reason),
                Self::InstallerDeadline(_) => Some(NativeRefusal::InstallerDeadlineExpired),
                _ => None,
            }
        }
    }

    /// Trust boundary for storage and process effects. Implementations must
    /// establish custody, observe fixed budgets, and retain resources if an
    /// operation fails without proving release. No adapter issues a native lease.
    pub(super) trait TemplateRuntime {
        /// Create fresh backing-less disk, private VARS and read-only CIDATA
        /// in the reserved generation, under the supplied byte limits. Return
        /// the CIDATA pin from a bounded reread through its custody handle.
        fn stage(
            &mut self,
            slots: BuildSlots,
            spec: &BuildRequest<'_>,
            seed: &[u8],
        ) -> Result<ArtifactEvidence, Failure>;
        /// Attach the installer and CIDATA, then observe poweroff and release.
        fn install(
            &mut self,
            slots: BuildSlots,
            timeout: Duration,
        ) -> Result<(Boot, StopDisposition), Failure>;
        /// Reopen the backing-less candidate at its final generation paths and
        /// verify the image, role, identity, size and digest of both files.
        fn capture_golden(&mut self, slots: PairSlots, images: u64) -> Result<Pair, Failure>;
        /// Create a fresh overlay with explicit qcow2 backing at the pinned
        /// golden disk and copy VARS into a distinct writable file. Return
        /// the verified immediate backing, with no other chain or data file.
        fn link(
            &mut self,
            golden: &Golden,
            slot: LinkedSlot,
            images: u64,
        ) -> Result<Linked, Failure>;
        /// Fresh child, serial endpoint and installed boot with only this pair.
        fn boot_detached(&mut self, pair: &Linked) -> Result<Boot, Failure>;
        /// ScriptChannel requires verified peer custody and a boot-local v1
        /// Hello echo advertising script support within GUEST_READY_BUDGET.
        fn ready(&mut self, boot: Boot) -> Result<Readiness, Failure>;
        fn health(
            &mut self,
            boot: Boot,
            script: &str,
            seconds: u32,
        ) -> Result<(ScriptStatus, Option<u32>), Failure>;
        fn stop(&mut self, boot: Boot) -> StopDisposition;
        /// Setup has the fixed OS, seed and driver media attached. Finalize
        /// has only the candidate pair. Both run through QMP without a guest
        /// script channel, and return only after release has been resolved.
        /// An installer report carries the time its reason was observed,
        /// before any independent recovery wait.
        fn windows_installer(
            &mut self,
            _slots: BuildSlots,
            _stage: crate::vm::runtime::InstallerStage,
        ) -> Result<(Boot, crate::vm::runtime::InstallerPhase), Failure> {
            Err(Failure::Native(NativeRefusal::InstallerPhaseUnavailable))
        }
        /// Read the at-rest candidate's SOFTWARE hive and Setup State.ini
        /// through a bounded read-only view of this exact released pair.
        /// The adapter verifies custody and both artifact pins across the read.
        fn windows_offline_setup_state(
            &mut self,
            slots: PairSlots,
            files: Pair,
            deadline: tokio::time::Instant,
        ) -> Result<windows::OfflineSetupState, Failure> {
            #[cfg(all(windows, target_arch = "x86_64"))]
            if let Some(released) = self.windows_released_pair() {
                let (registry, state_ini) = released
                    .observe(slots, files.evidence(), deadline.into_std())
                    .map_err(|_| Failure::GeneralizationUnverified)?;
                return Ok(windows::OfflineSetupState::from_released_pair(
                    registry, state_ini,
                ));
            }
            let _ = (slots, files, deadline);
            Err(Failure::GeneralizationUnverified)
        }

        #[cfg(all(windows, target_arch = "x86_64"))]
        fn windows_released_pair(&self) -> Option<&crate::vm::windows::offline_read::ReleasedPair> {
            None
        }
        /// Launch with media detached and establish boot-local readiness by
        /// the deadline. If startup fails before handing back a boot handle,
        /// the adapter is responsible for teardown or charged quarantine.
        fn windows_installed(
            &mut self,
            _target: windows::InstalledTarget,
            _deadline: tokio::time::Instant,
        ) -> Result<(Boot, Readiness), Failure> {
            Err(Failure::Native(NativeRefusal::InstallerPhaseUnavailable))
        }
        fn windows_health(
            &mut self,
            _boot: Boot,
            _script: &str,
            _deadline: tokio::time::Instant,
        ) -> Result<(ScriptStatus, Option<u32>), Failure> {
            Err(Failure::Native(NativeRefusal::InstallerPhaseUnavailable))
        }
        /// An expired deadline still requires teardown, with the runtime's
        /// independent recovery budget and dirty/quarantined disposition.
        fn windows_stop(
            &mut self,
            _boot: Boot,
            _deadline: tokio::time::Instant,
        ) -> StopDisposition {
            StopDisposition::ChargedQuarantine
        }
        /// Remove only the attributable, released CIDATA and verification pair.
        /// Keep the captured golden pair protected at its final paths.
        fn discard_transients(&mut self, slots: BuildSlots) -> Result<(), Failure>;
    }

    enum Installer<'a> {
        Candidate(&'a CandidateRuntime<'a>),
        #[cfg(test)]
        Fixture(&'a mut dyn TemplateRuntime),
    }

    impl<'a> Installer<'a> {
        fn admit(self) -> Result<&'a mut dyn TemplateRuntime, Failure> {
            match self {
                Self::Candidate(candidate) => candidate
                    .template_installer()
                    .map(|closed| match closed {})
                    .map_err(Failure::Native),
                #[cfg(test)]
                Self::Fixture(runtime) => Ok(runtime),
            }
        }
    }

    struct Lifecycle<'a, C> {
        controller: &'a VmController<C>,
        host: HostObservation,
        resources: Resources,
    }

    #[derive(Debug)]
    enum BuildResult {
        Created(Box<Golden>, OperationRef),
        Existing(OperationRef),
    }

    #[derive(Debug)]
    enum CloneResult {
        Created(Box<Linked>, OperationRef),
        Existing(OperationRef),
    }

    enum InstalledCandidate<'a> {
        Ubuntu(Boot),
        Windows(windows::WindowsBuild<'a>),
    }

    impl<C: Clock> Lifecycle<'_, C> {
        fn build(
            &self,
            installer: Installer<'_>,
            spec: BuildRequest<'_>,
            prepare: impl FnOnce(Id) -> Result<Seed, RecipeError>,
        ) -> Result<BuildResult, Failure> {
            let runtime = installer.admit()?;
            let admitted = match self
                .controller
                .admit_build(spec, self.host, self.resources, |_, pair| {
                    runtime
                        .capture_golden(pair, spec.retained.images)
                        .ok()
                        .map(Pair::evidence)
                })
                .map_err(Failure::Store)?
                .map_err(Failure::Refused)?
            {
                BuildAdmission::Fresh(slots, worker, operation) => (*slots, worker, operation),
                BuildAdmission::Existing(operation) => return Ok(BuildResult::Existing(operation)),
            };
            Self::finish_reserved_build(self.controller, runtime, spec, admitted, prepare)
        }

        fn finish_reserved_build(
            controller: &VmController<C>,
            runtime: &mut dyn TemplateRuntime,
            spec: BuildRequest<'_>,
            (slots, worker, mut operation): (BuildSlots, PendingOperation, OperationRef),
            prepare: impl FnOnce(Id) -> Result<Seed, RecipeError>,
        ) -> Result<BuildResult, Failure> {
            let seed = prepare(slots.id).map_err(Failure::Seed).and_then(|seed| {
                if seed.bytes().len() as u64 > spec.transient.staging {
                    return Err(Failure::Refused(Refusal::Disk));
                }
                seed.check_binding(&spec)?;
                Ok(seed)
            });
            if seed.is_err() {
                controller
                    .fail_build(&worker, slots, true)
                    .map_err(Failure::Store)?;
            }
            let seed = seed?;
            let evidence = |entries: &[_]| {
                controller
                    .build_evidence(&worker, slots.id, entries)
                    .map_err(Failure::Store)
            };
            // Media faults before a clean install are staging failures;
            // afterwards they fail the golden's capture and seal.
            let stage = std::cell::Cell::new(FailureReason::StagingFailed);
            let built = (|| {
                let staged = runtime.stage(slots, &spec, seed.bytes())?;
                if staged.pin != seed.pin() {
                    return Err(Failure::Media);
                }
                evidence(&[
                    (slots.seed, None),
                    (slots.seed, Some(staged)),
                    (slots.pair.disk, None),
                    (slots.pair.vars, None),
                ])?;
                let mut installed = match &seed {
                    Seed::Windows(_, prepared) => InstalledCandidate::Windows(
                        windows::WindowsBuild::install(runtime, slots, &prepared.image)?,
                    ),
                    Seed::Ubuntu(_) => {
                        let (boot, stop) = runtime.install(slots, INSTALL_TIMEOUT)?;
                        clean(stop)?;
                        InstalledCandidate::Ubuntu(boot)
                    }
                };
                stage.set(FailureReason::SealFailed);

                let files = runtime.capture_golden(slots.pair, spec.retained.images)?;
                if !files.fits(spec.retained.images) {
                    return Err(Failure::Media);
                }
                evidence(&[
                    (slots.pair.disk, Some(files.disk)),
                    (slots.pair.vars, Some(files.vars)),
                ])?;
                let golden = Golden {
                    id: slots.id,
                    slots: slots.pair,
                    files,
                };
                if let InstalledCandidate::Windows(build) = &mut installed {
                    build.attest_generalization(runtime, slots.pair, files)?;
                }
                let slot = LinkedSlot::Verification(slots.verification);
                let linked = runtime.link(&golden, slot, spec.transient.images)?;
                if !linked.matches(&golden, slot, spec.transient.images) {
                    return Err(Failure::Media);
                }
                evidence(&[
                    (slots.verification.disk, None),
                    (slots.verification.vars, None),
                    (slots.verification.disk, Some(linked.files.disk)),
                    (slots.verification.vars, Some(linked.files.vars)),
                ])?;
                match &mut installed {
                    InstalledCandidate::Ubuntu(boot) => verify_detached(runtime, &linked, *boot)?,
                    InstalledCandidate::Windows(build) => {
                        build.verify(runtime, slots.verification)?;
                    }
                }
                runtime.discard_transients(slots)?;
                unchanged(runtime, &golden, spec.retained.images)?;
                if let InstalledCandidate::Windows(build) = installed {
                    build.check_seal()?;
                }
                Ok(golden)
            })();
            if let Err(failure) = &built {
                controller
                    .fail_build_with(&worker, slots, false, failure.cause(stage.get()))
                    .map_err(Failure::Store)?;
            }
            let golden = built?;
            let status = controller
                .finish_build(&worker, slots, spec, golden.files.evidence())
                .map_err(Failure::Store)?;
            if status != Status::Succeeded {
                return Err(Failure::Interrupted);
            }
            operation.receipt.status = status;
            Ok(BuildResult::Created(Box::new(golden), operation))
        }

        fn linked_clone(
            &self,
            runtime: &mut dyn TemplateRuntime,
            golden: &Golden,
            request: Request<'_>,
        ) -> Result<CloneResult, Failure> {
            let images = request.disk.images;
            let (vm, slots, pending, mut operation) = match self
                .controller
                .admit_linked_clone(
                    request,
                    self.host,
                    self.resources,
                    golden.slots,
                    golden.files.evidence(),
                )
                .map_err(Failure::Store)?
                .map_err(Failure::Refused)?
            {
                CloneAdmission::Fresh(vm, slots, pending, operation) => {
                    (vm, slots, pending, operation)
                }
                CloneAdmission::Existing(receipt) => return Ok(CloneResult::Existing(receipt)),
            };
            let slot = LinkedSlot::Clone(vm, slots);
            let linked = (|| {
                let golden_bytes = golden.files.disk.pin.size + golden.files.vars.pin.size;
                unchanged(runtime, golden, golden_bytes)?;
                let pair = runtime.link(golden, slot, images)?;
                if !pair.matches(golden, slot, images) {
                    return Err(Failure::Media);
                }
                unchanged(runtime, golden, golden_bytes)?;
                Ok(pair)
            })();
            let status = self
                .controller
                .finish_linked_clone_with(
                    &pending,
                    slots,
                    match &linked {
                        Ok(pair) => Ok(pair.files.evidence()),
                        Err(failure) => Err(failure.cause(FailureReason::CloneFailed)),
                    },
                )
                .map_err(Failure::Store)?;
            let pair = linked?;
            if status != Status::Succeeded {
                return Err(Failure::Interrupted);
            }
            operation.receipt.status = status;
            Ok(CloneResult::Created(Box::new(pair), operation))
        }
    }

    pub(super) fn clean(stop: StopDisposition) -> Result<(), Failure> {
        if stop != StopDisposition::CleanRelease {
            return Err(Failure::Stop(stop));
        }
        Ok(())
    }

    fn unchanged(
        runtime: &mut dyn TemplateRuntime,
        golden: &Golden,
        images: u64,
    ) -> Result<(), Failure> {
        if runtime.capture_golden(golden.slots, images)? != golden.files {
            return Err(Failure::ChangedPair);
        }
        Ok(())
    }

    fn verify_detached(
        runtime: &mut dyn TemplateRuntime,
        pair: &Linked,
        installed: Boot,
    ) -> Result<(), Failure> {
        let boot = runtime.boot_detached(pair)?;
        let checked = (|| {
            if boot == installed {
                return Err(Failure::ReusedBoot);
            }
            let ready = runtime.ready(boot)?;
            if ready != Readiness::ScriptChannel {
                return Err(Failure::Readiness(ready));
            }
            if runtime.health(boot, DETACHED_HEALTH, HEALTH_TIMEOUT_S)?
                != (ScriptStatus::Exited, Some(0))
            {
                return Err(Failure::Health);
            }
            Ok(())
        })();
        clean(runtime.stop(boot))?;
        checked
    }

    #[cfg(test)]
    mod foreground_fixture {
        use super::*;
        use crate::vm::{
            ArtifactSlot,
            runtime::{InstallerPhase, InstallerStage},
        };
        use tokio::time::Instant;

        type WriteSlot<'a> = &'a dyn Fn(ArtifactSlot, &[u8]) -> anyhow::Result<ArtifactEvidence>;
        type CheckSlot<'a> = &'a dyn Fn(ArtifactSlot, ArtifactEvidence) -> anyhow::Result<()>;

        fn prepare_seed() -> Result<Seed, RecipeError> {
            let recipe = windows::WindowsRecipe {
                pins: Some(windows::tests::fixture()),
            };
            windows::tests::with_assets(|assets, guest| {
                Seed::prepare_windows(&recipe, assets, guest)
            })
        }

        impl<C: Clock> VmController<C> {
            pub(crate) fn windows_fixture_digest() -> [u8; 32] {
                let Seed::Windows(_, prepared) = prepare_seed().expect("fixture recipe") else {
                    unreachable!("fixture recipe is Windows");
                };
                prepared.digest
            }

            pub(crate) fn finish_windows_fixture_build(
                &self,
                spec: BuildRequest<'_>,
                admitted: (BuildSlots, PendingOperation, OperationRef),
                write: WriteSlot<'_>,
                verify: CheckSlot<'_>,
                remove: CheckSlot<'_>,
            ) -> anyhow::Result<(PairSlots, [ArtifactEvidence; 2])> {
                let mut runtime = Runtime {
                    slots: admitted.0,
                    write,
                    verify,
                    remove,
                    seed: None,
                    candidate: None,
                    verification: None,
                    sequence: 0,
                    active: None,
                };
                let outcome =
                    Lifecycle::finish_reserved_build(self, &mut runtime, spec, admitted, |_| {
                        prepare_seed()
                    });
                match outcome {
                    Ok(BuildResult::Created(golden, _)) => {
                        Ok((golden.slots, golden.files.evidence()))
                    }
                    Err(Failure::Store(error)) => Err(error),
                    _ => anyhow::bail!("Windows fixture build did not seal"),
                }
            }
        }

        // Uses the MCP fixture's held-file custody and the template lifecycle's
        // real durable settlement. The boot observations are synthetic.
        struct Runtime<'a> {
            slots: BuildSlots,
            write: WriteSlot<'a>,
            verify: CheckSlot<'a>,
            remove: CheckSlot<'a>,
            seed: Option<ArtifactEvidence>,
            candidate: Option<Pair>,
            verification: Option<Pair>,
            sequence: u8,
            active: Option<Boot>,
        }

        impl Runtime<'_> {
            fn pair(&self, slots: PairSlots, disk: &[u8]) -> Result<Pair, Failure> {
                Ok(Pair {
                    disk: (self.write)(slots.disk, disk).map_err(Failure::Store)?,
                    vars: (self.write)(slots.vars, b"vars").map_err(Failure::Store)?,
                })
            }

            fn boot(&mut self, deadline: Instant) -> Result<Boot, Failure> {
                if Instant::now() >= deadline {
                    return Err(Failure::Native(NativeRefusal::InstallerDeadlineExpired));
                }
                assert!(self.active.is_none());
                self.sequence += 1;
                Ok(Boot([self.sequence; 16]))
            }
        }

        impl TemplateRuntime for Runtime<'_> {
            fn stage(
                &mut self,
                slots: BuildSlots,
                _: &BuildRequest<'_>,
                seed: &[u8],
            ) -> Result<ArtifactEvidence, Failure> {
                assert_eq!(slots, self.slots);
                let held = (self.write)(slots.seed, seed).map_err(Failure::Store)?;
                self.seed = Some(held);
                self.candidate = Some(self.pair(slots.pair, b"candidate disk")?);
                Ok(held)
            }

            fn install(
                &mut self,
                _: BuildSlots,
                _: Duration,
            ) -> Result<(Boot, StopDisposition), Failure> {
                unreachable!("Windows fixture used Ubuntu installer")
            }

            fn capture_golden(&mut self, slots: PairSlots, _: u64) -> Result<Pair, Failure> {
                assert!(self.active.is_none());
                assert_eq!(slots, self.slots.pair);
                assert!(self.sequence >= 3);
                let pair = self.candidate.expect("staged candidate");
                (self.verify)(slots.disk, pair.disk).map_err(Failure::Store)?;
                (self.verify)(slots.vars, pair.vars).map_err(Failure::Store)?;
                Ok(pair)
            }

            fn link(
                &mut self,
                golden: &Golden,
                slot: LinkedSlot,
                _: u64,
            ) -> Result<Linked, Failure> {
                assert_eq!(slot, LinkedSlot::Verification(self.slots.verification));
                let files = self.pair(self.slots.verification, &golden.files.disk.pin.sha256)?;
                self.verification = Some(files);
                Ok(Linked {
                    slot,
                    files,
                    backing: golden.files.disk,
                })
            }

            fn boot_detached(&mut self, _: &Linked) -> Result<Boot, Failure> {
                unreachable!("Windows fixture used Ubuntu detached boot")
            }

            fn ready(&mut self, _: Boot) -> Result<Readiness, Failure> {
                unreachable!("Windows fixture used Ubuntu readiness")
            }

            fn health(
                &mut self,
                _: Boot,
                _: &str,
                _: u32,
            ) -> Result<(ScriptStatus, Option<u32>), Failure> {
                unreachable!("Windows fixture used Ubuntu health")
            }

            fn stop(&mut self, boot: Boot) -> StopDisposition {
                assert_eq!(self.active.take(), Some(boot));
                StopDisposition::CleanRelease
            }

            fn windows_installer(
                &mut self,
                slots: BuildSlots,
                stage: InstallerStage,
            ) -> Result<(Boot, InstallerPhase), Failure> {
                assert_eq!(slots, self.slots);
                assert!(matches!(
                    (self.sequence, stage.phase()),
                    (0, InstallerPhase::WindowsSetup) | (2, InstallerPhase::WindowsFinalize)
                ));
                let boot = self.boot(stage.deadline())?;
                let observed_at = Instant::now();
                stage
                    .complete(Ok(()), StopDisposition::CleanRelease, false)
                    .map_err(|report| Failure::Installer {
                        report,
                        observed_at,
                    })?;
                Ok((boot, stage.phase()))
            }

            fn windows_offline_setup_state(
                &mut self,
                slots: PairSlots,
                files: Pair,
                _: Instant,
            ) -> Result<windows::OfflineSetupState, Failure> {
                assert_eq!(self.sequence, 3);
                assert_eq!(Some(files), self.candidate);
                assert_eq!(self.capture_golden(slots, u64::MAX)?, files);
                Ok(windows::OfflineSetupState {
                    registry: "IMAGE_STATE_GENERALIZE_RESEAL_TO_OOBE".to_owned(),
                    state_ini: "IMAGE_STATE_GENERALIZE_RESEAL_TO_OOBE".to_owned(),
                })
            }

            fn windows_installed(
                &mut self,
                target: windows::InstalledTarget,
                deadline: Instant,
            ) -> Result<(Boot, Readiness), Failure> {
                let expected = match self.sequence {
                    1 => windows::InstalledTarget::Readiness(self.slots.pair),
                    3 => windows::InstalledTarget::Verification(self.slots.verification),
                    _ => panic!("Windows fixture boot sequence"),
                };
                assert_eq!(target, expected);
                let boot = self.boot(deadline)?;
                self.active = Some(boot);
                Ok((boot, Readiness::ScriptChannel))
            }

            fn windows_health(
                &mut self,
                boot: Boot,
                script: &str,
                _: Instant,
            ) -> Result<(ScriptStatus, Option<u32>), Failure> {
                assert_eq!(self.active, Some(boot));
                let phase = if self.sequence == 2 {
                    "$true"
                } else {
                    "$false"
                };
                assert!(script.starts_with(&format!("$validation = {phase}\n")));
                Ok((ScriptStatus::Exited, Some(0)))
            }

            fn windows_stop(&mut self, boot: Boot, _: Instant) -> StopDisposition {
                self.stop(boot)
            }

            fn discard_transients(&mut self, slots: BuildSlots) -> Result<(), Failure> {
                assert!(self.active.is_none());
                assert_eq!(slots, self.slots);
                let verify = self.verification.expect("staged verification pair");
                for (slot, evidence) in [
                    (slots.seed, self.seed.expect("staged seed")),
                    (slots.verification.disk, verify.disk),
                    (slots.verification.vars, verify.vars),
                ] {
                    (self.remove)(slot, evidence).map_err(Failure::Store)?;
                }
                Ok(())
            }
        }
    }

    #[cfg(test)]
    pub(super) mod tests {
        use super::*;
        use crate::{
            service::application::tests::private_temp,
            vm::{
                Admission, ArtifactSlot, Capacity, ClockReading, Completion, DEFAULT_LEASE_SECONDS,
                DiskClaim, Operation, RETRY_WINDOW_MS, Receipt, RequestId, ScriptResult, VmPolicy,
                paths::FileIdentity,
                runtime::{InstallerFailureReason, InstallerPhase, InstallerStage},
            },
        };
        use rusqlite::Connection;
        use std::{
            fs,
            io::Write,
            path::{Path, PathBuf},
            sync::mpsc,
            time::Instant,
        };

        const INSTALL: Boot = Boot([3; 16]);
        const DETACHED: Boot = Boot([4; 16]);
        const MIB: u64 = 1024 * 1024;
        const GIB: u64 = 1024 * MIB;
        const NOW: i64 = 1_800_000_000_000;

        struct TestClock(Instant);

        impl Clock for TestClock {
            fn now(&self) -> ClockReading {
                ClockReading {
                    utc_ms: NOW,
                    monotonic: self.0,
                }
            }
        }

        struct Fixture {
            controller: VmController<TestClock>,
            root: tempfile::TempDir,
        }

        impl Fixture {
            fn new() -> Self {
                let root = private_temp();
                protect_fixture(root.path(), true);
                let controller =
                    VmController::open(root.path(), VmPolicy::default(), TestClock(Instant::now()))
                        .unwrap();
                Self { controller, root }
            }

            fn lifecycle(&self) -> Lifecycle<'_, TestClock> {
                Lifecycle {
                    controller: &self.controller,
                    host: HostObservation {
                        owner_idle: Some(true),
                        logical_cpus: Some(8),
                        available_ram: Some(32 * GIB),
                        capacity: Some(Capacity {
                            free_bytes: 400 * GIB,
                            managed_bytes: 0,
                        }),
                    },
                    resources: Resources {
                        vcpus: 2,
                        guest_ram: 4 * GIB,
                        overhead_ram: 256 * MIB,
                    },
                }
            }

            fn runtime(&self) -> Recording<'_> {
                Recording {
                    root: self.root.path(),
                    calls: Vec::new(),
                    live: None,
                    fault: None,
                    contend: None,
                    build_slots: None,
                    clone_slots: None,
                    clone_count: 0,
                    windows: None,
                }
            }
        }

        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        enum Fault {
            PartialStage,
            SeedMismatch,
            Capture,
            InstallerStop(StopDisposition),
            ReuseChild,
            Readiness(Readiness),
            Health(ScriptStatus, Option<u32>),
            Stop(StopDisposition),
            GoldenWrite,
            WrongBacking,
            SharedVars,
            CrashAfterCloneWrite,
            RecipeBinding,
        }

        struct Recording<'a> {
            root: &'a Path,
            calls: Vec<&'static str>,
            live: Option<Boot>,
            fault: Option<Fault>,
            contend: Option<&'a Fixture>,
            build_slots: Option<BuildSlots>,
            clone_slots: Option<PairSlots>,
            clone_count: u8,
            windows: Option<WindowsFixture>,
        }

        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        enum WindowsFault {
            Qmp(InstallerPhase, InstallerFailureReason),
            PhaseMismatch,
            PingOnly,
            NonzeroHealth,
            ReusedBoot,
            UnprovedStop,
            FailedSysprep,
        }

        type PhaseLatch = (
            tokio::sync::oneshot::Sender<tokio::time::Instant>,
            mpsc::Receiver<()>,
        );

        #[derive(Default)]
        struct WindowsFixture {
            fault: Option<WindowsFault>,
            setup_latch: Option<PhaseLatch>,
            health_latch: Option<PhaseLatch>,
            setup_deadline: Option<tokio::time::Instant>,
            installed_target: Option<windows::InstalledTarget>,
        }

        fn db(root: &Path) -> Connection {
            Connection::open(root.join("activity.sqlite3")).unwrap()
        }

        fn scalar(root: &Path, query: &str) -> i64 {
            db(root).query_row(query, [], |row| row.get(0)).unwrap()
        }

        fn create(path: &Path, bytes: &[u8]) {
            fs::File::create_new(path)
                .unwrap()
                .write_all(bytes)
                .unwrap();
            protect_fixture(path, false);
        }

        #[cfg(unix)]
        pub(in crate::vm::template) fn protect_fixture(path: &Path, directory: bool) {
            use std::os::unix::fs::PermissionsExt;
            let mode = if directory { 0o700 } else { 0o600 };
            fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
        }

        #[cfg(windows)]
        pub(in crate::vm::template) fn protect_fixture(path: &Path, directory: bool) {
            use std::{os::windows::ffi::OsStrExt, ptr};
            use windows_sys::Win32::{
                Foundation::LocalFree,
                Security::{Authorization::*, *},
            };
            let user = crate::local::current_identity().unwrap();
            let inheritance = if directory { "OICI" } else { "" };
            let sddl: Vec<u16> = format!("O:{user}D:P(A;{inheritance};FA;;;{user})")
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            unsafe {
                let mut sd = ptr::null_mut();
                assert_ne!(
                    ConvertStringSecurityDescriptorToSecurityDescriptorW(
                        sddl.as_ptr(),
                        1,
                        &mut sd,
                        ptr::null_mut()
                    ),
                    0
                );
                let (mut present, mut defaulted) = (0, 0);
                let (mut acl, mut owner) = (ptr::null_mut(), ptr::null_mut());
                assert_ne!(
                    GetSecurityDescriptorDacl(sd, &mut present, &mut acl, &mut defaulted),
                    0
                );
                assert_ne!(
                    GetSecurityDescriptorOwner(sd, &mut owner, &mut defaulted),
                    0
                );
                let dacl = SetNamedSecurityInfoW(
                    wide.as_ptr(),
                    SE_FILE_OBJECT,
                    PROTECTED_DACL_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    acl,
                    ptr::null_mut(),
                );
                let owner = SetNamedSecurityInfoW(
                    wide.as_ptr(),
                    SE_FILE_OBJECT,
                    OWNER_SECURITY_INFORMATION,
                    owner,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                );
                LocalFree(sd);
                assert_eq!((dacl, owner), (0, 0));
            }
        }

        fn file(path: &Path, id: u8) -> ArtifactEvidence {
            let bytes = fs::read(path).unwrap();
            ArtifactEvidence {
                identity: FileIdentity {
                    volume: 1,
                    file: [id; 16],
                },
                directory: [9; 16],
                pin: ArtifactPin {
                    size: bytes.len() as u64,
                    sha256: Sha256::digest(bytes).into(),
                },
            }
        }

        impl Recording<'_> {
            fn slot_path(&self, slot: ArtifactSlot) -> PathBuf {
                self.root
                    .join(format!("{}-{}", slot.role, hex::encode(slot.key)))
            }

            fn path(&self, name: &str) -> PathBuf {
                let build = || self.build_slots.unwrap();
                let clone = || self.clone_slots.unwrap();
                let slot = match name {
                    "seed.iso" => build().seed,
                    "golden.disk" => build().pair.disk,
                    "golden.vars" => build().pair.vars,
                    "verify.disk" => build().verification.disk,
                    "verify.vars" => build().verification.vars,
                    "clone.disk" => clone().disk,
                    "clone.vars" => clone().vars,
                    _ => panic!("unexpected fixture role"),
                };
                self.slot_path(slot)
            }

            fn build(&mut self, f: &Fixture) -> Result<Golden, Failure> {
                match f
                    .lifecycle()
                    .build(Installer::Fixture(self), spec(), seed)?
                {
                    BuildResult::Created(golden, _) => Ok(*golden),
                    BuildResult::Existing(_) => panic!("expected a fresh build"),
                }
            }
        }

        impl TemplateRuntime for Recording<'_> {
            fn stage(
                &mut self,
                slots: BuildSlots,
                _spec: &BuildRequest<'_>,
                seed: &[u8],
            ) -> Result<ArtifactEvidence, Failure> {
                self.calls.push("stage");
                assert_eq!(
                    scalar(
                        self.root,
                        "SELECT staging_bytes FROM vm_build_intents WHERE state='pending'"
                    ),
                    5 * MIB as i64
                );
                self.build_slots = Some(slots);
                let label: &[u8] = if self.windows.is_some() {
                    b"ARXSETUP"
                } else {
                    b"CIDATA"
                };
                assert_eq!(&seed[16 * 2048 + 40..][..label.len()], label);
                create(&self.path("seed.iso"), seed);
                if self.fault == Some(Fault::PartialStage) {
                    return Err(Failure::Media);
                }
                create(&self.path("golden.disk"), b"empty fixture disk");
                create(&self.path("golden.vars"), b"firmware fixture");
                if self.fault == Some(Fault::SeedMismatch) {
                    fs::write(self.path("seed.iso"), b"truncated seed").unwrap();
                }
                Ok(file(&self.path("seed.iso"), 7))
            }

            fn install(
                &mut self,
                slots: BuildSlots,
                timeout: Duration,
            ) -> Result<(Boot, StopDisposition), Failure> {
                assert_eq!(Some(slots), self.build_slots);
                assert_eq!(timeout.as_secs(), 3600);
                self.calls.push("installer boot");
                assert!(self.path("seed.iso").exists());
                assert!(self.live.replace(INSTALL).is_none());
                if let Some(f) = self.contend {
                    let mut second_slot = f.runtime();
                    let mut second_spec = spec();
                    second_spec.key = key(3);
                    assert!(matches!(
                        f.lifecycle().build(
                            Installer::Fixture(&mut second_slot),
                            second_spec,
                            |_| panic!("second writer prepared a seed")
                        ),
                        Err(Failure::Refused(Refusal::Busy))
                    ));
                    assert!(second_slot.calls.is_empty());
                }
                fs::write(self.path("golden.disk"), b"installed fixture disk").unwrap();
                self.calls.push("installer release");
                let stop = match self.fault {
                    Some(Fault::InstallerStop(stop)) => stop,
                    _ => StopDisposition::CleanRelease,
                };
                if stop != StopDisposition::ChargedQuarantine {
                    assert_eq!(self.live.take(), Some(INSTALL));
                }
                Ok((INSTALL, stop))
            }

            fn capture_golden(&mut self, slots: PairSlots, images: u64) -> Result<Pair, Failure> {
                assert!(self.live.is_none());
                self.calls.push("capture golden");
                if self.fault == Some(Fault::Capture) {
                    return Err(Failure::Media);
                }
                let pair = Pair {
                    disk: file(&self.slot_path(slots.disk), 1),
                    vars: file(&self.slot_path(slots.vars), 2),
                };
                assert!(pair.fits(images));
                Ok(pair)
            }

            fn link(
                &mut self,
                golden: &Golden,
                slot: LinkedSlot,
                _images: u64,
            ) -> Result<Linked, Failure> {
                assert!(self.live.is_none());
                let (slots, disk_id, vars_id) = match slot {
                    LinkedSlot::Verification(slots) => (slots, 3, 4),
                    LinkedSlot::Clone(_, slots) => {
                        self.clone_count += 1;
                        assert_eq!(
                            scalar(self.root, "SELECT count(*) FROM vm_disk_claims"),
                            i64::from(self.clone_count)
                        );
                        self.clone_slots = Some(slots);
                        let disk = 3 + 2 * self.clone_count;
                        (slots, disk, disk + 1)
                    }
                };
                self.calls.push("link");
                let disk = self.slot_path(slots.disk);
                let vars = self.slot_path(slots.vars);
                let backing_path = self.slot_path(golden.slots.disk);
                // This miniature overlay records the explicit backing name.
                create(&disk, backing_path.to_str().unwrap().as_bytes());
                create(&vars, &fs::read(self.slot_path(golden.slots.vars)).unwrap());
                assert_ne!(
                    self.fault,
                    Some(Fault::CrashAfterCloneWrite),
                    "injected clone interruption"
                );
                let mut pair = Linked {
                    slot,
                    files: Pair {
                        disk: file(&disk, disk_id),
                        vars: file(&vars, vars_id),
                    },
                    backing: file(&backing_path, 1),
                };
                if self.fault == Some(Fault::WrongBacking) {
                    pair.backing = pair.files.disk;
                }
                if self.fault == Some(Fault::SharedVars) {
                    pair.files.vars = golden.files.vars;
                }
                Ok(pair)
            }

            fn boot_detached(&mut self, pair: &Linked) -> Result<Boot, Failure> {
                assert_eq!(
                    pair.slot,
                    LinkedSlot::Verification(self.build_slots.unwrap().verification)
                );
                self.calls.push("detached boot");
                let boot = if self.fault == Some(Fault::ReuseChild) {
                    INSTALL
                } else {
                    DETACHED
                };
                assert!(self.live.replace(boot).is_none());
                fs::write(self.path("verify.vars"), b"verification firmware writes").unwrap();
                if self.fault == Some(Fault::GoldenWrite) {
                    fs::write(self.path("golden.vars"), b"unexpected golden write").unwrap();
                }
                Ok(boot)
            }

            fn ready(&mut self, boot: Boot) -> Result<Readiness, Failure> {
                self.calls.push("ready");
                assert_eq!(self.live, Some(DETACHED));
                assert_eq!(boot, DETACHED);
                Ok(match self.fault {
                    Some(Fault::Readiness(readiness)) => readiness,
                    _ => Readiness::ScriptChannel,
                })
            }

            fn health(
                &mut self,
                boot: Boot,
                script: &str,
                seconds: u32,
            ) -> Result<(ScriptStatus, Option<u32>), Failure> {
                self.calls.push("health");
                assert_eq!(boot, DETACHED);
                assert_eq!(self.live, Some(boot));
                assert_eq!(script, DETACHED_HEALTH);
                assert!(super::super::ubuntu::health_uses_public_paths(script));
                assert!(script.contains("verify_account_result /proc/sys/kernel/random/boot_id /run/arx-account-audit/result"));
                assert_eq!(seconds, 30);
                Ok(match self.fault {
                    Some(Fault::Health(status, exit_code)) => (status, exit_code),
                    _ => (ScriptStatus::Exited, Some(0)),
                })
            }

            fn stop(&mut self, boot: Boot) -> StopDisposition {
                self.calls.push("graceful stop");
                assert_eq!(scalar(self.root, "SELECT count(*) FROM vm_templates"), 0);
                assert_eq!(self.live, Some(boot));
                let stop = match self.fault {
                    Some(Fault::Stop(stop)) => stop,
                    _ => StopDisposition::CleanRelease,
                };
                if stop != StopDisposition::ChargedQuarantine {
                    self.live = None;
                }
                stop
            }

            fn windows_installer(
                &mut self,
                slots: BuildSlots,
                stage: InstallerStage,
            ) -> Result<(Boot, InstallerPhase), Failure> {
                assert_eq!(Some(slots), self.build_slots);
                assert!(self.live.is_none());
                let fixture = self.windows.as_mut().unwrap();
                let (boot, launch, release, bytes) = match stage.phase() {
                    InstallerPhase::WindowsSetup => {
                        assert!(fixture.setup_deadline.replace(stage.deadline()).is_none());
                        (
                            Boot([10; 16]),
                            "setup QMP",
                            "setup release",
                            b"setup fixture".as_slice(),
                        )
                    }
                    InstallerPhase::WindowsFinalize => {
                        assert_eq!(fixture.setup_deadline, Some(stage.deadline()));
                        assert_eq!(self.calls.last(), Some(&"graceful stop"));
                        (
                            Boot([12; 16]),
                            "finalize QMP",
                            "finalize release",
                            b"generalized fixture".as_slice(),
                        )
                    }
                    InstallerPhase::UbuntuInstall => panic!("Windows selected Ubuntu media"),
                };
                self.calls.push(launch);
                let fault = fixture.fault;
                if stage.phase() == InstallerPhase::WindowsSetup
                    && let Some((entered, resume)) = fixture.setup_latch.take()
                {
                    entered.send(stage.deadline()).unwrap();
                    resume.recv().unwrap();
                }
                let (result, observed) = if let Some(WindowsFault::Qmp(phase, reason)) = fault
                    && phase == stage.phase()
                {
                    (Err(reason), StopDisposition::ChargedQuarantine)
                } else {
                    (Ok(()), StopDisposition::CleanRelease)
                };
                let observed_at = tokio::time::Instant::now();
                stage
                    .complete(result, observed, false)
                    .map_err(|report| Failure::Installer {
                        report,
                        observed_at,
                    })?;
                let bytes = if fault == Some(WindowsFault::FailedSysprep)
                    && stage.phase() == InstallerPhase::WindowsFinalize
                {
                    b"still specialized fixture".as_slice()
                } else {
                    bytes
                };
                fs::write(self.path("golden.disk"), bytes).unwrap();
                self.calls.push(release);
                let phase = if fault == Some(WindowsFault::PhaseMismatch) {
                    InstallerPhase::WindowsFinalize
                } else {
                    stage.phase()
                };
                Ok((boot, phase))
            }

            fn windows_offline_setup_state(
                &mut self,
                slots: PairSlots,
                files: Pair,
                deadline: tokio::time::Instant,
            ) -> Result<windows::OfflineSetupState, Failure> {
                assert_eq!(slots, self.build_slots.unwrap().pair);
                assert_eq!(
                    Some(deadline),
                    self.windows.as_ref().unwrap().setup_deadline
                );
                assert_eq!(self.calls.last(), Some(&"capture golden"));
                assert!(self.live.is_none());
                let disk = self.slot_path(slots.disk);
                assert_eq!(files.disk, file(&disk, 1));
                assert_eq!(files.vars, file(&self.slot_path(slots.vars), 2));
                self.calls.push("inspect offline Setup state");
                let state = match fs::read(disk).unwrap().as_slice() {
                    b"generalized fixture" => "IMAGE_STATE_GENERALIZE_RESEAL_TO_OOBE",
                    b"still specialized fixture" => "IMAGE_STATE_COMPLETE",
                    _ => panic!("opened an unexpected disk for offline Setup state"),
                };
                Ok(windows::OfflineSetupState {
                    registry: state.to_owned(),
                    state_ini: state.to_owned(),
                })
            }

            fn windows_installed(
                &mut self,
                target: windows::InstalledTarget,
                deadline: tokio::time::Instant,
            ) -> Result<(Boot, Readiness), Failure> {
                let fixture = self.windows.as_mut().unwrap();
                assert!(deadline <= fixture.setup_deadline.unwrap());
                let slots = self.build_slots.unwrap();
                let (boot, label, ready) = match target {
                    windows::InstalledTarget::Readiness(pair) => {
                        assert_eq!(pair, slots.pair);
                        assert_eq!(self.calls.last(), Some(&"setup release"));
                        let ready = if fixture.fault == Some(WindowsFault::PingOnly) {
                            Readiness::PingOnly
                        } else {
                            Readiness::ScriptChannel
                        };
                        (Boot([11; 16]), "validation boot", ready)
                    }
                    windows::InstalledTarget::Verification(pair) => {
                        assert_eq!(pair, slots.verification);
                        assert_eq!(self.calls.last(), Some(&"link"));
                        let number = if fixture.fault == Some(WindowsFault::ReusedBoot) {
                            12
                        } else {
                            13
                        };
                        (
                            Boot([number; 16]),
                            "verification boot",
                            Readiness::ScriptChannel,
                        )
                    }
                };
                assert!(self.live.replace(boot).is_none());
                fixture.installed_target = Some(target);
                self.calls.push(label);
                Ok((boot, ready))
            }

            fn windows_health(
                &mut self,
                boot: Boot,
                script: &str,
                deadline: tokio::time::Instant,
            ) -> Result<(ScriptStatus, Option<u32>), Failure> {
                let fixture = self.windows.as_mut().unwrap();
                assert_eq!(self.live, Some(boot));
                assert!(deadline <= fixture.setup_deadline.unwrap());
                assert!(deadline <= tokio::time::Instant::now() + Duration::from_secs(30));
                let validation = matches!(
                    fixture.installed_target,
                    Some(windows::InstalledTarget::Readiness(_))
                );
                let phase = if validation {
                    "$validation = $true\n"
                } else {
                    "$validation = $false\n"
                };
                assert!(script.starts_with(phase));
                assert!(script.contains("IMAGE_STATE_COMPLETE"));
                assert!(script.contains("ArxcovoTemplateFinalize"));
                assert!(script.contains("\"2:$boot\""));
                self.calls.push(if validation {
                    "validation health"
                } else {
                    "verification health"
                });
                if !validation && let Some((entered, resume)) = fixture.health_latch.take() {
                    entered.send(deadline).unwrap();
                    resume.recv().unwrap();
                }
                if !validation && self.fault == Some(Fault::GoldenWrite) {
                    fs::write(self.path("golden.vars"), b"unexpected golden write").unwrap();
                }
                let failed = !validation
                    && self.windows.as_ref().unwrap().fault == Some(WindowsFault::NonzeroHealth);
                Ok((ScriptStatus::Exited, Some(u32::from(failed))))
            }

            fn windows_stop(
                &mut self,
                boot: Boot,
                deadline: tokio::time::Instant,
            ) -> StopDisposition {
                let fixture = self.windows.as_ref().unwrap();
                assert_eq!(Some(deadline), fixture.setup_deadline);
                if fixture.fault == Some(WindowsFault::UnprovedStop) {
                    self.calls.push("unproved stop");
                    return StopDisposition::ChargedQuarantine;
                }
                self.stop(boot)
            }

            fn discard_transients(&mut self, slots: BuildSlots) -> Result<(), Failure> {
                assert_eq!(Some(slots), self.build_slots);
                assert!(self.live.is_none());
                assert_eq!(self.calls.last(), Some(&"graceful stop"));
                self.calls.push("discard transients");
                for name in ["seed.iso", "verify.disk", "verify.vars"] {
                    fs::remove_file(self.path(name)).unwrap();
                }
                if self.fault == Some(Fault::RecipeBinding) {
                    db(self.root)
                        .execute(
                            "UPDATE vm_operations SET recipe_digest=?1 WHERE action=11",
                            [[7_u8; 32]],
                        )
                        .unwrap();
                }
                Ok(())
            }
        }

        fn key(nonce: u64) -> RequestId {
            RequestId::parse(&format!("{NOW:016x}{nonce:016x}")).unwrap()
        }

        fn spec() -> BuildRequest<'static> {
            BuildRequest {
                principal: "fixture",
                key: key(2),
                recipe: "e03a3012c18d20b8691490b580ab41e9",
                digest: [9; 32],
                retained: DiskClaim {
                    images: MIB,
                    metadata: MIB,
                    staging: 0,
                },
                transient: DiskClaim {
                    images: MIB,
                    metadata: MIB,
                    staging: MIB,
                },
            }
        }

        fn seed(id: Id) -> Result<Seed, RecipeError> {
            use super::super::AssetPin;
            fn pin(bytes: &[u8]) -> AssetPin {
                AssetPin {
                    length: bytes.len() as u64,
                    sha256: Sha256::digest(bytes).into(),
                }
            }
            let guest = b"fixture guest";
            let recipe = UbuntuRecipe {
                guest: Some(pin(guest)),
                service: Some(pin(UbuntuRecipe::SERVICE)),
                finalize: Some(pin(UbuntuRecipe::FINALIZE)),
                nocloud_schema: Some(UbuntuRecipe::NOCLOUD_PROFILE_PIN),
                ..UbuntuRecipe::default()
            };
            // The fixture uses the same CIDATA encoder without asserting
            // qualification of an owner ISO that the test does not have.
            recipe.render_seed(id, guest).map(Seed::Ubuntu)
        }

        fn clone_request(golden: &Golden) -> Request<'static> {
            Request {
                principal: "fixture",
                request_id: key(1),
                vm: [0; 16],
                operation: Operation::Clone {
                    template: golden.id,
                    lease_seconds: None,
                },
                disk: DiskClaim {
                    images: MIB,
                    metadata: 0,
                    staging: MIB,
                },
            }
        }

        fn windows_seed() -> Seed {
            let recipe = windows::WindowsRecipe {
                pins: Some(windows::tests::fixture()),
            };
            windows::tests::with_assets(|assets, guest| {
                Seed::prepare_windows(&recipe, assets, guest)
            })
            .unwrap()
        }

        fn windows_spec<'a>(recipe: &'a str, seed: &Seed) -> BuildRequest<'a> {
            let Seed::Windows(_, prepared) = seed else {
                panic!("expected validated Windows seed")
            };
            BuildRequest {
                recipe,
                digest: prepared.digest,
                ..spec()
            }
        }

        fn windows_build(f: &Fixture, rt: &mut Recording<'_>) -> Result<BuildResult, Failure> {
            let seed = windows_seed();
            let recipe = hex::encode(windows::catalog_id());
            let request = windows_spec(&recipe, &seed);
            f.lifecycle()
                .build(Installer::Fixture(rt), request, |_| Ok(seed))
        }

        #[test]
        fn windows_catalog_id_binds_the_validated_recipe_and_native_stays_closed() {
            let f = Fixture::new();
            let recipe = hex::encode(windows::catalog_id());
            assert_eq!(recipe, "3c2fe2069942ce6c6cebe5c3e700e758");
            assert_ne!(recipe, spec().recipe);
            let seed = windows_seed();
            let request = windows_spec(&recipe, &seed);
            assert!(matches!(
                f.lifecycle().build(
                    Installer::Candidate(&CandidateRuntime::new(None)),
                    request,
                    |_| panic!("native path prepared seed")
                ),
                Err(Failure::Native(NativeRefusal::InstallerPhaseUnavailable))
            ));
            assert_eq!(
                scalar(f.root.path(), "SELECT count(*) FROM vm_operations"),
                0
            );
            let mut runtime = f.runtime();
            let request = BuildRequest {
                digest: [0; 32],
                ..request
            };
            assert!(matches!(
                f.lifecycle()
                    .build(Installer::Fixture(&mut runtime), request, |_| Ok(seed)),
                Err(Failure::Seed(RecipeError::Unqualified))
            ));
            assert!(runtime.calls.is_empty());
            assert_eq!(staging(&f), 0);
        }

        #[test]
        fn windows_worker_allows_three_hours_plus_preparation_and_settlement() {
            let before = Instant::now();
            let f = Fixture::new();
            let after = Instant::now();
            let BuildAdmission::Fresh(slots, ubuntu, _) = reserve(&f, spec()).unwrap() else {
                panic!("expected Ubuntu worker");
            };
            f.controller.fail_build(&ubuntu, *slots, true).unwrap();
            let recipe = hex::encode(windows::catalog_id());
            let windows = BuildRequest {
                key: key(3),
                ..windows_spec(&recipe, &windows_seed())
            };
            let BuildAdmission::Fresh(_, worker, _) = reserve(&f, windows).unwrap() else {
                panic!("expected Windows worker");
            };
            let ubuntu_budget = Duration::from_secs(3_900);
            let windows_budget = Duration::from_secs(11_100);
            assert!((before + ubuntu_budget..=after + ubuntu_budget).contains(&ubuntu.deadline()));
            assert!(
                (before + windows_budget..=after + windows_budget).contains(&worker.deadline())
            );
        }

        #[tokio::test(start_paused = true)]
        async fn windows_lifecycle_seals_the_generalized_at_rest_pair_and_links_two_stopped_clones()
        {
            let f = Fixture::new();
            let mut runtime = f.runtime();
            runtime.windows = Some(WindowsFixture::default());
            let BuildResult::Created(golden, operation) = windows_build(&f, &mut runtime).unwrap()
            else {
                panic!("expected fresh build")
            };
            assert_eq!(operation.receipt.status, Status::Succeeded);
            assert_eq!(
                runtime.calls,
                [
                    "stage",
                    "setup QMP",
                    "setup release",
                    "validation boot",
                    "validation health",
                    "graceful stop",
                    "finalize QMP",
                    "finalize release",
                    "capture golden",
                    "inspect offline Setup state",
                    "link",
                    "verification boot",
                    "verification health",
                    "graceful stop",
                    "discard transients",
                    "capture golden",
                ]
            );
            assert_eq!(
                fs::read(runtime.path("golden.disk")).unwrap(),
                b"generalized fixture"
            );
            assert_eq!(staging(&f), 0);
            assert!(!runtime.path("verify.vars").exists());
            let mut clones = Vec::new();
            for nonce in [5, 6] {
                let request = Request {
                    request_id: key(nonce),
                    ..clone_request(&golden)
                };
                let CloneResult::Created(clone, _) = f
                    .lifecycle()
                    .linked_clone(&mut runtime, &golden, request)
                    .unwrap()
                else {
                    panic!("expected fresh clone")
                };
                let LinkedSlot::Clone(vm, _) = clone.slot else {
                    panic!("expected clone slot")
                };
                let status = f.controller.get_status("fixture", vm).unwrap().unwrap();
                assert_eq!(status.state, "stopped");
                assert_eq!(status.guest, Some(crate::vm::Guest::WindowsX86_64));
                assert_eq!(clone.backing, golden.files.disk);
                assert_eq!(clone.files.vars.pin, golden.files.vars.pin);
                fs::write(runtime.path("clone.vars"), [nonce as u8]).unwrap();
                clones.push(clone);
            }
            assert_ne!(clones[0].slot, clones[1].slot);
            assert_ne!(clones[0].files.disk.identity, clones[1].files.disk.identity);
            assert_ne!(clones[0].files.vars.identity, clones[1].files.vars.identity);
            assert_eq!(
                runtime.capture_golden(golden.slots, MIB).unwrap(),
                golden.files
            );
        }

        #[tokio::test(start_paused = true)]
        async fn windows_build_cannot_seal_after_phase_or_release_failure() {
            use InstallerFailureReason::{ProcessFailed, ReleaseUnverified, Timeout};
            use WindowsFault::*;
            for (fault, last) in [
                (
                    Qmp(InstallerPhase::WindowsSetup, ProcessFailed),
                    "setup QMP",
                ),
                (PhaseMismatch, "setup release"),
                (Qmp(InstallerPhase::WindowsSetup, Timeout), "setup QMP"),
                (PingOnly, "graceful stop"),
                (UnprovedStop, "unproved stop"),
                (
                    Qmp(InstallerPhase::WindowsFinalize, ReleaseUnverified),
                    "finalize QMP",
                ),
                (ReusedBoot, "graceful stop"),
                (NonzeroHealth, "graceful stop"),
                (FailedSysprep, "inspect offline Setup state"),
            ] {
                let f = Fixture::new();
                let mut runtime = f.runtime();
                runtime.windows = Some(WindowsFixture {
                    fault: Some(fault),
                    ..WindowsFixture::default()
                });
                let error = windows_build(&f, &mut runtime).unwrap_err();
                match (fault, &error) {
                    (Qmp(phase, reason), Failure::Installer { report, .. }) => {
                        assert_eq!((report.phase, report.reason), (phase, reason));
                        assert_eq!(report.recovery, StopDisposition::ChargedQuarantine);
                        assert_eq!(error.native_refusal(), None);
                    }
                    (PhaseMismatch, Failure::Native(NativeRefusal::InstallerPhaseMismatch))
                    | (PingOnly, Failure::Readiness(Readiness::PingOnly))
                    | (UnprovedStop, Failure::Stop(StopDisposition::ChargedQuarantine))
                    | (ReusedBoot, Failure::ReusedBoot)
                    | (NonzeroHealth, Failure::Health)
                    | (FailedSysprep, Failure::GeneralizationUnverified) => {}
                    _ => panic!("unexpected failure for {fault:?}: {error:?}"),
                }
                assert_eq!(runtime.calls.last(), Some(&last));
                assert_eq!(
                    scalar(f.root.path(), "SELECT count(*) FROM vm_templates"),
                    0
                );
                assert_eq!(staging(&f), 5 * MIB as i64);
            }
            let f = Fixture::new();
            let mut runtime = f.runtime();
            runtime.windows = Some(WindowsFixture::default());
            runtime.fault = Some(Fault::GoldenWrite);
            assert!(matches!(
                windows_build(&f, &mut runtime),
                Err(Failure::ChangedPair)
            ));
            assert_eq!(
                scalar(f.root.path(), "SELECT count(*) FROM vm_templates"),
                0
            );
        }

        #[tokio::test(start_paused = true)]
        async fn windows_qmp_cutoff_preserves_quarantine_and_reports_typed_deadline() {
            let (setup_entered, setup_deadline) = tokio::sync::oneshot::channel();
            let (resume_setup, setup_paused) = mpsc::channel();
            let worker = tokio::task::spawn_blocking(move || {
                let f = Fixture::new();
                let mut runtime = f.runtime();
                runtime.windows = Some(WindowsFixture {
                    fault: Some(WindowsFault::Qmp(
                        InstallerPhase::WindowsSetup,
                        InstallerFailureReason::Timeout,
                    )),
                    setup_latch: Some((setup_entered, setup_paused)),
                    ..WindowsFixture::default()
                });
                let error = windows_build(&f, &mut runtime).unwrap_err();
                assert_eq!(
                    error.native_refusal(),
                    Some(NativeRefusal::InstallerDeadlineExpired)
                );
                let Failure::InstallerDeadline(report) = error else {
                    panic!("lost recovery evidence")
                };
                assert_eq!(report.phase, InstallerPhase::WindowsSetup);
                assert_eq!(report.reason, InstallerFailureReason::Timeout);
                assert_eq!(report.recovery, StopDisposition::ChargedQuarantine);
                assert_eq!(runtime.calls, ["stage", "setup QMP"]);
                assert_eq!(staging(&f), 5 * MIB as i64);
                assert_eq!(
                    scalar(f.root.path(), "SELECT count(*) FROM vm_templates"),
                    0
                );
            });
            let deadline = setup_deadline.await.unwrap();
            tokio::time::advance(deadline.saturating_duration_since(tokio::time::Instant::now()))
                .await;
            resume_setup.send(()).unwrap();
            worker.await.unwrap();
        }

        #[tokio::test(start_paused = true)]
        async fn windows_expired_verification_health_still_stops_and_cannot_seal() {
            expired_verification_health(false).await;
            expired_verification_health(true).await;
        }

        async fn expired_verification_health(near_build_deadline: bool) {
            let (setup_entered, setup_deadline) = tokio::sync::oneshot::channel();
            let (resume_setup, setup_paused) = mpsc::channel();
            let (health_entered, health_deadline) = tokio::sync::oneshot::channel();
            let (resume_health, health_paused) = mpsc::channel();
            let worker = tokio::task::spawn_blocking(move || {
                let f = Fixture::new();
                let mut runtime = f.runtime();
                runtime.windows = Some(WindowsFixture {
                    setup_latch: Some((setup_entered, setup_paused)),
                    health_latch: Some((health_entered, health_paused)),
                    ..WindowsFixture::default()
                });
                assert!(matches!(
                    windows_build(&f, &mut runtime),
                    Err(Failure::Native(NativeRefusal::InstallerDeadlineExpired))
                ));
                assert_eq!(runtime.calls.last(), Some(&"graceful stop"));
                assert_eq!(
                    scalar(f.root.path(), "SELECT count(*) FROM vm_templates"),
                    0
                );
            });
            let setup_deadline = setup_deadline.await.unwrap();
            if near_build_deadline {
                let desired = setup_deadline - Duration::from_secs(1);
                tokio::time::advance(
                    desired.saturating_duration_since(tokio::time::Instant::now()),
                )
                .await;
            }
            resume_setup.send(()).unwrap();
            let health_deadline = health_deadline.await.unwrap();
            if near_build_deadline {
                assert_eq!(health_deadline, setup_deadline);
            } else {
                assert_eq!(
                    health_deadline,
                    tokio::time::Instant::now() + Duration::from_secs(30)
                );
            }
            tokio::time::advance(
                health_deadline.saturating_duration_since(tokio::time::Instant::now()),
            )
            .await;
            resume_health.send(()).unwrap();
            worker.await.unwrap();
        }

        #[test]
        fn detached_lifecycle_and_clone_preserve_the_sealed_pair() {
            let f = Fixture::new();
            let mut rt = f.runtime();
            rt.contend = Some(&f);
            let golden = rt.build(&f).unwrap();
            assert_eq!(
                rt.calls,
                [
                    "stage",
                    "installer boot",
                    "installer release",
                    "capture golden",
                    "link",
                    "detached boot",
                    "ready",
                    "health",
                    "graceful stop",
                    "discard transients",
                    "capture golden"
                ]
            );
            let hashes: (Vec<u8>, Vec<u8>) = db(f.root.path())
                .query_row(
                    "SELECT disk_digest,vars_digest FROM vm_templates",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            assert_eq!(hashes.0, golden.files.disk.pin.sha256);
            assert_eq!(hashes.1, golden.files.vars.pin.sha256);
            assert_eq!(staging(&f), 0);
            assert!(!rt.path("seed.iso").exists());
            assert!(!rt.path("verify.disk").exists());
            assert!(!rt.path("verify.vars").exists());

            let CloneResult::Created(clone, _) = f
                .lifecycle()
                .linked_clone(&mut rt, &golden, clone_request(&golden))
                .unwrap()
            else {
                panic!("fresh clone was not created")
            };
            assert_eq!(clone.backing, golden.files.disk);
            assert_eq!(
                fs::read_to_string(rt.path("clone.disk")).unwrap(),
                rt.path("golden.disk").to_str().unwrap()
            );
            assert_ne!(clone.files.vars.identity, golden.files.vars.identity);
            assert_eq!(
                fs::read(rt.path("clone.vars")).unwrap(),
                fs::read(rt.path("golden.vars")).unwrap()
            );
            fs::write(rt.path("clone.disk"), b"clone disk writes").unwrap();
            fs::write(rt.path("clone.vars"), b"clone firmware writes").unwrap();
            assert_eq!(rt.capture_golden(golden.slots, MIB).unwrap(), golden.files);
            let calls = rt.calls.len();
            assert!(matches!(
                f.lifecycle()
                    .linked_clone(&mut rt, &golden, clone_request(&golden)),
                Ok(CloneResult::Existing(OperationRef {
                    receipt: Receipt {
                        status: Status::Succeeded,
                        ..
                    },
                    ..
                }))
            ));
            assert_eq!(rt.calls.len(), calls);
        }

        #[test]
        fn candidate_refuses_installer_with_zero_effects() {
            let f = Fixture::new();
            let candidate = CandidateRuntime::new(None);
            let lifecycle = f.lifecycle();
            let before = fs::read_dir(f.root.path()).unwrap().count();
            assert!(matches!(
                lifecycle.build(Installer::Candidate(&candidate), spec(), |_| panic!(
                    "prepared"
                )),
                Err(Failure::Native(NativeRefusal::InstallerPhaseUnavailable))
            ));
            assert_eq!(
                scalar(f.root.path(), "SELECT count(*) FROM vm_build_intents"),
                0
            );
            assert_eq!(fs::read_dir(f.root.path()).unwrap().count(), before);
            assert_eq!(scalar(f.root.path(), "SELECT count(*) FROM activity"), 0);
        }

        #[test]
        fn peak_refusal_and_failed_seed_preparation_allow_clean_retry() {
            let f = Fixture::new();
            let mut rt = f.runtime();
            let mut lifecycle = f.lifecycle();
            lifecycle.host.capacity.as_mut().unwrap().free_bytes = 50 * GIB + 13 * MIB;
            assert!(matches!(
                lifecycle.build(Installer::Fixture(&mut rt), spec(), |_| panic!("prepared")),
                Err(Failure::Refused(Refusal::Disk))
            ));
            assert!(matches!(
                f.lifecycle().build(
                    Installer::Fixture(&mut rt),
                    BuildRequest {
                        key: key(4),
                        ..spec()
                    },
                    |id| Seed::prepare(
                        &UbuntuRecipe::default(),
                        UbuntuAssets::default(),
                        id,
                        b"guest"
                    )
                ),
                Err(Failure::Seed(RecipeError::Unqualified))
            ));
            assert!(rt.calls.is_empty());
            assert_eq!(staging(&f), 0);
            rt.build(&f).unwrap();
        }

        #[test]
        fn readiness_and_verification_fail_closed() {
            for fault in [
                Fault::ReuseChild,
                Fault::Readiness(Readiness::PingOnly),
                Fault::Readiness(Readiness::Timeout),
                Fault::Readiness(Readiness::WrongPeer),
                Fault::Health(ScriptStatus::Exited, Some(1)),
                Fault::Health(ScriptStatus::TimedOut, None),
                Fault::Stop(StopDisposition::DirtyRelease),
                Fault::Stop(StopDisposition::ChargedQuarantine),
                Fault::GoldenWrite,
                Fault::WrongBacking,
                Fault::SharedVars,
                Fault::InstallerStop(StopDisposition::DirtyRelease),
                Fault::InstallerStop(StopDisposition::ChargedQuarantine),
            ] {
                let f = Fixture::new();
                let mut rt = f.runtime();
                rt.fault = Some(fault);
                let failure = rt.build(&f).unwrap_err();
                match (fault, failure) {
                    (Fault::ReuseChild, Failure::ReusedBoot) => {
                        assert!(!rt.calls.contains(&"ready"))
                    }
                    (Fault::Readiness(expected), Failure::Readiness(actual)) => {
                        assert_eq!(expected, actual);
                        assert!(!rt.calls.contains(&"health"));
                        assert_eq!(rt.calls.last(), Some(&"graceful stop"));
                    }
                    (Fault::Health(..), Failure::Health) => {
                        assert_eq!(rt.calls.last(), Some(&"graceful stop"))
                    }
                    (Fault::Stop(expected), Failure::Stop(actual)) => {
                        assert_eq!(expected, actual);
                        assert!(!rt.calls.contains(&"discard transients"));
                    }
                    (Fault::InstallerStop(expected), Failure::Stop(actual)) => {
                        assert_eq!(expected, actual);
                        assert!(!rt.calls.contains(&"detached boot"));
                    }
                    (Fault::GoldenWrite, Failure::ChangedPair) => {}
                    (Fault::WrongBacking | Fault::SharedVars, Failure::Media) => {
                        assert!(!rt.calls.contains(&"detached boot"));
                    }
                    pair => panic!("unexpected disposition {pair:?}"),
                }
                // The staged failure keeps its proven cause on the receipt.
                let expected = match fault {
                    Fault::Stop(_) | Fault::InstallerStop(_) => FailureReason::DirtyStop,
                    Fault::GoldenWrite | Fault::WrongBacking | Fault::SharedVars => {
                        FailureReason::SealFailed
                    }
                    _ => FailureReason::HealthFailed,
                };
                assert_eq!(
                    scalar(
                        f.root.path(),
                        "SELECT failure_reason FROM vm_operations WHERE action=11"
                    ),
                    expected as i64,
                    "{fault:?}"
                );
                assert_eq!(
                    scalar(f.root.path(), "SELECT count(*) FROM vm_templates"),
                    0
                );
                assert_eq!(staging(&f), 5 * MIB as i64);
            }
        }

        #[test]
        fn interrupted_build_and_seal_rollback_retain_charge_without_publishing() {
            for fault in [
                Some(Fault::PartialStage),
                Some(Fault::SeedMismatch),
                Some(Fault::Capture),
                None,
            ] {
                let f = Fixture::new();
                let mut rt = f.runtime();
                rt.fault = fault;
                if fault.is_none() {
                    // Fail after the template insert, when recording the seal activity.
                    db(f.root.path())
                        .execute_batch(
                            "CREATE TRIGGER abort_seal BEFORE UPDATE ON activity WHEN NEW.action=11
                        BEGIN SELECT RAISE(ABORT, 'fixture seal rollback'); END;",
                        )
                        .unwrap();
                }
                let failure = rt.build(&f).unwrap_err();
                match (fault, failure) {
                    (Some(Fault::PartialStage | Fault::SeedMismatch), Failure::Media) => {
                        assert!(rt.path("seed.iso").exists());
                        assert_eq!(rt.calls, ["stage"]);
                    }
                    (Some(Fault::Capture), Failure::Media) => {
                        assert!(rt.path("golden.disk").exists())
                    }
                    (None, Failure::Store(error)) => {
                        assert!(error.to_string().contains("fixture seal rollback"))
                    }
                    pair => panic!("unexpected interruption {pair:?}"),
                }
                assert_eq!(
                    scalar(f.root.path(), "SELECT count(*) FROM vm_templates"),
                    0
                );
                assert_eq!(staging(&f), 5 * MIB as i64);
                assert!(matches!(
                    f.lifecycle().build(
                        Installer::Fixture(&mut rt),
                        BuildRequest {
                            key: key(3),
                            ..spec()
                        },
                        seed
                    ),
                    Err(Failure::Refused(Refusal::Busy))
                ));
            }
        }

        #[test]
        fn failed_clone_keeps_durable_claim_and_never_replays_writes() {
            let f = Fixture::new();
            let mut rt = f.runtime();
            let golden = rt.build(&f).unwrap();
            rt.fault = Some(Fault::WrongBacking);
            assert!(matches!(
                f.lifecycle()
                    .linked_clone(&mut rt, &golden, clone_request(&golden)),
                Err(Failure::Media)
            ));
            assert_eq!(
                scalar(
                    f.root.path(),
                    "SELECT sum(persistent + staging) FROM vm_disk_claims"
                ),
                (2 * MIB) as i64
            );
            assert_eq!(scalar(f.root.path(), "SELECT state FROM vm_instances"), 4);
            // The worker's known clone failure survives the unproven release.
            assert_eq!(
                scalar(
                    f.root.path(),
                    "SELECT failure_reason FROM vm_operations WHERE action=1"
                ),
                FailureReason::CloneFailed as i64
            );
            assert_eq!(
                scalar(
                    f.root.path(),
                    "SELECT count(*) FROM vm_instances WHERE cleanup_route='staging'"
                ),
                1
            );
            assert!(f.controller.claim_expired().unwrap().is_empty());
            let calls = rt.calls.len();
            assert!(matches!(
                f.lifecycle()
                    .linked_clone(&mut rt, &golden, clone_request(&golden)),
                Ok(CloneResult::Existing(OperationRef {
                    receipt: Receipt {
                        status: Status::Interrupted,
                        ..
                    },
                    ..
                }))
            ));
            assert_eq!(rt.calls.len(), calls);
        }

        #[test]
        fn clone_crash_after_writes_retains_claim_across_reopen() {
            let f = Fixture::new();
            let mut rt = f.runtime();
            let golden = rt.build(&f).unwrap();
            rt.fault = Some(Fault::CrashAfterCloneWrite);
            let interrupted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                f.lifecycle()
                    .linked_clone(&mut rt, &golden, clone_request(&golden))
            }));
            assert!(interrupted.is_err());
            assert!(rt.path("clone.disk").exists());
            assert!(rt.path("clone.vars").exists());
            let Fixture {
                controller, root, ..
            } = f;
            drop(controller);
            let reopened = Fixture {
                controller: VmController::open(
                    root.path(),
                    VmPolicy::default(),
                    TestClock(Instant::now()),
                )
                .unwrap(),
                root,
            };
            assert_eq!(
                scalar(
                    reopened.root.path(),
                    "SELECT sum(persistent + staging) FROM vm_disk_claims"
                ),
                (2 * MIB) as i64
            );
            assert_eq!(
                scalar(reopened.root.path(), "SELECT state FROM vm_instances"),
                4
            );
            let mut fresh = reopened.runtime();
            assert!(matches!(
                reopened
                    .lifecycle()
                    .linked_clone(&mut fresh, &golden, clone_request(&golden)),
                Ok(CloneResult::Existing(OperationRef {
                    receipt: Receipt {
                        status: Status::Interrupted,
                        ..
                    },
                    ..
                }))
            ));
            assert!(fresh.calls.is_empty());
        }

        fn reserve(f: &Fixture, spec: BuildRequest<'_>) -> Result<BuildAdmission, Refusal> {
            let lifecycle = f.lifecycle();
            f.controller
                .admit_build(spec, lifecycle.host, lifecycle.resources, |_, _| {
                    panic!("pending build must never read media")
                })
                .unwrap()
        }

        #[test]
        fn build_publication_refuses_an_operation_recipe_mismatch() {
            let f = Fixture::new();
            let mut rt = f.runtime();
            rt.fault = Some(Fault::RecipeBinding);
            assert!(matches!(rt.build(&f), Err(Failure::Store(error))
                if error.to_string().contains("invalid build publication binding")));
            assert_eq!(staging(&f), 5 * MIB as i64);
            assert_eq!(
                scalar(f.root.path(), "SELECT count(*) FROM vm_templates"),
                0
            );
            assert_eq!(
                scalar(
                    f.root.path(),
                    "SELECT count(*) FROM vm_generations WHERE active=1"
                ),
                0
            );
        }

        #[test]
        fn durable_build_caps_count_retained_and_reserved_charges() {
            let f = Fixture::new();
            let mut lifecycle = f.lifecycle();
            for index in 1..=4 {
                f.controller
                    .seal_template(
                        crate::vm::SealedTemplate {
                            id: [index; 16],
                            guest: crate::vm::Guest::UbuntuX86_64,
                            recipe_digest: [index; 32],
                            disk_digest: [1; 32],
                            vars_digest: [2; 32],
                            disk: DiskClaim {
                                images: 10 * MIB,
                                ..DiskClaim::default()
                            },
                        },
                        lifecycle.host,
                        lifecycle.resources,
                    )
                    .unwrap()
                    .unwrap();
                if index == 1 {
                    let free = &mut lifecycle.host.capacity.as_mut().unwrap().free_bytes;
                    let previous = *free;
                    *free = 50 * GIB + 20 * MIB;
                    let mut rt = f.runtime();
                    assert!(matches!(
                        lifecycle.build(Installer::Fixture(&mut rt), spec(), |_| panic!(
                            "overbilled build"
                        )),
                        Err(Failure::Refused(Refusal::Disk))
                    ));
                    lifecycle.host.capacity.as_mut().unwrap().free_bytes = previous;
                }
            }
            assert_eq!(reserve(&f, spec()).unwrap_err(), Refusal::TemplateLimit);
            assert_eq!(staging(&f), 0);
        }

        #[test]
        fn concurrent_build_replay_keeps_one_writer_and_one_stable_receipt() {
            let f = Fixture::new();
            let (first, second) = std::thread::scope(|scope| {
                let one = scope.spawn(|| reserve(&f, spec()).unwrap());
                let two = scope.spawn(|| reserve(&f, spec()).unwrap());
                (one.join().unwrap(), two.join().unwrap())
            });
            let ((slots, worker), fresh, replay) = match (first, second) {
                (BuildAdmission::Fresh(s, w, op), BuildAdmission::Existing(replay))
                | (BuildAdmission::Existing(replay), BuildAdmission::Fresh(s, w, op)) => {
                    ((*s, w), op, replay)
                }
                _ => panic!("expected one durable writer"),
            };
            assert_eq!(fresh, replay);
            assert_eq!(staging(&f), 5 * MIB as i64);
            assert_eq!(
                reserve(
                    &f,
                    BuildRequest {
                        key: key(9),
                        ..spec()
                    }
                )
                .unwrap_err(),
                Refusal::Busy
            );
            assert_eq!(
                reserve(
                    &f,
                    BuildRequest {
                        digest: [8; 32],
                        ..spec()
                    }
                )
                .unwrap_err(),
                Refusal::KeyCollision
            );
            // The worker proves it never invoked stage, so it may release even
            // though an observer cannot make that determination after restart.
            f.controller.fail_build(&worker, slots, true).unwrap();
            assert_eq!(staging(&f), 0);
            let BuildAdmission::Existing(settled) = reserve(&f, spec()).unwrap() else {
                panic!("terminal receipt must replay")
            };
            assert_eq!(settled.id, fresh.id);
            assert_eq!(settled.receipt.status, Status::Failed);
        }

        #[test]
        fn committed_build_and_clone_replay_and_project_only_to_the_caller() {
            let f = Fixture::new();
            let mut rt = f.runtime();
            let BuildResult::Created(golden, build) = f
                .lifecycle()
                .build(Installer::Fixture(&mut rt), spec(), seed)
                .unwrap()
            else {
                panic!("expected build")
            };
            let calls = rt.calls.len();
            let mut replay_runtime = f.runtime();
            std::thread::scope(|scope| {
                let first = scope.spawn(|| {
                    f.lifecycle()
                        .build(Installer::Fixture(&mut rt), spec(), |_| {
                            panic!("duplicate prepared seed")
                        })
                });
                let second = scope.spawn(|| {
                    f.lifecycle()
                        .build(Installer::Fixture(&mut replay_runtime), spec(), |_| {
                            panic!("duplicate prepared seed")
                        })
                });
                for result in [first.join().unwrap(), second.join().unwrap()] {
                    assert!(
                        matches!(result, Ok(BuildResult::Existing(operation)) if operation == build)
                    );
                }
            });
            assert_eq!(rt.calls.len(), calls);
            assert!(replay_runtime.calls.is_empty());
            let BuildResult::Existing(reuse) = f
                .lifecycle()
                .build(
                    Installer::Fixture(&mut rt),
                    BuildRequest {
                        principal: "other",
                        ..spec()
                    },
                    |_| panic!("sealed recipe rebuilt"),
                )
                .unwrap()
            else {
                panic!("expected sealed reuse")
            };
            assert_ne!(reuse.id, build.id);
            assert_eq!(rt.calls.last(), Some(&"capture golden"));
            let status = |principal, id| f.controller.operation_status(principal, id).unwrap();
            assert_eq!(
                status("fixture", build.id).unwrap().template,
                Some(golden.id)
            );
            assert_eq!(status("other", reuse.id).unwrap().template, Some(golden.id));
            assert_eq!(status("other", build.id), Err(Refusal::NotFound));
            assert_eq!(status("fixture", [0; 16]), Err(Refusal::NotFound));

            let CloneResult::Created(clone, operation) = f
                .lifecycle()
                .linked_clone(&mut rt, &golden, clone_request(&golden))
                .unwrap()
            else {
                panic!("expected clone")
            };
            let LinkedSlot::Clone(vm, _) = clone.slot else {
                panic!("expected clone slot")
            };
            assert_eq!(
                scalar(
                    f.root.path(),
                    "SELECT count(*) FROM vm_artifacts WHERE state='active'"
                ),
                2
            );
            assert_eq!(status("fixture", operation.id).unwrap().vm, Some(vm));
            let mut request = clone_request(&golden);
            request.operation = Operation::Clone {
                template: golden.id,
                lease_seconds: Some(DEFAULT_LEASE_SECONDS),
            };
            assert!(
                matches!(f.lifecycle().linked_clone(&mut rt, &golden, request), Ok(CloneResult::Existing(prior)) if prior == operation)
            );
            let mut request = clone_request(&golden);
            request.operation = Operation::Clone {
                template: golden.id,
                lease_seconds: Some(60),
            };
            assert!(matches!(
                f.lifecycle().linked_clone(&mut rt, &golden, request),
                Err(Failure::Refused(Refusal::KeyCollision))
            ));
            assert_eq!(
                f.controller.get_status("other", vm).unwrap(),
                Err(Refusal::NotFound)
            );
            let listed = f
                .controller
                .list_status("fixture", None, 1)
                .unwrap()
                .unwrap();
            assert_eq!(
                listed,
                [f.controller.get_status("fixture", vm).unwrap().unwrap()]
            );
            assert_eq!(listed[0].state, "stopped");
            assert!(!listed[0].ready);
            assert!(
                f.controller
                    .list_status("fixture", Some(vm), 1)
                    .unwrap()
                    .unwrap()
                    .is_empty()
            );
            assert!(
                f.controller
                    .list_status("other", None, 1)
                    .unwrap()
                    .unwrap()
                    .is_empty()
            );
            db(f.root.path())
                .execute(
                    "DELETE FROM activity WHERE id=?1",
                    [build.receipt.activity_id],
                )
                .unwrap();
            assert_eq!(
                status("fixture", build.id).unwrap().template,
                Some(golden.id)
            );
            db(f.root.path())
                .execute(
                    "UPDATE vm_retry_clock SET high_water_ms=?1",
                    [NOW + RETRY_WINDOW_MS + 1],
                )
                .unwrap();
            assert_eq!(status("fixture", build.id), Err(Refusal::NotFound));
            drop(rt);
            drop(replay_runtime);
            let Fixture { controller, root } = f;
            drop(controller);
            let reopened =
                VmController::open(root.path(), VmPolicy::default(), TestClock(Instant::now()))
                    .unwrap();
            assert_eq!(
                reopened.get_status("fixture", vm).unwrap().unwrap().state,
                "stopped"
            );
            assert_eq!(scalar(root.path(), "SELECT count(*) FROM vm_templates"), 1);
            assert_eq!(
                scalar(
                    root.path(),
                    "SELECT count(*) FROM vm_generations WHERE active=1"
                ),
                2
            );
            assert_eq!(
                scalar(root.path(), "SELECT staging_bytes FROM vm_build_intents"),
                0
            );
        }

        #[test]
        fn reopen_distinguishes_unknown_partial_and_identified_builds() {
            for point in 0..=3 {
                let f = Fixture::new();
                let BuildAdmission::Fresh(slots, worker, operation) = reserve(&f, spec()).unwrap()
                else {
                    panic!("expected build")
                };
                let slots = *slots;
                let mut rt = f.runtime();
                if point > 0 {
                    let seed = seed(slots.id).unwrap();
                    let staged = rt.stage(slots, &spec(), seed.bytes()).unwrap();
                    if point > 1 {
                        let files = rt.capture_golden(slots.pair, MIB).unwrap();
                        let mut entries = vec![
                            (slots.seed, None),
                            (slots.seed, Some(staged)),
                            (slots.pair.disk, None),
                            (slots.pair.vars, None),
                            (slots.pair.disk, Some(files.disk)),
                        ];
                        if point == 3 {
                            let golden = Golden {
                                id: slots.id,
                                slots: slots.pair,
                                files,
                            };
                            let verified = rt
                                .link(&golden, LinkedSlot::Verification(slots.verification), MIB)
                                .unwrap();
                            entries.extend([
                                (slots.pair.vars, Some(files.vars)),
                                (slots.verification.disk, None),
                                (slots.verification.vars, None),
                                (slots.verification.disk, Some(verified.files.disk)),
                                (slots.verification.vars, Some(verified.files.vars)),
                            ]);
                        }
                        f.controller
                            .build_evidence(&worker, slots.id, &entries)
                            .unwrap();
                    }
                }
                drop(rt);
                let Fixture { controller, root } = f;
                drop(controller);
                for _ in 0..2 {
                    let reopened = VmController::open(
                        root.path(),
                        VmPolicy::default(),
                        TestClock(Instant::now()),
                    )
                    .unwrap();
                    assert_eq!(
                        reopened
                            .operation_status("fixture", operation.id)
                            .unwrap()
                            .unwrap()
                            .state,
                        Status::Interrupted
                    );
                    assert_eq!(
                        scalar(root.path(), "SELECT staging_bytes FROM vm_build_intents"),
                        5 * MIB as i64
                    );
                    assert_eq!(scalar(root.path(), "SELECT count(*) FROM vm_templates"), 0);
                    assert_eq!(
                        scalar(
                            root.path(),
                            "SELECT count(*) FROM vm_generations WHERE active=1"
                        ),
                        0
                    );
                    assert_eq!(
                        scalar(
                            root.path(),
                            "SELECT count(*) FROM vm_artifacts WHERE state='pending_disposal'"
                        ),
                        [0, 0, 2, 5][point]
                    );
                    let phase: String = db(root.path())
                        .query_row("SELECT state FROM vm_build_intents", [], |row| row.get(0))
                        .unwrap();
                    assert_eq!(
                        phase,
                        if point == 3 {
                            "pending_disposal"
                        } else {
                            "quarantined"
                        }
                    );
                }
            }
        }

        #[test]
        fn script_ledger_keeps_only_closed_result_and_no_sensitive_bytes() {
            let f = Fixture::new();
            let mut rt = f.runtime();
            let golden = rt.build(&f).unwrap();
            let CloneResult::Created(clone, _) = f
                .lifecycle()
                .linked_clone(&mut rt, &golden, clone_request(&golden))
                .unwrap()
            else {
                panic!("expected clone")
            };
            let LinkedSlot::Clone(vm, _) = clone.slot else {
                panic!("expected clone slot")
            };
            let lifecycle = f.lifecycle();
            let admit = |operation, nonce| {
                let Admission::Admitted(worker) = f
                    .controller
                    .admit(
                        Request {
                            principal: "fixture",
                            request_id: key(nonce),
                            vm,
                            operation,
                            disk: DiskClaim::default(),
                        },
                        lifecycle.host,
                        lifecycle.resources,
                    )
                    .unwrap()
                else {
                    panic!("expected worker")
                };
                worker
            };
            f.controller
                .finish(&admit(Operation::Start, 6), Completion::Succeeded)
                .unwrap();
            let script = "script-sentinel token-sentinel P:/path-sentinel S-1-5-21-987654321";
            let worker = admit(
                Operation::Script {
                    script,
                    timeout_seconds: 30,
                },
                7,
            );
            assert_eq!(
                f.controller
                    .finish_script(&worker, ScriptResult::Exited(7))
                    .unwrap(),
                Status::Failed
            );
            let operation: Id = db(f.root.path())
                .query_row(
                    "SELECT operation_id FROM vm_operations WHERE action=2",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(
                f.controller
                    .operation_status("fixture", operation)
                    .unwrap()
                    .unwrap()
                    .script,
                Some(ScriptResult::Exited(7))
            );
            let contents = fs::read(f.root.path().join("activity.sqlite3")).unwrap();
            for sentinel in script.split_ascii_whitespace() {
                assert!(
                    !contents
                        .windows(sentinel.len())
                        .any(|window| window == sentinel.as_bytes())
                );
            }
        }

        fn staging(f: &Fixture) -> i64 {
            scalar(
                f.root.path(),
                "SELECT coalesce(sum(staging_bytes),0) FROM vm_build_intents",
            )
        }
    }
}
