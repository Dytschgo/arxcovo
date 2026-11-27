//! Synthetic launch and helper peers over the foreground's real TEST ledger.

use super::*;
use crate::store::vm::{native::Origin, snapshot::Work};
use crate::vm::{
    Capacity, Clock, ClockReading, HostObservation, PendingOperation, SealedGolden, VmController,
    paths::{Artifact, ArtifactPin},
    qualification_record::{Class, Hash, TestRecipes},
    selection_epoch::{Invalidation, Sample},
    test_issuer::{NativeTarget, SelectedSource, TestIssuer, retry_contended, tests::FakeSource},
};
use clone::{CloneRequest, HeldGolden, OverlayHelper, stage_linked_clone};
use snapshot::{ControllerSnapshot, Holds, SnapshotRequest, copy_offline_snapshot};
use std::path::Path;

const IMAGE_SIZE: u64 = 64 << 20;
const PATH: NativeRefusal = NativeRefusal::PathCustodyUnverified;

pub(crate) struct FixtureQualification {
    pub(super) production: PathPin,
    owner: Vec<usize>,
    pub(super) cwd: LocalPath,
    _assets: tempfile::TempDir,
    selection: FakeSource,
    pub(crate) busy: std::sync::atomic::AtomicBool,
    /// Holds a synthetic guest stop so a test can act while the owner is
    /// still tearing down.
    pub(crate) hold_stop: std::sync::atomic::AtomicBool,
}

impl FixtureQualification {
    /// Gives objects this process creates from now on the token user as
    /// owner, as the service does before creating any object
    /// (`set_current_token_owner`). Production custody, `StorePins::open`
    /// and `NativeRecovery` expect that owner. An elevated runner otherwise
    /// stamps new objects with its default owner, Administrators, and each
    /// check then refuses the workspace. The call is a no-op where
    /// TokenOwner is already TokenUser. nextest runs each test in its own
    /// process, so the change never reaches another test.
    pub(crate) fn create_as_token_user() {
        crate::service::windows::set_current_token_owner().expect("TokenOwner = TokenUser");
        let token = process_token(unsafe { GetCurrentProcess() }, TOKEN_QUERY).unwrap();
        let owner = token_data(&token, TokenOwner).unwrap();
        let user = token_data(&token, TokenUser).unwrap();
        // Both buffers stay alive across the comparison of their interior SIDs.
        let same = unsafe {
            windows_sys::Win32::Security::EqualSid(
                (*owner.as_ptr().cast::<TOKEN_OWNER>()).Owner,
                user_sid(&user),
            )
        };
        assert_ne!(same, 0, "new objects must get the token user as owner");
    }

    pub(crate) fn open(root: &Path, now: Instant) -> Self {
        Self::create_as_token_user();
        let production =
            PathPin::open(&local(root.to_str().unwrap()).unwrap(), true, false).unwrap();
        let assets = tempfile::tempdir().unwrap();
        let cwd = local(assets.path().to_str().unwrap()).unwrap();
        std::fs::write(assets.path().join("qemu-img.exe"), b"synthetic helper").unwrap();
        Self {
            production,
            owner: current_user().unwrap(),
            cwd,
            _assets: assets,
            selection: FakeSource::new(now),
            busy: true.into(),
            hold_stop: false.into(),
        }
    }

    pub(super) fn owner(&self) -> PSID {
        user_sid(&self.owner)
    }

    fn image(&self) -> Result<Artifact, NativeRefusal> {
        let path = self.cwd.join("qemu-img.exe").map_err(|_| PATH)?;
        launch::artifact(ArtifactRole::QemuImg, &PathPin::open(&path, false, false)?)
    }

