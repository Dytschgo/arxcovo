//! Native static effects have their own recipe selection and persistent custody.
use super::super::{
    docker_trial::{native::duplicate, witness},
    static_compose::{StaticDossier, StockBootInputs},
};
use super::*;
use crate::store::vm::{self as store, native};
use crate::vm::{
    BuildRequest, BuildSlots, Clock, FailureReason, PendingOperation, VmController,
    paths::{Artifact, ArtifactPin},
    qemu_args::{BootPurpose, BootRecipe, SerialName, StaticNetwork},
    runtime::{InstallerBudget, InstallerPhase, InstallerRecipe},
    static_web::OriginPortLease,
};
use std::io::{Read, Seek, SeekFrom, Write};
use tokio::{net::windows::named_pipe::NamedPipeClient, time::Instant as Deadline};

const PATH: NativeRefusal = NativeRefusal::PathCustodyUnverified;
const ENDPOINT: NativeRefusal = NativeRefusal::GuestEndpointUnverified;
const QMP: NativeRefusal = NativeRefusal::QmpUnverified;
const JOB: NativeRefusal = NativeRefusal::JobUnverified;
const QUALIFICATION: NativeRefusal = NativeRefusal::QualificationMissing;

const DISK_BYTES: u64 = 32 * GIB;
const DISK_LIMIT: u64 = 33 * GIB;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum StaticRecipe {
    Web,
    Compose,
}

impl StaticRecipe {
    pub(crate) fn identity(
        self,
        dossier: &StaticDossier,
    ) -> crate::vm::qualification_record::RecipeIdentity {
        let recipe = dossier.evidence.recipe();
        match self {
            Self::Web => crate::vm::qualification_record::RecipeIdentity {
                recipe_id: crate::vm::WEB_RECIPE_ID,
                recipe_sha256: recipe.web,
            },
            Self::Compose => recipe.identity,
        }
    }
}

pub(crate) struct StaticMachine<'a> {
    pub m4: &'a ForegroundQualification,
    pub dossier: &'a StaticDossier,
    pub cancel: &'a Shutdown,
}

type Pair = (PairSlots, [ArtifactEvidence; 2]);

impl StaticMachine<'_> {
    pub(crate) fn require_current(&self) -> Result<(), NativeRefusal> {
        self.m4.fresh().map_err(NativeRefusal::from)?;
        self.dossier.require_current()
    }

    pub(crate) fn live(&self) -> Result<(Instant, crate::vm::HostObservation), NativeRefusal> {
        self.dossier.require_current()?;
        self.m4.observe_host(TEST_JOB_MEMORY)
    }

    fn check(&self, deadline: Instant) -> Result<Instant, NativeRefusal> {
        launch::check_preparation(deadline, &|| self.cancel.is_cancelled())?;
        self.live().map(|(captured, _)| captured)
    }

    fn reserve_writer(&self, workspace: &ProductionWorkspace) -> Result<(), NativeRefusal> {
        let store = &self.m4.store;
        let origin =
            recovery::StorePins::open(self.m4.production_root())?.origin(store, boot::marker()?)?;
        let scope = custody::scope(workspace.binding());
        store
            .transact(|db| native::recovery::record_job(db, scope, scope.generation, origin))
            .map_err(|_| PATH)
    }

    fn launch<'a>(
        &'a self,
        workspace: &'a ProductionWorkspace,
        plan: CommandPlan<'_>,
        pins: Vec<PathPin>,
        installer: Option<InstallerStage>,
        deadline: Instant,
    ) -> Result<(CheckedLaunchCore<'a>, BootIo), NativeRefusal> {
        let cwd = self.m4.command_cwd()?;
        let bundle = if plan.kind() == PlanKind::PublicEgressBoot {
            &self.dossier.patched.bundle
        } else {
            &self.dossier.stock
        };
        let inputs = LaunchInputs {
            plan,
            cwd: &cwd,
            workspace: WorkspaceCustody::production(&workspace.root, &workspace.lease)?,
            bundle,
            memory: TEST_JOB_MEMORY,
            cpu_rate: JOB_CPU_RATE,
            installer,
            finalization_slots: None,
            pins,
        };
        let (core, io, ()) = CheckedLaunchCore::hold_checked_for(
            &self.m4.held.token,
            inputs,
            |_| Ok((self.check(deadline)?, ())),
            || self.check(deadline).map(|_| ()),
            true,
        )
        .map_err(|failure| failure.reason)?;
        Ok((core, io))
    }

    fn loader(
        &self,
        patched: bool,
        deadline: Instant,
    ) -> Result<[(ArtifactRole, PathPin); 3], NativeRefusal> {
        use ArtifactRole::*;
        let recipe = &self.dossier.recipe;
        let stock_firmware = recipe.firmware[0].verified_content.ok_or(PATH)?;
        if !patched {
            let stock = &self.dossier.stock;
            let held = StockBootInputs::hold(
                stock,
                self.dossier.stock_inventory,
                stock_firmware,
                deadline,
            )?;
            let [a, b, c] = held
                .roles()
                .map(|(role, pin)| duplicate(pin).map(|pin| (role, pin)));
            return Ok([a?, b?, c?]);
        }
        let patched = &self.dossier.patched;
        let bundle = &patched.bundle;
        let image = launch::stock_asset(
            bundle,
            "qemu-system-x86_64.exe",
            patched.review.qemu.size,
            deadline,
        )?;
        let data = PathPin::open(
            &bundle.root.path.join("share").map_err(|_| PATH)?,
            true,
            false,
        )?;
        let code = launch::stock_asset(
            bundle,
            "share/edk2-x86_64-code.fd",
            patched.review.firmware_code.size,
            deadline,
        )?;
        let vars = launch::stock_asset(
            bundle,
            "share/edk2-i386-vars.fd",
            patched.review.firmware_vars.size,
            deadline,
        )?;
        if image.verified_content != Some(patched.review.qemu)
            || code.verified_content != Some(patched.review.firmware_code)
            || code.verified_content != Some(stock_firmware)
            || vars.verified_content != Some(patched.review.firmware_vars)
            || vars.verified_content != recipe.firmware[1].verified_content
        {
            return Err(NativeRefusal::DistributionUnverified);
        }
        Ok([
            (PublicEgressQemu, image),
            (Data, data),
            (FirmwareCode, code),
        ])
    }

    fn verify_disk(pin: &PathPin) -> Result<(), NativeRefusal> {
        if crate::vm::qemu_img::inspect_header(&launch::header(pin)?, None) != Ok(DISK_BYTES)
            || !pin
                .leaf
                .metadata()
                .is_ok_and(|file| file.len() <= DISK_LIMIT)
        {
            return Err(PATH);
        }
        Ok(())
    }

    fn private_pin(
        &self,
        workspace: &ProductionWorkspace,
        slot: ArtifactSlot,
        identity: FileIdentity,
        mutable: bool,
    ) -> Result<PathPin, NativeRefusal> {
        let pin = workspace.pin_slot(slot, identity, mutable)?;
        if !has_exact_leaf(&pin.leaf, &disposal::leaf_name(slot))
            || !sink::held_private(&pin.leaf, self.m4.owner())
        {
            return Err(PATH);
        }
        Ok(pin)
    }

    fn evidence(
        workspace: &ProductionWorkspace,
        pin: &PathPin,
        guard: &File,
        limit: u64,
        deadline: Instant,
        stopped: &dyn Fn() -> bool,
    ) -> Result<ArtifactEvidence, NativeRefusal> {
        if !file_identity(guard, false).is_ok_and(|(id, _)| id == pin.identity) {
            return Err(PATH);
        }
        Ok(ArtifactEvidence {
            identity: pin.identity,
            directory: workspace.binding().workspace.file,
            pin: launch::hash_held(guard, limit, deadline, stopped)?,
        })
    }

    fn hold_golden(
        &self,
        golden: &crate::vm::SealedGolden,
    ) -> Result<(ProductionWorkspace, [PathPin; 2]), NativeRefusal> {
        let store = &self.m4.store;
        let scope = store
            .read_only(|db| native::disposal::generation_state(db, golden.slots().disk.generation))
            .map_err(|_| PATH)?;
        let native::disposal::GenerationState::Bound { scope, .. } = scope else {
            return Err(PATH);
        };
        let workspace = self
            .m4
            .hold_start_workspace(disposal::binding_from_scope(scope))?;
        let [disk, vars] = golden.evidence();
        let [disk_slot, vars_slot] = [golden.slots().disk, golden.slots().vars];
        let pins = [
            self.private_pin(&workspace, disk_slot, disk.identity, false)?,
            self.private_pin(&workspace, vars_slot, vars.identity, false)?,
        ];
        Ok((workspace, pins))
    }

    pub(crate) fn check_golden(&self, golden: &crate::vm::SealedGolden) -> bool {
        self.hold_golden(golden).is_ok()
    }

    fn copy_to(
        &self,
        source: &File,
        size: u64,
        deadline: Instant,
        mut write: impl FnMut(&[u8]) -> Result<(), NativeRefusal>,
    ) -> Result<(), NativeRefusal> {
        let mut source = source;
        source.seek(SeekFrom::Start(0)).map_err(|_| PATH)?;
        let mut buffer = vec![0_u8; 256 << 10];
        let mut remaining = size;
        while remaining > 0 {
            launch::check_preparation(deadline, &|| {
                self.cancel.is_cancelled() || self.dossier.watch.require_current().is_err()
            })?;
            let n = remaining.min(buffer.len() as u64) as usize;
            source.read_exact(&mut buffer[..n]).map_err(|_| PATH)?;
            write(&buffer[..n])?;
            remaining -= n as u64;
        }
        if source.read(&mut [0]).map_err(|_| PATH)? != 0 {
            return Err(PATH);
        }
        Ok(())
    }

    fn new_static_workspace(&self, generation: Id) -> Result<ProductionWorkspace, NativeRefusal> {
        let production = self.m4.production_root();
        let name = hex::encode(generation);
        let dir = open_relative(&production.leaf, &name, Node::Directory, true, true)
            .map_err(|_| PATH)?;
        let lease =
            open_relative(&dir, GENERATION_LEASE, Node::File, true, true).map_err(|_| PATH)?;
        lease.sync_all().map_err(|_| PATH)?;
        let binding = WorkspaceBinding {
            generation,
            production: production.identity,
            workspace: file_identity(&dir, true).map_err(|_| PATH)?.0,
            lease: file_identity(&lease, false).map_err(|_| PATH)?.0,
        };
        drop(lease);
        drop(dir);
        self.m4.hold_start_workspace(binding)
    }

    pub(crate) fn copy<C: Clock>(
        &self,
        controller: &VmController<C>,
        worker: &PendingOperation,
        slots: PairSlots,
        golden: &crate::vm::SealedGolden,
    ) -> anyhow::Result<()> {
        let deadline = worker.deadline();
        let cancelled =
            || self.cancel.is_cancelled() || self.dossier.watch.require_current().is_err();
        let mut touched = false;
        let mut effect = || -> Result<_, NativeRefusal> {
            self.check(deadline)?;
            let (source_workspace, pins) = self.hold_golden(golden)?;
            Self::verify_disk(&pins[0])?;
            let sources = pins
                .each_ref()
                .map(|pin| pin.reopen_without_writer().map_err(|_| PATH));
            let [source_disk, source_vars] = sources;
            let sources = [source_disk?, source_vars?];
            touched = true;
            let workspace = self.new_static_workspace(slots.disk.generation)?;
            let copy = |index: usize, slot: ArtifactSlot, limit| {
                self.check(deadline)?;
                let source = &sources[index];
                let expected = golden.evidence()[index].pin;
                if launch::hash_held(source, limit, deadline, &cancelled)? != expected {
                    return Err(PATH);
                }
                let name = disposal::leaf_name(slot);
                let file = open_relative(&workspace.root.leaf, &name, Node::File, true, true)
                    .map_err(|_| PATH)?;
                let id = file_identity(&file, false).map_err(|_| PATH)?.0;
                let bridge = self.private_pin(&workspace, slot, id, true)?;
                let mut sink = &file;
                self.copy_to(source, expected.size, deadline, |bytes| {
                    sink.write_all(bytes).map_err(|_| PATH)
                })?;
                file.sync_all().map_err(|_| PATH)?;
                drop(file);
                let guard = bridge.reopen_without_writer().map_err(|_| PATH)?;
                let evidence =
                    Self::evidence(&workspace, &bridge, &guard, limit, deadline, &cancelled)?;
                if evidence.pin != expected || evidence.identity == pins[index].identity {
                    return Err(PATH);
                }
                Ok((bridge, guard, evidence))
            };
            let (disk, disk_guard, disk_evidence) = copy(0, slots.disk, DISK_LIMIT)?;
            let (_vars, vars_guard, vars_evidence) = copy(1, slots.vars, MAX_SETUP_VARS_BYTES)?;
            Self::verify_disk(&disk)?;
            Ok((
                workspace,
                [disk_guard, vars_guard],
                [disk_evidence, vars_evidence],
                (source_workspace, pins, sources),
            ))
        };
        let result = match effect() {
            Ok((workspace, _guards, evidence, _source)) => controller.commit_static_copy(
                worker,
                slots,
                custody::scope(workspace.binding()),
                evidence,
                || !cancelled() && self.dossier.require_current().is_ok(),
            ),
            Err(reason) => Err(anyhow::anyhow!("static copy refused: {reason:?}")),
        };
        if result.is_err() {
            settle_copy_failure(controller, worker, slots, touched)?;
        }
        Ok(())
    }
}