    pub(crate) fn test_host(&self) -> Result<HostObservation, NativeRefusal> {
        if self.busy.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(NativeRefusal::OwnerActivityUnverified);
        }
        Ok(HostObservation {
            owner_idle: Some(true),
            logical_cpus: Some(8),
            available_ram: Some(32 * GIB),
            capacity: Some(Capacity {
                free_bytes: 400 * GIB,
                managed_bytes: 0,
            }),
        })
    }

    fn hold_golden<C: Clock>(
        &self,
        controller: &VmController<C>,
        ledger: &LedgerCheckpoints<'_>,
        golden: &SealedGolden,
        cutoff: Instant,
        cancelled: &dyn Fn() -> bool,
        revoked: &dyn Fn() -> bool,
    ) -> Result<HeldGolden, NativeRefusal> {
        let binding = retry_contended(
            cancelled,
            revoked,
            || anyhow::anyhow!("TEST qualification missing"),
            || controller.native_workspace_scope(golden.slots().disk.generation),
        )
        .map_err(|_| PATH)?;
        HeldGolden::hold(
            &self.production,
            self.owner(),
            ledger,
            binding_from_scope(binding),
            golden,
            cutoff,
            cancelled,
        )
    }

    /// The production build-reuse check: custody without the content hash.
    pub(crate) fn confirm_golden<C: Clock>(
        &self,
        controller: &VmController<C>,
        golden: &SealedGolden,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), NativeRefusal> {
        let ledger = LedgerCheckpoints {
            store: controller.native_store(),
            origin: Origin::Clone,
        };
        let binding = controller
            .native_workspace_scope(golden.slots().disk.generation)
            .map_err(|_| PATH)?;
        HeldGolden::identify(
            &self.production,
            self.owner(),
            &ledger,
            binding_from_scope(binding),
            golden,
            Instant::now() + clone::GOLDEN_CUSTODY_BUDGET,
            cancelled,
        )?;
        Ok(())
    }

    pub(crate) async fn clone_test<S: SelectedSource, C: Clock>(
        &self,
        controller: &VmController<C>,
        issuer: &TestIssuer<'_, S, C>,
        worker: PendingOperation,
        slots: PairSlots,
        cutoff: Instant,
        cancel: &Shutdown,
    ) -> anyhow::Result<()> {
        let cancelled = || cancel.is_cancelled();
        let revoked = || issuer.epoch().effects_revoked();
        // As in production, a refusal before staging settles the clone.
        let prepared = (|| {
            let intent = issuer.intent(
                &worker,
                NativeTarget {
                    generation: slots.disk.generation,
                    writer_epoch: None,
                },
                cancel,
            )?;
            Ok::<_, NativeRefusal>((intent, self.image()?))
        })();
        let Ok((intent, image)) = prepared else {
            controller.finish_native_clone(
                &worker,
                slots,
                Err(crate::vm::FailureReason::CloneFailed),
                crate::vm::ClonePublication {
                    golden: None,
                    effect_cutoff: cutoff,
                    cancelled: &cancelled,
                },
            )?;
            anyhow::bail!("fixture clone preparation refused");
        };
        let store = controller.native_store();
        let ledger = LedgerCheckpoints {
            store,
            origin: Origin::Clone,
        };
        let helper = SimulatedOverlay {
            create: |prepared: &helper::PreparedHelper<'_, '_>, plan: &CommandPlan<'_>| {
                let scope = prepared.workspace().binding();
                let target = || prepared.launch_target(plan, &self.cwd);
                let permit = intent.launch(scope, target())?;
                consume_checked(permit, target(), scope, self.seal_sha256())?;
                self.record_creation(store, scope, scope.generation, &cancelled, &revoked)?;
                Ok(())
            },
        };
        let status = stage_linked_clone(
            || {
                controller
                    .admitted_sealed_golden(&worker)
                    .ok()
                    .flatten()
                    .ok_or(PATH)
            },
            |golden| self.hold_golden(controller, &ledger, golden, cutoff, &cancelled, &revoked),
            || {
                reserve_workspace_under(
                    &self.production,
                    self.owner(),
                    slots.disk.generation,
                    &ledger,
                )
            },
            &ledger,
            &CloneRequest {
                slots,
                size: IMAGE_SIZE,
                worker_deadline: worker.deadline(),
                effect_cutoff: cutoff,
                owner: self.owner(),
                cancelled: &cancelled,
            },
            &image,
            helper,
            |slot, pin| intent.copy(slot, pin)?.consume(slot, pin),
            |outcome, gate| controller.finish_native_clone(&worker, slots, outcome, gate),
        )
        .await?;
        anyhow::ensure!(
            status == crate::vm::Status::Succeeded,
            "fixture clone settlement"
        );
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn snapshot_test<S: SelectedSource, C: Clock>(
        &self,
        controller: &VmController<C>,
        issuer: &TestIssuer<'_, S, C>,
        worker: PendingOperation,
        work: Work,
        cutoff: Instant,
        cancel: &Shutdown,
        runtime: &tokio::runtime::Handle,
    ) -> anyhow::Result<()> {
        let slots = work.candidate.expect("admitted copy");
        // As in production, a refusal before the copy settles it untouched.
        let Ok(intent) = issuer.intent(
            &worker,
            NativeTarget {
                generation: slots.disk.generation,
                writer_epoch: None,
            },
            cancel,
        ) else {
            return controller.fail_snapshot_copy(
                &worker,
                &work,
                [crate::store::vm::snapshot::StagedLeaf::Untouched; 2],
            );
        };
        let store = controller.native_store();
        let custody = LedgerCheckpoints {
            store,
            origin: Origin::Restore,
        };
        let disposal = LedgerDisposal { store };
        let request = SnapshotRequest {
            work: &work,
            worker: &worker,
            production: &self.production,
            owner: self.owner(),
            firmware: ArtifactPin {
                size: 1,
                sha256: [1; 32],
            },
            effect_cutoff: cutoff,
            cancelled: &|| cancel.is_cancelled(),
        };
        // Fail closed like production when the owner becomes active.
        let host = || {
            self.test_host().unwrap_or(HostObservation {
                owner_idle: None,
                logical_cpus: None,
                available_ram: None,
                capacity: None,
            })
        };
        let ledger = ControllerSnapshot {
            controller,
            worker: &worker,
            work: &work,
            host: &host,
            resources: selected::TEST_RESOURCES,
        };
        let image = self.image().expect("fixture image");
        let check = |target: &snapshot::InspectTarget<'_>,
                     info: &crate::vm::qemu_args::SnapshotInfo<'_>,
                     _: &crate::vm::snapshot::CopyBudget<'_>| {
            let workspace = target.workspace;
            let lease = crate::vm::runtime::OfflineLease::for_inspection(
                workspace,
                target.slot,
                target.identity,
            );
            let plan = crate::vm::qemu_args::offline_snapshot_info(&lease, &image, info).unwrap();
            let scope = workspace.binding();
            let launch_target = || snapshot::inspection_launch_target(&plan, workspace, &self.cwd);
            let permit = intent.launch(scope, launch_target()).unwrap();
            consume_checked(permit, launch_target(), scope, self.seal_sha256()).unwrap();
            runtime.block_on(async { tokio::time::sleep(Duration::from_millis(1)).await });
        };
        copy_offline_snapshot(
            &request,
            || {
                Holds::hold(
                    &self.production,
                    self.owner(),
                    &custody,
                    &disposal,
                    &work,
                    cutoff.min(worker.deadline()),
                    request.cancelled,
                )
            },
            || {
                reserve_workspace_under(
                    &self.production,
                    self.owner(),
                    slots.disk.generation,
                    &custody,
                )
            },
            &custody,
            |slot, pin| intent.copy(slot, pin)?.consume(slot, pin),
            &mut snapshot::tests::FixtureInspector::checking(&check),
            &ledger,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn hold_run<C: Clock>(
        &self,
        controller: &VmController<C>,
        generation: Id,
        epoch: Id,
        pair: StartPair,
        cutoff: Instant,
        cancelled: &dyn Fn() -> bool,
        revoked: &dyn Fn() -> bool,
    ) -> anyhow::Result<ProductionWorkspace> {
        let binding = binding_from_scope(retry_contended(
            cancelled,
            revoked,
            || anyhow::anyhow!("TEST qualification missing"),
            || controller.native_workspace_scope(generation),
        )?);
        let workspace = hold_workspace_under(&self.production, self.owner(), binding)
            .map_err(|_| anyhow::anyhow!("fixture run hold"))?;
        workspace
            .check_start_pair(pair, cutoff, &|| false)
            .map_err(|_| anyhow::anyhow!("fixture start pair"))?;
        self.record_creation(
            controller.native_store(),
            binding,
            epoch,
            cancelled,
            revoked,
        )
        .map_err(|_| anyhow::anyhow!("fixture creation checkpoint"))?;
        Ok(workspace)
    }

    pub(crate) fn release_run<'a>(
        &self,
        workspace: &'a ProductionWorkspace,
        pair: StartPair,
    ) -> ReleasedGeneration<'a> {
        let pins = [
            workspace
                .pin_slot(pair.slots.disk, pair.evidence[0].identity, false)
                .unwrap(),
            workspace
                .pin_slot(pair.slots.vars, pair.evidence[1].identity, false)
                .unwrap(),
        ];
        let exclusions = pins
            .iter()
            .map(|pin| pin.reopen_without_writer().unwrap())
            .collect();
        let mut proof = ReleasedGeneration::checked(
            workspace.binding(),
            ReleaseTargets::Pair {
                pair: pair.slots,
                ids: pair.evidence.map(|file| file.identity),
            },
            WorkspaceCustody::production(&workspace.root, &workspace.lease).unwrap(),
            (pins.into(), exclusions),
        )
        .unwrap();
        proof.stop_disposition = Some(StopDisposition::CleanRelease);
        proof
    }

    fn record_creation(
        &self,
        store: &crate::store::vm::VmStore,
        binding: WorkspaceBinding,
        epoch: Id,
        cancelled: &dyn Fn() -> bool,
        revoked: &dyn Fn() -> bool,
    ) -> Result<(), NativeRefusal> {
        retry_contended(
            cancelled,
            revoked,
            || NativeRefusal::QualificationMissing,
            || {
                let origin =
                    recovery::StorePins::open(&self.production)?.origin(store, boot::marker()?)?;
                store
                    .try_native_transaction(|db| {
                        crate::store::vm::native::recovery::record_job(
                            db,
                            custody::scope(binding),
                            epoch,
                            origin,
                        )
                    })
                    .map_err(|_| PATH)
            },
        )
    }
}