fn settle_copy_failure<C: Clock>(
    controller: &VmController<C>,
    worker: &PendingOperation,
    slots: PairSlots,
    touched: bool,
) -> anyhow::Result<()> {
    let failure = if touched {
        crate::vm::CopyFailure::unknown(FailureReason::CloneFailed)
    } else {
        crate::vm::CopyFailure::untouched(FailureReason::CloneFailed)
    };
    controller.finish_static(worker, slots, Err(failure))?;
    Ok(())
}

impl StaticMachine<'_> {
    fn recorded_file(
        &self,
        workspace: &ProductionWorkspace,
        ledger: &LedgerCheckpoints<'_>,
        slot: ArtifactSlot,
        bytes: &[u8],
        deadline: Instant,
    ) -> Result<PathPin, NativeRefusal> {
        self.check(deadline)?;
        let mut writer = workspace.create_slot(slot, ledger)?;
        for chunk in bytes.chunks(256 << 10) {
            launch::check_preparation(deadline, &|| {
                self.cancel.is_cancelled() || self.dossier.watch.require_current().is_err()
            })?;
            writer.write_all(chunk)?;
        }
        writer.sync()?;
        let pin = writer.into_pin(slot.role != "seed")?;
        if !sink::held_private(&pin.leaf, self.m4.owner()) {
            return Err(PATH);
        }
        Ok(pin)
    }

    async fn blank(
        &self,
        workspace: &ProductionWorkspace,
        ledger: &LedgerCheckpoints<'_>,
        slot: ArtifactSlot,
        deadline: Instant,
    ) -> Result<PathPin, NativeRefusal> {
        use ArtifactRole::*;
        let helper = launch::stock_asset(&self.dossier.stock, "qemu-img.exe", 32 << 20, deadline)?;
        let target = workspace.create_slot(slot, ledger)?.into_pin(true)?;
        let image = launch::artifact(QemuImg, &helper)?;
        let disk = launch::artifact(Staging, &target)?;
        let plan =
            crate::vm::qemu_args::qualifier_create(&image, &disk, None, DISK_BYTES, DISK_LIMIT)
                .map_err(|_| PATH)?;
        self.reserve_writer(workspace)?;
        let (mut core, io) = self.launch(
            workspace,
            plan,
            vec![duplicate(&target)?, helper],
            None,
            deadline,
        )?;
        let (output, (released, _)) = core
            .helper_held(
                io,
                self.cancel,
                Deadline::from_std(deadline).min(Deadline::now() + Duration::from_secs(30)),
                || self.check(deadline),
                || true,
            )
            .await;
        output?;
        if released != StopDisposition::CleanRelease {
            return Err(JOB);
        }
        Self::verify_disk(&target)?;
        Ok(target)
    }

    async fn prelaunch(
        &self,
        core: &mut CheckedLaunchCore<'_>,
        io: BootIo,
        serial: &SerialName,
        origin: Option<&OriginPortLease>,
        release_guards: Vec<File>,
        deadline: Instant,
    ) -> Result<(PipeQmp, NamedPipeClient), NativeRefusal> {
        let pid = core.child.verify_live().map_err(|_| JOB)?;
        let handoff = origin
            .map(|port| port.take_for_handoff().ok_or(ENDPOINT))
            .transpose()?;
        let (read, write) = io;
        let cutoff = Deadline::from_std(deadline).min(Deadline::now() + Duration::from_secs(60));
        core.check_running(|| self.check(deadline), false)?;
        core.child.resume_checked_with_handoff(
            self.cancel,
            Some(cutoff),
            &mut || self.check(deadline),
            || drop((handoff, release_guards)),
        )?;
        let token = &self.m4.held.token;
        let (mut qmp, pipe) = monitor_running(
            async {
                let pipe = connect_serial(
                    serial,
                    &core.child,
                    token.account.0,
                    &token.entries(),
                    self.cancel,
                    cutoff,
                )
                .await?;
                let mut qmp = Qmp::handshake(read, write, self.cancel)
                    .await
                    .map_err(|_| QMP)?;
                qmp.verify_whpx(self.cancel)
                    .await
                    .map_err(|_| NativeRefusal::WhpxUnverified)?;
                Ok((qmp, pipe))
            },
            || core.check_running(|| self.check(deadline), false),
            self.cancel,
            cutoff,
            |_| QMP,
        )
        .await?;
        monitor_running(
            async {
                core.dispatch_cont(
                    &mut qmp,
                    self.cancel,
                    &mut || {
                        if let Some(port) = origin {
                            witness::listener(&core.child, port.port())?.check(port.port(), pid)?;
                        }
                        self.check(deadline)
                    },
                    Some(cutoff),
                )
                .await
                .map_err(|reason| match reason {
                    InstallerFailureReason::Native(reason) => reason,
                    _ => QMP,
                })
            },
            || core.check_running(|| self.check(deadline), false),
            self.cancel,
            cutoff,
            |_| QMP,
        )
        .await?;
        Ok((qmp, pipe))
    }

    async fn install(
        &self,
        recipe: StaticRecipe,
        workspace: &ProductionWorkspace,
        disk: &PathPin,
        vars: &PathPin,
        seed: &PathPin,
        deadline: Instant,
    ) -> Result<(), NativeRefusal> {
        use ArtifactRole::*;
        let assets = &self.dossier.recipe;
        let loader = self.loader(false, deadline)?;
        let [image, data, firmware] = Self::loader_artifacts(&loader)?;
        let iso = launch::artifact(UbuntuInstallIso, &assets.installer)?;
        let cidata = launch::artifact(UbuntuCidata, seed)?;
        let kernel = launch::artifact(UbuntuKernel, &assets.kernel)?;
        let initrd = launch::artifact(UbuntuInitrd, &assets.initrd)?;
        let compose = recipe == StaticRecipe::Compose;
        let (disk_role, vars_role) = if compose {
            (StaticDisk, StaticVars)
        } else {
            (TestInstallDisk, Vars)
        };
        let serial = Self::serial()?;
        let boot = BootRecipe {
            image: &image,
            data: &data,
            firmware: &firmware,
            vars: &launch::artifact(vars_role, vars)?,
            disk: &launch::artifact(disk_role, disk)?,
            disk_header: &launch::header(disk)?,
            backing: None,
            backing_header: None,
            serial: &serial,
            boot: BootPurpose::UbuntuInstall {
                iso: &iso,
                cidata: &cidata,
                kernel: &kernel,
                initrd: &initrd,
            },
            ram_mib: 4096,
            vcpus: 2,
        };
        let mut pins = loader.into_iter().map(|(_, pin)| pin).collect::<Vec<_>>();
        for pin in [
            disk,
            vars,
            seed,
            &assets.installer,
            &assets.kernel,
            &assets.initrd,
        ] {
            pins.push(duplicate(pin)?);
        }
        let medium = if compose {
            pins.push(duplicate(&assets.medium)?);
            Some(assets.dependency_artifact()?)
        } else {
            None
        };
        let plan = match &medium {
            Some(medium) => boot.static_compose_installer(medium),
            None => boot.plan(),
        }
        .map_err(|_| PATH)?;
        let stage = InstallerBudget::start(InstallerRecipe::Ubuntu)
            .stage(
                InstallerPhase::UbuntuInstall,
                deadline.saturating_duration_since(Instant::now()),
            )
            .ok_or(NativeRefusal::InstallerDeadlineExpired)?;
        self.reserve_writer(workspace)?;
        let (mut core, io) = self.launch(workspace, plan, pins, Some(stage), deadline)?;
        let result = core
            .boot_installer(stage, io, self.cancel, &mut || self.check(deadline), || {
                true
            })
            .await;
        match result {
            Ok(BootOutcome::InstallerReleased(..)) => Ok(()),
            _ => {
                let _ = core.stop(None, &Shutdown::default(), || true).await;
                Err(NativeRefusal::InstallerPhaseMismatch)
            }
        }
    }

    fn serial() -> Result<SerialName, NativeRefusal> {
        SerialName::generate().map_err(|_| PATH)
    }

    fn loader_artifacts(
        loader: &[(ArtifactRole, PathPin); 3],
    ) -> Result<[Artifact; 3], NativeRefusal> {
        let [image, data, code] = loader
            .each_ref()
            .map(|(role, pin)| launch::artifact(*role, pin));
        Ok([image?, data?, code?])
    }

    async fn detached(
        &self,
        recipe: StaticRecipe,
        workspace: &ProductionWorkspace,
        pins: &[PathPin; 2],
        deadline: Instant,
    ) -> Result<(), NativeRefusal> {
        use ArtifactRole::*;
        let loader = self.loader(false, deadline)?;
        let [image, data, firmware] = Self::loader_artifacts(&loader)?;
        let compose = recipe == StaticRecipe::Compose;
        let (disk_role, vars_role, purpose) = if compose {
            (StaticDisk, StaticVars, BootPurpose::DetachedStaticCompose)
        } else {
            (GuestDisk, Vars, BootPurpose::Installed)
        };
        let serial = Self::serial()?;
        let boot = BootRecipe {
            image: &image,
            data: &data,
            firmware: &firmware,
            vars: &launch::artifact(vars_role, &pins[1])?,
            disk: &launch::artifact(disk_role, &pins[0])?,
            disk_header: &launch::header(&pins[0])?,
            backing: None,
            backing_header: None,
            serial: &serial,
            boot: purpose,
            ram_mib: 4096,
            vcpus: 2,
        };
        let plan = if compose {
            boot.docker_trial_boot(arx_guest::docker_trial::BootMode::Detached)
        } else {
            boot.plan()
        }
        .map_err(|_| PATH)?;
        let guards = pins
            .iter()
            .map(PathPin::reopen_without_writer)
            .collect::<io::Result<Vec<_>>>()
            .map_err(|_| PATH)?;
        let mut held = loader.into_iter().map(|(_, pin)| pin).collect::<Vec<_>>();
        for pin in pins {
            held.push(duplicate(pin)?);
        }
        self.reserve_writer(workspace)?;
        let (mut core, io) = self.launch(workspace, plan, held, None, deadline)?;
        let (mut qmp, pipe) = match self
            .prelaunch(&mut core, io, &serial, None, guards, deadline)
            .await
        {
            Ok(channels) => channels,
            Err(reason) => {
                let _ = core.stop(None, &Shutdown::default(), || true).await;
                return Err(reason);
            }
        };
        let health = monitor_running(
            async {
                if compose {
                    self.compose_health(&mut qmp, pipe).await
                } else {
                    let mut channel = GuestChannel::ready(pipe, &mut qmp, self.cancel)
                        .await
                        .map_err(|_| ENDPOINT)?;
                    for check in &self.dossier.recipe.web_health {
                        let result = channel
                            .run_script(self.cancel, check.clone(), 30)
                            .await
                            .map_err(|_| ENDPOINT)?;
                        if result != (ScriptStatus::Exited, Some(0)) {
                            return Err(ENDPOINT);
                        }
                    }
                    Ok(())
                }
            },
            || core.check_running(|| self.check(deadline), false),
            self.cancel,
            Deadline::from_std(deadline).min(Deadline::now() + Duration::from_secs(220)),
            |_| ENDPOINT,
        )
        .await;
        let released = core.stop(Some(qmp), &Shutdown::default(), || true).await;
        health?;
        if released != StopDisposition::CleanRelease {
            return Err(JOB);
        }
        Ok(())
    }

    pub(crate) async fn build<C: Clock>(
        &self,
        controller: &VmController<C>,
        worker: &PendingOperation,
        slots: BuildSlots,
        request: BuildRequest<'_>,
        recipe: StaticRecipe,
        guards: &GenerationGuards,
    ) -> anyhow::Result<()> {
        let result = self
            .build_inner(controller, worker, slots, request, recipe, guards)
            .await;
        if let Err(reason) = result {
            controller.fail_build_with(worker, slots, false, Some(reason))?;
        }
        Ok(())
    }

    async fn build_inner<C: Clock>(
        &self,
        controller: &VmController<C>,
        worker: &PendingOperation,
        slots: BuildSlots,
        request: BuildRequest<'_>,
        recipe: StaticRecipe,
        guards: &GenerationGuards,
    ) -> Result<(), FailureReason> {
        let staging = |_| FailureReason::StagingFailed;
        let seal = |_| FailureReason::SealFailed;
        let deadline = worker.deadline();
        self.check(deadline).map_err(staging)?;
        let store = controller.native_store();
        let ledger = LedgerCheckpoints {
            store,
            origin: native::Origin::Build,
        };
        let workspace = self
            .m4
            .reserve_workspace(slots.pair.disk.generation, &ledger)
            .map_err(staging)?;
        let recipe_inputs = &self.dossier.recipe;
        let input_seed = match recipe {
            StaticRecipe::Web => &recipe_inputs.web_seed,
            StaticRecipe::Compose => &recipe_inputs.seed,
        };
        let seed_bytes =
            crate::vm::template::ubuntu_web::rebind_seed(input_seed, slots.pair.disk.generation)
                .map_err(|_| FailureReason::StagingFailed)?;
        if seed_bytes.len() as u64 > request.transient.staging {
            return Err(FailureReason::StagingFailed);
        }
        let mut seed = self
            .recorded_file(&workspace, &ledger, slots.seed, &seed_bytes, deadline)
            .map_err(staging)?;
        let seed_pin = ArtifactPin {
            size: seed_bytes.len() as u64,
            sha256: crate::vm::qualification_record::digest(&seed_bytes),
        };
        seed.verify_content(seed_pin, request.transient.staging)
            .map_err(staging)?;
        controller
            .build_evidence(
                worker,
                slots.id,
                &[(
                    slots.seed,
                    Some(ArtifactEvidence {
                        identity: seed.identity,
                        directory: workspace.binding().workspace.file,
                        pin: seed_pin,
                    }),
                )],
            )
            .map_err(|_| FailureReason::StagingFailed)?;
        let vars_bytes = launch::held_bytes(&recipe_inputs.firmware[1], MAX_SETUP_VARS_BYTES)
            .map_err(staging)?;
        let mut vars = self
            .recorded_file(&workspace, &ledger, slots.pair.vars, &vars_bytes, deadline)
            .map_err(staging)?;
        let vars_pin = recipe_inputs.firmware[1]
            .verified_content
            .ok_or(FailureReason::StagingFailed)?;
        vars.verify_content(vars_pin, MAX_SETUP_VARS_BYTES)
            .map_err(staging)?;
        let disk = self
            .blank(&workspace, &ledger, slots.pair.disk, deadline)
            .await
            .map_err(staging)?;
        self.install(recipe, &workspace, &disk, &vars, &seed, deadline)
            .await
            .map_err(|reason| match reason {
                NativeRefusal::InstallerDeadlineExpired => FailureReason::InstallTimedOut,
                _ => FailureReason::InstallFailed,
            })?;
        drop(seed);
        let primary = [disk, vars];
        let read_guards = primary
            .iter()
            .map(PathPin::reopen_without_writer)
            .collect::<io::Result<Vec<_>>>()
            .map_err(|_| FailureReason::SealFailed)?;
        let cancelled =
            || self.cancel.is_cancelled() || self.dossier.watch.require_current().is_err();
        let observe = |n| {
            Self::evidence(
                &workspace,
                &primary[n],
                &read_guards[n],
                [DISK_LIMIT, MAX_SETUP_VARS_BYTES][n],
                deadline,
                &cancelled,
            )
        };
        Self::verify_disk(&primary[0]).map_err(seal)?;
        let proof = [observe(0).map_err(seal)?, observe(1).map_err(seal)?];
        let scope = custody::scope(workspace.binding());
        store
            .transact(|db| {
                for (slot, evidence) in [slots.pair.disk, slots.pair.vars].into_iter().zip(proof) {
                    native::verify_build_slot(db, scope, slot, evidence)?;
                }
                Ok(())
            })
            .map_err(|_| FailureReason::SealFailed)?;

        let verification = self
            .m4
            .reserve_workspace(slots.verification.disk.generation, &ledger)
            .map_err(staging)?;
        let copy = |n: usize, slot| -> Result<PathPin, NativeRefusal> {
            self.check(deadline)?;
            let mut sink = verification.create_slot(slot, &ledger)?;
            self.copy_to(&read_guards[n], proof[n].pin.size, deadline, |bytes| {
                sink.write_all(bytes)
            })?;
            sink.sync()?;
            let pin = sink.into_pin(true)?;
            let guard = pin.reopen_without_writer().map_err(|_| PATH)?;
            if Self::evidence(
                &verification,
                &pin,
                &guard,
                proof[n].pin.size,
                deadline,
                &cancelled,
            )?
            .pin != proof[n].pin
            {
                return Err(PATH);
            }
            Ok(pin)
        };
        let verified = [
            copy(0, slots.verification.disk).map_err(staging)?,
            copy(1, slots.verification.vars).map_err(staging)?,
        ];
        self.detached(recipe, &verification, &verified, deadline)
            .await
            .map_err(|_| FailureReason::HealthFailed)?;
        let verification_scope = custody::scope(verification.binding());
        for ((n, pin), slot) in verified
            .iter()
            .enumerate()
            .zip([slots.verification.disk, slots.verification.vars])
        {
            let guard = pin
                .reopen_without_writer()
                .map_err(|_| FailureReason::SealFailed)?;
            let evidence = Self::evidence(
                &verification,
                pin,
                &guard,
                [DISK_LIMIT, MAX_SETUP_VARS_BYTES][n],
                deadline,
                &cancelled,
            )
            .map_err(seal)?;
            store
                .transact(|db| native::verify_build_slot(db, verification_scope, slot, evidence))
                .map_err(|_| FailureReason::SealFailed)?;
        }
        drop(verified);
        drop(verification);
        self.check(deadline).map_err(seal)?;
        let disposal = disposal::LedgerDisposal { store };
        self.m4
            .disposal(&disposal, guards)
            .dispose_build_transient_held(worker.activity_id(), slots, deadline, &workspace)
            .map_err(seal)?;
        controller
            .commit_static_build(worker, slots, request, scope, proof, || {
                !cancelled() && self.dossier.require_current().is_ok()
            })
            .map_err(|_| FailureReason::SealFailed)
    }
}

pub(crate) struct StartedStatic<'a> {
    pub boot: StaticBoot<'a>,
    pub pipe: NamedPipeClient,
    pub origin: crate::vm::runtime::publication::Origin,
    pub wire_boot: u64,
}

pub(crate) struct StaticBoot<'a> {
    core: CheckedLaunchCore<'a>,
    qmp: PipeQmp,
    port: u16,
    pid: u32,
}

impl StaticBoot<'_> {
    pub(crate) fn current(&self) -> bool {
        witness::listener(&self.core.child, self.port)
            .and_then(|proof| proof.check(self.port, self.pid))
            .is_ok()
    }

    pub(crate) async fn stop(
        mut self,
        settle: impl AsyncFnOnce(StopDisposition) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        let (disposition, _held) = self
            .core
            .stop_held(Some(self.qmp), &Shutdown::default(), || true)
            .await;
        settle(disposition).await
    }
}

impl StaticMachine<'_> {
    async fn compose_health(
        &self,
        qmp: &mut PipeQmp,
        mut pipe: NamedPipeClient,
    ) -> Result<(), NativeRefusal> {
        use arx_guest::docker_trial::{Challenge, TrialRequest, TrialResponse};
        self.wait_port(qmp).await?;
        let challenge = Challenge {
            generation: 1,
            nonce: store::new_id().map_err(|_| PATH)?,
        };
        let reply: TrialResponse =
            exchange(&mut pipe, &TrialRequest::DockerTrialHealth { challenge }).await?;
        if reply
            != (TrialResponse::DockerTrialHealthy {
                challenge,
                packages: self.dossier.recipe.bootstrap_packages,
            })
        {
            return Err(ENDPOINT);
        }
        Ok(())
    }

    async fn wait_port(&self, qmp: &mut PipeQmp) -> Result<(), NativeRefusal> {
        let mut watch = crate::vm::qmp::PortWatch::default();
        while !qmp
            .port_change(crate::vm::qemu_args::GUEST_PORT_ID, &mut watch)
            .await
            .map_err(|_| QMP)?
        {}
        Ok(())
    }

    pub(crate) fn hold_static(
        &self,
        principal: &str,
        vm: Id,
        recipe: StaticRecipe,
    ) -> Result<(ProductionWorkspace, Pair, Id), NativeRefusal> {
        use crate::store::vm::static_vm;
        use crate::vm::static_web::StaticState;
        let selected = recipe.identity(self.dossier);
        let (scope, slots, evidence, origin) = self
            .m4
            .store
            .read_only(|db| {
                let row = static_vm::rows(db, principal, Some(vm), None, 1)?
                    .pop()
                    .ok_or_else(|| anyhow::anyhow!("static VM unavailable"))?;
                anyhow::ensure!(
                    row.provenance.recipe_digest == selected.recipe_sha256,
                    "static recipe no longer selected"
                );
                let (scope, slots, evidence) =
                    static_vm::native::custody(db, principal, vm, StaticState::Stopped)?
                        .ok_or_else(|| anyhow::anyhow!("static custody unavailable"))?;
                Ok((
                    scope,
                    slots,
                    evidence,
                    static_vm::origin_generation(db, vm)?,
                ))
            })
            .map_err(|_| PATH)?;
        let workspace = self
            .m4
            .hold_start_workspace(disposal::binding_from_scope(scope))?;
        Ok((workspace, (slots, evidence), origin))
    }

    pub(crate) async fn start<'a>(
        &'a self,
        workspace: &'a ProductionWorkspace,
        pair: Pair,
        origin_generation: Id,
        recipe: StaticRecipe,
        deadline: Instant,
    ) -> Result<StartedStatic<'a>, NativeRefusal> {
        use crate::vm::runtime::publication::Origin;
        use ArtifactRole::*;
        use arx_guest::static_origin::{self, Request, Response};
        let (slots, [disk, vars]) = pair;
        let disk_pin = self.private_pin(workspace, slots.disk, disk.identity, true)?;
        let vars_pin = self.private_pin(workspace, slots.vars, vars.identity, true)?;
        let guards = [&disk_pin, &vars_pin]
            .into_iter()
            .map(PathPin::reopen_without_writer)
            .collect::<io::Result<Vec<_>>>()
            .map_err(|_| PATH)?;
        Self::verify_disk(&disk_pin)?;
        if !vars_pin
            .leaf
            .metadata()
            .is_ok_and(|file| file.len() == vars.pin.size && file.len() <= MAX_SETUP_VARS_BYTES)
        {
            return Err(PATH);
        }
        let lease = OriginPortLease::acquire().map_err(|_| ENDPOINT)?;
        let port = lease.port();
        let compose = recipe == StaticRecipe::Compose;
        let loader = self.loader(compose, deadline)?;
        let [image, data, firmware] = Self::loader_artifacts(&loader)?;
        let policy = launch::artifact(PublicEgressPolicy, &self.dossier.policy_file)?;
        let network = if compose {
            StaticNetwork::PublicEgress(&policy)
        } else {
            StaticNetwork::RestrictedOrigin
        };
        let serial = Self::serial()?;
        let boot = BootRecipe {
            image: &image,
            data: &data,
            firmware: &firmware,
            vars: &launch::artifact(StaticVars, &vars_pin)?,
            disk: &launch::artifact(StaticDisk, &disk_pin)?,
            disk_header: &launch::header(&disk_pin)?,
            backing: None,
            backing_header: None,
            serial: &serial,
            boot: BootPurpose::InstalledStaticWeb(&lease, network),
            ram_mib: 4096,
            vcpus: 2,
        };
        let plan = boot
            .provision_static(
                origin_generation,
                compose.then(|| self.dossier.public_dns()),
            )
            .map_err(|_| PATH)?;
        let mut pins = loader.into_iter().map(|(_, pin)| pin).collect::<Vec<_>>();
        pins.extend([disk_pin, vars_pin]);
        if compose {
            pins.push(duplicate(&self.dossier.policy_file)?);
        }
        let (mut core, io) = self.launch(workspace, plan, pins, None, deadline)?;
        let (mut qmp, mut pipe) = match self
            .prelaunch(&mut core, io, &serial, Some(&lease), guards, deadline)
            .await
        {
            Ok(channels) => channels,
            Err(reason) => {
                let _ = core.stop(None, &Shutdown::default(), || true).await;
                return Err(reason);
            }
        };
        let provisioned = monitor_running(
            async {
                let pid = core.child.verify_live().map_err(|_| JOB)?;
                let boot = store::new_id().map_err(|_| PATH)?;
                let request = Request::StaticOrigin {
                    generation: origin_generation,
                    nonce: boot,
                };
                self.wait_port(&mut qmp).await?;
                witness::listener(&core.child, port)?.check(port, pid)?;
                let Response::StaticOriginReady {
                    request: echo,
                    ca,
                    leaf,
                } = exchange(&mut pipe, &request).await?;
                if echo != request {
                    return Err(ENDPOINT);
                }
                super::super::docker_trial::origin::probe_exact(
                    port,
                    &static_origin::server_name(origin_generation),
                    &static_origin::health_body(boot),
                    &ca,
                    &leaf,
                    deadline.saturating_duration_since(Instant::now()),
                    self.cancel,
                )
                .await?;
                witness::listener(&core.child, port)?.check(port, pid)?;
                let (ca_path, ca_receipt) =
                    pin_generation_ca(&workspace.root, self.m4.owner(), &ca, || {
                        self.check(deadline).map(|_| ())
                    })?;
                let wire_boot = u64::from_be_bytes(boot[..8].try_into().expect("eight bytes"))
                    & (i64::MAX as u64);
                if wire_boot == 0 {
                    return Err(ENDPOINT);
                }
                let origin = Origin {
                    generation: slots.disk.generation,
                    origin_generation,
                    boot,
                    port: port.try_into().map_err(|_| ENDPOINT)?,
                    ca_path,
                    ca_receipt,
                    retired: crate::vm::runtime::publication::Retirement::on_route(
                        self.dossier.retirement(),
                    ),
                };
                Ok((pid, origin, wire_boot))
            },
            || core.check_running(|| self.check(deadline), false),
            self.cancel,
            Deadline::from_std(deadline),
            |_| ENDPOINT,
        )
        .await;
        let (pid, origin, wire_boot) = match provisioned {
            Ok(provisioned) => provisioned,
            Err(reason) => {
                let _ = core.stop(Some(qmp), &Shutdown::default(), || true).await;
                return Err(reason);
            }
        };
        Ok(StartedStatic {
            boot: StaticBoot {
                core,
                qmp,
                pid,
                port,
            },
            pipe,
            origin,
            wire_boot,
        })
    }
}