impl SelectedSource for FixtureQualification {
    fn seal_sha256(&self) -> Hash {
        (&self.selection).seal_sha256()
    }
    fn class(&self) -> Class {
        (&self.selection).class()
    }
    fn scope(&self) -> Hash {
        (&self.selection).scope()
    }
    fn recipes(&self) -> TestRecipes {
        (&self.selection).recipes()
    }
    fn owner_matches(&self, configured: &str) -> bool {
        (&self.selection).owner_matches(configured)
    }
    fn sample(
        &self,
        advance: &dyn Fn() -> Result<ClockReading, Invalidation>,
    ) -> Result<Sample, Invalidation> {
        (&self.selection).sample(advance)
    }
}

struct SimulatedOverlay<F> {
    create: F,
}

impl<F> OverlayHelper for SimulatedOverlay<F>
where
    F: FnOnce(&helper::PreparedHelper<'_, '_>, &CommandPlan<'_>) -> Result<(), NativeRefusal>,
{
    async fn create<'a>(
        self,
        prepared: &'a helper::PreparedHelper<'a, 'a>,
        plan: CommandPlan<'a>,
    ) -> HelperSettlement<'a>
    where
        Self: 'a,
    {
        if let Err(reason) = (self.create)(prepared, &plan) {
            return HelperSettlement::quarantined(Err(reason));
        }
        let inputs = plan
            .inputs()
            .iter()
            .map(|(role, file)| (*role, file.identity().unwrap()))
            .collect();
        let (_, backing) = plan
            .inputs()
            .iter()
            .find(|(role, _)| *role == ArtifactRole::Golden)
            .unwrap();
        let name = backing.path_for(ArtifactRole::Golden).unwrap().as_str();
        let path = slot_path(&prepared.workspace().root.path, prepared.slot()).unwrap();
        std::fs::write(path.as_str(), clone::tests::backed_header(IMAGE_SIZE, name)).unwrap();
        let proof = clone::tests::held_release(
            prepared.workspace(),
            prepared.slot(),
            prepared.identity(),
            inputs,
        );
        HelperSettlement {
            output: Ok(Vec::new()),
            disposition: StopDisposition::CleanRelease,
            released: Some(proof),
        }
    }
}