fn pin_generation_ca(
    root: &PathPin,
    owner: PSID,
    ca: &[u8],
    current: impl FnOnce() -> Result<(), NativeRefusal>,
) -> Result<(std::path::PathBuf, ArtifactPin), NativeRefusal> {
    const DIR: &str = "origin-ca";
    const CA: &str = "origin-ca.der";
    if !(1..=arx_guest::static_origin::MAX_CERT).contains(&ca.len()) {
        return Err(PATH);
    }
    let pin = ArtifactPin {
        size: ca.len() as u64,
        sha256: crate::vm::qualification_record::digest(ca),
    };
    let path = root
        .path
        .join(DIR)
        .and_then(|dir| dir.join(CA))
        .map_err(|_| PATH)?;
    {
        // A rename opens its target directory for adding a file with shared
        // write, which the workspace root's read-only share refuses. Publish
        // in a dedicated subdirectory whose only held handle shares write.
        let dir = match open_relative(&root.leaf, DIR, Node::Directory, true, false) {
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                drop(
                    open_relative(&root.leaf, DIR, Node::Directory, false, true)
                        .map_err(|_| PATH)?,
                );
                open_relative(&root.leaf, DIR, Node::Directory, true, false)
            }
            opened => opened,
        }
        .map_err(|_| PATH)?;
        if file_identity(&dir, true).map_err(|_| PATH)?.0.volume != root.identity.volume
            || !has_exact_leaf(&dir, DIR)
            || !sink::held_private(&dir, owner)
        {
            return Err(PATH);
        }
        match open_relative(&dir, CA, Node::File, false, false) {
            Ok(_) => {}
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                let temp = format!(
                    ".origin-ca-{}",
                    hex::encode(store::new_id().map_err(|_| PATH)?)
                );
                let guard = sink::create_hidden(&dir, &temp).map_err(|_| PATH)?;
                let result = (|| {
                    if !sink::held_private(&guard, owner) {
                        return Err(PATH);
                    }
                    (&guard).write_all(ca).map_err(|_| PATH)?;
                    guard.sync_all().map_err(|_| PATH)?;
                    current()?;
                    sink::rename_within(&guard, &dir, CA).map_err(|_| PATH)
                })();
                if result.is_err() {
                    let _ = sink::discard(&guard);
                }
                result?;
            }
            Err(_) => return Err(PATH),
        }
        // The temporary entry and the shared-write directory handle close
        // here, before the final file is pinned read-only.
    }
    let mut held = PathPin::open(&path, false, false)?;
    if !has_exact_leaf(&held.leaf, CA) || !sink::held_private(&held.leaf, owner) {
        return Err(PATH);
    }
    held.verify_content(pin, arx_guest::static_origin::MAX_CERT as u64)?;
    Ok((std::path::PathBuf::from(path.as_str()), pin))
}

async fn exchange<T: arx_guest::protocol::Message, R: arx_guest::protocol::Message>(
    pipe: &mut NamedPipeClient,
    request: &T,
) -> Result<R, NativeRefusal> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut encoded = Vec::new();
    arx_guest::write_frame(&mut encoded, request).map_err(|_| ENDPOINT)?;
    pipe.write_all(&encoded).await.map_err(|_| ENDPOINT)?;
    encoded.resize(4, 0);
    pipe.read_exact(&mut encoded).await.map_err(|_| ENDPOINT)?;
    let length = u32::from_be_bytes(encoded[..4].try_into().expect("four bytes")) as usize;
    if !(1..=arx_guest::MAX_FRAME).contains(&length) {
        return Err(ENDPOINT);
    }
    encoded.resize(4 + length, 0);
    pipe.read_exact(&mut encoded[4..])
        .await
        .map_err(|_| ENDPOINT)?;
    arx_guest::read_frame(&mut encoded.as_slice())
        .map_err(|_| ENDPOINT)?
        .ok_or(ENDPOINT)
}

#[cfg(test)]
mod tests;
