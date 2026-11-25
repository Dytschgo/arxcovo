//! Private construction boundary for the deferred TEST recipe collectors.
#![allow(dead_code, reason = "recipe collectors wire this entry in PR 2")]

use super::*;
use std::{cell::Cell, future::Future};

pub(super) struct HeldAsset {
    pin: PathPin,
    expected: ArtifactPin,
}

impl HeldAsset {
    pub(super) fn open(
        enrollment: Option<&ReleaseImage>,
        evidence: &LocalPath,
        limit: u64,
    ) -> Result<Self, NativeRefusal> {
        let missing = NativeRefusal::InstallerPhaseUnavailable;
        let input = enrollment.ok_or(missing)?;
        let path = local(&input.path)?;
        if path.overlaps(evidence) {
            return Err(NativeRefusal::PathCustodyUnverified);
        }
        let mut digest = [0; 32];
        hex::decode_to_slice(&input.sha256, &mut digest).map_err(|_| missing)?;
        let expected = ArtifactPin {
            size: input.size,
            sha256: digest,
        };
        let mut pin = PathPin::open(&path, false, false)?;
        pin.verify_content(expected, limit).map_err(|_| missing)?;
        Ok(Self { pin, expected })
    }

    pub(super) fn overlaps(&self, path: &LocalPath) -> bool {
        self.pin.path.overlaps(path)
    }

    fn read(&self) -> Result<Vec<u8>, NativeRefusal> {
        use std::io::{Seek, SeekFrom};
        let reason = NativeRefusal::InstallerPhaseUnavailable;
        inventory::binding(&self.pin)?;
        let mut file = &self.pin.leaf;
        file.seek(SeekFrom::Start(0)).map_err(|_| reason)?;
        let mut bytes = Vec::new();
        file.take(self.expected.size + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| reason)?;
        if !matches!(
            self.expected.verify(bytes.as_slice(), self.expected.size),
            Ok(true)
        ) {
            return Err(reason);
        }
        Ok(bytes)
    }

    fn stage_setup_vars(
        &self,
        workspace: &QualifiedWorkspace,
    ) -> Result<(crate::vm::paths::Artifact, PathPin), NativeRefusal> {
        let failure = NativeRefusal::InstallerPhaseUnavailable;
        let bytes = self.read()?;
        let mut copy = workspace.stage_bytes("setup-vars.fd", &bytes)?;
        copy.verify_content(self.expected, super::super::MAX_SETUP_VARS_BYTES)?;
        let artifact =
            crate::vm::paths::Artifact::new(ArtifactRole::WindowsSetupVars, copy.path.clone())
                .map_err(|_| failure)?
                .with_identity(copy.identity)
                .with_pin(self.expected);
        Ok((artifact, copy))
    }
}

/// Run- and generation-bound ownership of a disposable workspace and its
/// exclusive slot file. Replaces bare mutable paths on the native launch edge.
pub(in crate::vm::windows) struct WorkspaceCustody<'a> {
    run: [u8; 16],
    generation: u64,
    root: &'a PathPin,
    slot: &'a File,
}

impl<'a> WorkspaceCustody<'a> {
    fn new(
        run: [u8; 16],
        generation: u64,
        root: &'a PathPin,
        slot: &'a File,
    ) -> Result<Self, NativeRefusal> {
        let custody = NativeRefusal::PathCustodyUnverified;
        if run == [0; 16] || generation == 0 {
            return Err(custody);
        }
        qualification::inventory::binding(root)?;
        let (slot_id, _) = file_identity(slot, false).map_err(|_| custody)?;
        if slot_id.volume != root.identity.volume {
            return Err(custody);
        }
        Ok(Self {
            run,
            generation,
            root,
            slot,
        })
    }

    pub(in crate::vm::windows) fn verify(&self) -> Result<(), NativeRefusal> {
        let custody = NativeRefusal::PathCustodyUnverified;
        qualification::inventory::binding(self.root)?;
        let (root_id, _) = file_identity(&self.root.leaf, true).map_err(|_| custody)?;
        let (slot_id, _) = file_identity(self.slot, false).map_err(|_| custody)?;
        if root_id != self.root.identity || slot_id.volume != self.root.identity.volume {
            return Err(custody);
        }
        Ok(())
    }

    pub(in crate::vm::windows) fn volume(&self) -> u64 {
        self.root.identity.volume
    }

    pub(in crate::vm::windows) fn path(&self) -> &LocalPath {
        &self.root.path
    }
}

struct QualifierPlanPermit {
    run: [u8; 16],
    generation: u64,
}

impl QualifierPlanPermit {
    fn check(
        &self,
        run: [u8; 16],
        generation: u64,
        kind: PlanKind,
        has_offline_lease: bool,
    ) -> Result<(), NativeRefusal> {
        if run != self.run
            || generation != self.generation
            || has_offline_lease
            || !matches!(
                kind,
                PlanKind::Boot
                    | PlanKind::InstallerBoot(_)
                    | PlanKind::Inspect
                    | PlanKind::QualifierCreate
            )
        {
            return Err(NativeRefusal::QualificationMissing);
        }
        Ok(())
    }
}

struct QualifierLaunch<'a> {
    core: CheckedLaunchCore<'a>,
}

struct QualifiedWorkspace {
    root: PathPin,
    charge: File,
    run: [u8; 16],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AssetDigest {
    size: u64,
    sha256: String,
}

impl QualifiedWorkspace {
    fn hold<'a>(
        &'a self,
        generation: u64,
    ) -> Result<(WorkspaceCustody<'a>, QualifierPlanPermit), NativeRefusal> {
        let custody = WorkspaceCustody::new(self.run, generation, &self.root, &self.charge)?;
        Ok((
            custody,
            QualifierPlanPermit {
                run: self.run,
                generation,
            },
        ))
    }
}

/// Only the held full-TEST baseline supplies this projection in production.
/// The recipe area is deliberately distinct from the sacrificial trial area.
struct HeldRecipeScope<'a> {
    full_test: bool,
    run: [u8; 16],
    bundle: Option<&'a inventory::Bundle>,
    recipe_root: Option<&'a PathPin>,
}

impl<'a> HeldRecipeScope<'a> {
    fn from_baseline(baseline: &'a Baseline) -> Self {
        let held = &baseline._held;
        Self {
            full_test: held.full_test,
            run: baseline.run(),
            bundle: held.bundle.as_ref(),
            recipe_root: held.scope.as_ref().map(|scope| &scope.recipe.root),
        }
    }
}

impl<'a> QualifierLaunch<'a> {
    fn hold<T>(
        permit: QualifierPlanPermit,
        baseline: HeldRecipeScope<'a>,
        inputs: LaunchInputs<'a>,
        at_create: impl FnOnce(LaunchInputs<'a>) -> Result<T, NativeRefusal>,
    ) -> Result<T, NativeRefusal> {
        permit.check(
            inputs.workspace.run,
            inputs.workspace.generation,
            inputs.plan.kind(),
            inputs.plan.offline_lease().is_some(),
        )?;
        if !baseline.full_test
            || baseline.run != inputs.workspace.run
            || !baseline
                .bundle
                .is_some_and(|b| std::ptr::eq(b, inputs.bundle))
            || !baseline.recipe_root.is_some_and(|root| {
                root.path != *inputs.workspace.path()
                    && root.path.contains(inputs.workspace.path())
                    && root.identity.volume == inputs.workspace.volume()
            })
        {
            return Err(NativeRefusal::QualificationMissing);
        }
        at_create(inputs)
    }

    fn prepare(
        permit: QualifierPlanPermit,
        baseline: &'a Baseline,
        inputs: LaunchInputs<'a>,
        refresh: impl FnOnce() -> Result<std::time::Instant, NativeRefusal>,
    ) -> Result<(Self, tokio::io::DuplexStream, tokio::io::DuplexStream), NativeRefusal> {
        Self::hold(
            permit,
            HeldRecipeScope::from_baseline(baseline),
            inputs,
            |inputs| {
                let (core, read, write) =
                    CheckedLaunchCore::hold_checked(&baseline._held.token, inputs, refresh)?;
                Ok((Self { core }, read, write))
            },
        )
    }

    async fn helper(
        &mut self,
        (mut read, write): BootIo,
        cancel: &Shutdown,
        deadline: tokio::time::Instant,
        mut refresh: impl FnMut() -> Result<Instant, NativeRefusal>,
    ) -> (Result<Vec<u8>, NativeRefusal>, StopDisposition) {
        use tokio::io::AsyncReadExt;
        let refused = NativeRefusal::JobUnverified;
        let work = async {
            if !matches!(
                self.core.kind,
                PlanKind::Inspect | PlanKind::QualifierCreate
            ) {
                return Err(NativeRefusal::QualificationMissing);
            }
            self.core
                .stock
                .recheck_all(&self.core.pins, Some(&self.core.workspace))?;
            self.core.child.resume_checked(cancel, &mut refresh)?;
            let mut output = Vec::new();
            (&mut read)
                .take(16 * 1024 + 1)
                .read_to_end(&mut output)
                .await
                .map_err(|_| refused)?;
            self.core.child.reap(deadline).await.map_err(|_| refused)?;
            if output.len() > 16 * 1024 || self.core.child.exit_code()? != 0 {
                return Err(refused);
            }
            Ok(output)
        };
        let result = tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(refused),
            _ = tokio::time::sleep_until(deadline) => Err(refused),
            result = work => result,
        };
        drop(write);
        if result.is_err() && self.core.child.job.terminate().is_err() {
            return (result, StopDisposition::ChargedQuarantine);
        }
        let successful = result.is_ok();
        let release = self
            .core
            .release_evidence(
                successful,
                successful,
                tokio::time::Instant::now() + Duration::from_secs(5),
                || true,
            )
            .await;
        (result, release)
    }
}

impl QualifiedWorkspace {
    fn claim(baseline: &Baseline, label: &str, peak: u64) -> Result<Self, NativeRefusal> {
        use std::io::Write;
        let reason = NativeRefusal::DiskCeilingUnverified;
        let root = &baseline._held.scope.as_ref().ok_or(reason)?.recipe.root;
        inventory::binding(root)?;
        recipe_live(baseline, peak, 0)?;
        let mut charge =
            sink::create_hidden(&root.leaf, ".arx-recipes-charge").map_err(|_| reason)?;
        charge
            .write_all(&baseline.run())
            .and_then(|()| charge.sync_all())
            .map_err(|_| reason)?;
        let path = root
            .path
            .join(&format!("recipe-{label}-{}", hex::encode(baseline.run())))
            .map_err(|_| reason)?;
        let workspace = PathPin::pin(&path, true, true, true).map_err(|_| reason)?;
        if workspace.identity.volume != root.identity.volume
            || !sink::held_private(&workspace.leaf, baseline._held.token.account.0)
        {
            return Err(NativeRefusal::PathCustodyUnverified);
        }
        Ok(Self {
            root: workspace,
            charge,
            run: baseline.run(),
        })
    }

    fn stage_bytes(&self, name: &str, bytes: &[u8]) -> Result<PathPin, NativeRefusal> {
        use std::io::Write;
        let path = self
            .root
            .path
            .join(name)
            .map_err(|_| NativeRefusal::PathCustodyUnverified)?;
        let file = PathPin::create(&path)?;
        (&file.leaf)
            .write_all(bytes)
            .and_then(|()| file.leaf.sync_all())
            .map_err(|_| NativeRefusal::PathCustodyUnverified)?;
        let id = file.identity;
        drop(file);
        let reopened = PathPin::open(&path, false, true)?;
        (reopened.identity == id)
            .then_some(reopened)
            .ok_or(NativeRefusal::PathCustodyUnverified)
    }
}

fn recipe_live(baseline: &Baseline, peak: u64, allocated: u64) -> Result<Instant, NativeRefusal> {
    let reason = NativeRefusal::DiskCeilingUnverified;
    let captured = baseline.paused_live()?;
    let held = &baseline._held;
    let scope = held.scope.as_ref().ok_or(reason)?;
    use super::capacity::{MountManager, held_free, still_bound};
    let free = held_free(&scope.recipe.root.leaf)?;
    let outer = held_free(&held.volumes[1].root)?;
    let trial = match &baseline.observations().limits {
        Taken::Observed(observed) => &observed.result,
        _ => return Err(reason),
    };
    if free.total != scope.recipe.limits.inner_total_bytes
        || peak > scope.recipe.limits.disk_budget_bytes
        || allocated > peak
        || free.caller.min(free.total_free) < peak.saturating_sub(allocated)
        || outer.caller.min(outer.total_free) < OUTER_FLOOR
        || !still_bound(&MountManager, &scope.recipe.volume)
        || !still_bound(&MountManager, &held.volumes[1])
        || !scope.aggregate_reservation_within_cap(trial, 0)
    {
        return Err(reason);
    }
    let (cpus, ram) = host_resources()?;
    if cpus < 2 || ram < RAM_RESERVE + 7 * GIB || !hypervisor_present()? {
        return Err(NativeRefusal::ReservesUnverified);
    }
    Ok(captured)
}

fn artifact(
    role: ArtifactRole,
    pin: &PathPin,
) -> Result<crate::vm::paths::Artifact, NativeRefusal> {
    let artifact = crate::vm::paths::Artifact::new(role, pin.path.clone())
        .map_err(|_| NativeRefusal::PathCustodyUnverified)?
        .with_identity(pin.identity);
    Ok(match pin.verified_content {
        Some(expected) => artifact.with_pin(expected),
        None => artifact,
    })
}

fn bound_pin(path: &LocalPath, held: &PathPin, limit: u64) -> Result<PathPin, NativeRefusal> {
    let mut pin = PathPin::open(path, false, false)?;
    if pin.identity != held.identity {
        return Err(NativeRefusal::PathCustodyUnverified);
    }
    pin.verify_content(
        held.verified_content
            .ok_or(NativeRefusal::DistributionUnverified)?,
        limit,
    )?;
    Ok(pin)
}

fn hash_at_rest(
    pin: &mut PathPin,
    limit: u64,
    deadline: Instant,
) -> Result<ArtifactPin, NativeRefusal> {
    use sha2::{Digest, Sha256};
    use std::io::{Seek, SeekFrom};
    let reason = NativeRefusal::PathCustodyUnverified;
    inventory::binding(pin)?;
    let size = pin.leaf.metadata().map_err(|_| reason)?.len();
    if !(1..=limit).contains(&size) {
        return Err(reason);
    }
    let mut reader = &pin.leaf;
    reader.seek(SeekFrom::Start(0)).map_err(|_| reason)?;
    let mut digest = Sha256::new();
    let mut work = vec![0; 256 << 10];
    let mut remaining = size;
    while remaining != 0 {
        if Instant::now() >= deadline {
            return Err(reason);
        }
        let len = remaining.min(work.len() as u64) as usize;
        reader.read_exact(&mut work[..len]).map_err(|_| reason)?;
        digest.update(&work[..len]);
        remaining -= len as u64;
    }
    let mut overflow = [0];
    if reader.read(&mut overflow).map_err(|_| reason)? != 0 {
        return Err(reason);
    }
    inventory::binding(pin)?;
    let expected = ArtifactPin {
        size,
        sha256: digest.finalize().into(),
    };
    pin.verified_content = Some(expected);
    Ok(expected)
}

fn header(pin: &PathPin) -> Result<Vec<u8>, NativeRefusal> {
    use std::io::{Seek, SeekFrom};
    let reason = NativeRefusal::PathCustodyUnverified;
    let mut bytes = vec![0; crate::vm::qemu_img::CLUSTER];
    let mut file = &pin.leaf;
    file.seek(SeekFrom::Start(0))
        .and_then(|_| file.read_exact(&mut bytes))
        .map_err(|_| reason)?;
    Ok(bytes)
}

struct RecipeCommands<'a> {
    baseline: &'a Baseline,
    workspace: &'a QualifiedWorkspace,
    bundle: &'a inventory::Bundle,
    cwd: LocalPath,
    peak: u64,
    cancel: &'a Shutdown,
    deadline: tokio::time::Instant,
}

impl RecipeCommands<'_> {
    fn live(&self) -> Result<Instant, NativeRefusal> {
        if self.cancel.is_cancelled() || tokio::time::Instant::now() >= self.deadline {
            return Err(NativeRefusal::InstallerDeadlineExpired);
        }
        let allocated = allocated_under(
            &self.workspace.root.path,
            self.workspace.root.identity.volume,
            16,
        )?;
        recipe_live(self.baseline, self.peak, allocated)
    }

    fn command<'a>(
        &'a self,
        plan: CommandPlan<'a>,
        pins: Vec<PathPin>,
        generation: u64,
        installer: Option<InstallerStage>,
        finalization_slots: Option<crate::vm::PairSlots>,
    ) -> Result<(QualifierLaunch<'a>, BootIo), NativeRefusal> {
        let (workspace, permit) = self.workspace.hold(generation)?;
        let (launch, read, write) = QualifierLaunch::prepare(
            permit,
            self.baseline,
            LaunchInputs {
                plan,
                cwd: &self.cwd,
                workspace,
                bundle: self.bundle,
                memory: (6 * GIB) as usize,
                cpu_rate: JOB_CPU_RATE,
                installer,
                finalization_slots,
                pins,
            },
            || self.live(),
        )?;
        Ok((launch, (read, write)))
    }

    async fn make_disk(
        &self,
        name: &str,
        size: u64,
        backing: Option<PathPin>,
        generation: u64,
    ) -> Result<(PathPin, FileIdentity), NativeRefusal> {
        use crate::vm::qemu_args::qualifier_create;
        let reason = NativeRefusal::PathCustodyUnverified;
        self.live()?;
        let path = self.workspace.root.path.join(name).map_err(|_| reason)?;
        let image = self
            .bundle
            .root
            .path
            .join("qemu-img.exe")
            .map_err(|_| reason)?;
        let image = PathPin::open(&image, false, false)?;
        let staging = self.workspace.stage_bytes(name, &[])?;
        let identity = staging.identity;
        let image_arg = artifact(ArtifactRole::QemuImg, &image)?;
        let staging_arg = artifact(ArtifactRole::Staging, &staging)?;
        let golden = backing
            .as_ref()
            .map(|pin| artifact(ArtifactRole::Golden, pin))
            .transpose()?;
        let backing_header = backing.as_ref().map(header).transpose()?;
        let plan = qualifier_create(
            &image_arg,
            &staging_arg,
            golden.as_ref().zip(backing_header.as_deref()),
            size,
            self.peak,
        )
        .map_err(|_| reason)?;
        let pins = [image, staging].into_iter().chain(backing).collect();
        let (mut launch, io) = self.command(plan, pins, generation, None, None)?;
        let helper_deadline = self
            .deadline
            .min(tokio::time::Instant::now() + Duration::from_secs(30));
        let (result, release) = launch
            .helper(io, self.cancel, helper_deadline, || self.live())
            .await;
        drop(launch);
        if release != StopDisposition::CleanRelease {
            return Err(NativeRefusal::JobUnverified);
        }
        result?;
        let pin = PathPin::open(&path, false, true)?;
        if pin.identity != identity {
            return Err(reason);
        }
        Ok((pin, identity))
    }

    async fn inspect_golden(
        &self,
        golden: &PathPin,
        size: u64,
        generation: u64,
    ) -> Result<(), NativeRefusal> {
        let reason = NativeRefusal::PathCustodyUnverified;
        let image_path = self
            .bundle
            .root
            .path
            .join("qemu-img.exe")
            .map_err(|_| reason)?;
        let image = PathPin::open(&image_path, false, false)?;
        let image_arg = artifact(ArtifactRole::QemuImg, &image)?;
        let golden_arg = artifact(ArtifactRole::Golden, golden)?;
        let plan = crate::vm::qemu_args::golden_info(&image_arg, &golden_arg, &header(golden)?)
            .map_err(|_| reason)?;
        let pins = vec![image, bound_pin(&golden.path, golden, self.peak)?];
        let (mut launch, io) = self.command(plan, pins, generation, None, None)?;
        let deadline = self
            .deadline
            .min(tokio::time::Instant::now() + Duration::from_secs(30));
        let (output, release) = launch
            .helper(io, self.cancel, deadline, || self.live())
            .await;
        if release != StopDisposition::CleanRelease {
            return Err(reason);
        }
        crate::vm::qemu_img::parse_info(&output?, &golden.path, size, self.peak, None)
            .map(|_| ())
            .map_err(|_| reason)
    }
}

fn stock_asset(
    bundle: &inventory::Bundle,
    name: &str,
    max: u64,
    deadline: Instant,
) -> Result<PathPin, NativeRefusal> {
    let reason = NativeRefusal::DistributionUnverified;
    let path = name
        .split('/')
        .try_fold(bundle.root.path.clone(), |parent, child| parent.join(child))
        .map_err(|_| reason)?;
    let mut target = PathPin::open(&path, false, false)?;
    if target.identity != bundle.identity(name)? {
        return Err(reason);
    }
    hash_at_rest(&mut target, max, deadline)?;
    Ok(target)
}

fn held_bytes(pin: &PathPin, limit: u64) -> Result<Vec<u8>, NativeRefusal> {
    use std::io::{Seek, SeekFrom};
    let reason = NativeRefusal::PathCustodyUnverified;
    let mut file = &pin.leaf;
    file.seek(SeekFrom::Start(0)).map_err(|_| reason)?;
    super::read_bounded(file, limit).map_err(|_| reason)
}

struct BootFiles {
    disk: PathPin,
    vars: PathPin,
    backing: Option<PathPin>,
    media: Vec<(ArtifactRole, PathPin)>,
}

impl RecipeCommands<'_> {
    async fn install_or_boot(
        &self,
        files: BootFiles,
        stage: Option<InstallerStage>,
        finalization_slots: Option<crate::vm::PairSlots>,
        generation: u64,
        check: impl for<'a> FnOnce(
            &'a mut SerialGuest,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<(), NativeRefusal>> + 'a>,
        >,
    ) -> Result<Option<offline_read::ReleasedPair>, NativeRefusal> {
        use crate::vm::qemu_args::{BootPurpose, BootRecipe};
        use crate::vm::runtime::InstallerPhase;
        use ArtifactRole as A;

        let reason = NativeRefusal::InstallerPhaseUnavailable;
        let BootFiles {
            disk,
            vars,
            backing,
            media,
        } = files;
        let data_path = self.bundle.root.path.join("share").map_err(|_| reason)?;
        let image_path = self
            .bundle
            .root
            .path
            .join("qemu-system-x86_64.exe")
            .map_err(|_| reason)?;
        let data = PathPin::open(&data_path, true, false)?;
        let image = PathPin::open(&image_path, false, false)?;
        let code = stock_asset(
            self.bundle,
            "share/edk2-x86_64-code.fd",
            16 << 20,
            self.deadline.into_std(),
        )?;
        let image_arg = artifact(A::Qemu, &image)?;
        let data_arg = artifact(A::Data, &data)?;
        let code_arg = artifact(A::FirmwareCode, &code)?;
        let vars_arg = artifact(
            if stage.is_some_and(|s| s.phase() == InstallerPhase::WindowsSetup) {
                A::WindowsSetupVars
            } else {
                A::Vars
            },
            &vars,
        )?;
        let disk_arg = artifact(
            if stage.is_some() {
                A::TestInstallDisk
            } else {
                A::GuestDisk
            },
            &disk,
        )?;
        let disk_header = header(&disk)?;
        let backing_arg = backing
            .as_ref()
            .map(|pin| artifact(A::Golden, pin))
            .transpose()?;
        let backing_header = backing.as_ref().map(header).transpose()?;
        let media_args: Vec<_> = media
            .iter()
            .map(|(role, pin)| artifact(*role, pin))
            .collect::<Result<_, _>>()?;
        let boot = match (stage.map(InstallerStage::phase), media_args.as_slice()) {
            (Some(InstallerPhase::UbuntuInstall), [iso, seed, kernel, initrd]) => {
                BootPurpose::UbuntuInstall {
                    iso,
                    cidata: seed,
                    kernel,
                    initrd,
                }
            }
            (Some(InstallerPhase::WindowsSetup), [iso, answer, drivers]) => {
                BootPurpose::WindowsSetup {
                    iso,
                    answer,
                    drivers,
                }
            }
            (Some(InstallerPhase::WindowsFinalize), []) => BootPurpose::WindowsFinalize,
            (None, []) => BootPurpose::Installed,
            _ => return Err(reason),
        };
        let serial = SerialName::generate().map_err(|_| reason)?;
        let recipe = BootRecipe {
            image: &image_arg,
            data: &data_arg,
            firmware: &code_arg,
            vars: &vars_arg,
            disk: &disk_arg,
            disk_header: &disk_header,
            backing: backing_arg.as_ref(),
            backing_header: backing_header.as_deref(),
            serial: &serial,
            boot,
            ram_mib: 4096,
            vcpus: 2,
        };
        let plan = recipe.plan().map_err(|_| reason)?;
        let pins = [image, data, code, vars, disk]
            .into_iter()
            .chain(backing)
            .chain(media.into_iter().map(|(_, pin)| pin))
            .collect();
        let (mut active, io) = self.command(plan, pins, generation, stage, finalization_slots)?;
        let running = self
            .watch(async {
                Ok(active
                    .core
                    .boot(
                        Some((&self.baseline._held.token, &serial)),
                        io,
                        self.cancel,
                        || self.live(),
                        || true,
                    )
                    .await)
            })
            .await;
        match running {
            Ok(Ok(BootOutcome::InstallerReleased(pair))) => Ok(pair),
            Ok(Ok(BootOutcome::Installed(channel))) => {
                let (qmp, mut guest) = *channel;
                let tested = self.watch(check(&mut guest)).await;
                drop(guest);
                let released = active
                    .core
                    .stop(tested.as_ref().ok().map(|_| qmp), self.cancel, || true)
                    .await;
                if released != StopDisposition::CleanRelease {
                    return Err(reason);
                }
                tested?;
                Ok(None)
            }
            Ok(Err(BootFailure::Native(error))) | Err(error) => {
                active.core.stop(None, self.cancel, || true).await;
                Err(error)
            }
            Ok(Err(BootFailure::Installer(error))) => {
                if error.recovery == StopDisposition::ChargedQuarantine {
                    return Err(NativeRefusal::PathCustodyUnverified);
                }
                Err(reason)
            }
        }
    }

    async fn watch<T>(
        &self,
        operation: impl Future<Output = Result<T, NativeRefusal>>,
    ) -> Result<T, NativeRefusal> {
        tokio::pin!(operation);
        loop {
            self.live()?;
            tokio::select! {
                biased;
                _ = self.cancel.cancelled() => return Err(NativeRefusal::InstallerDeadlineExpired),
                _ = tokio::time::sleep_until(self.deadline) => return Err(NativeRefusal::InstallerDeadlineExpired),
                value = &mut operation => return value,
                _ = tokio::time::sleep(Duration::from_millis(250)) => {}
            }
        }
    }
}

/// Native recipe and its installed endpoint project to separate rows.
pub struct RecipeProbe {
    pub recipe: crate::vm::qualification_record::Observation,
    pub endpoint: crate::vm::qualification_record::Observation,
}

fn asset_pin(expected: ArtifactPin) -> crate::vm::AssetPin {
    crate::vm::AssetPin {
        length: expected.size,
        sha256: expected.sha256,
    }
}

fn content(pin: &PathPin) -> Result<ArtifactPin, NativeRefusal> {
    pin.verified_content
        .ok_or(NativeRefusal::DistributionUnverified)
}

fn open_material(
    path: &LocalPath,
    expected: Option<FileIdentity>,
    mutable: bool,
) -> Result<PathPin, NativeRefusal> {
    let pin = PathPin::open(path, false, mutable)?;
    if expected.is_some_and(|identity| identity != pin.identity) {
        return Err(NativeRefusal::PathCustodyUnverified);
    }
    Ok(pin)
}

fn cleaned_workspace(
    workspace: QualifiedWorkspace,
    expected: &[(&str, FileIdentity)],
) -> Result<(), NativeRefusal> {
    let reason = NativeRefusal::DiskCeilingUnverified;
    for &(name, id) in expected {
        sink::discard_owned(&workspace.root.leaf, name, id).map_err(|_| reason)?;
    }
    if !listing_bounded(
        &workspace.root.leaf,
        16,
        Some(Instant::now() + Duration::from_secs(2)),
    )
    .is_ok_and(|rows| rows.is_empty())
    {
        return Err(reason);
    }
    sink::discard(&workspace.charge).map_err(|_| reason)
}

pub(super) async fn run_ubuntu(
    baseline: &Baseline,
    cancel: &Shutdown,
) -> Result<RecipeProbe, NativeRefusal> {
    use crate::vm::UbuntuAssets;
    use crate::vm::qualification_record::Observation;
    use crate::vm::runtime::{InstallerBudget, InstallerPhase, InstallerRecipe};
    use crate::vm::template::{UBUNTU_DETACHED_HEALTH, installed_witness_script};
    use arx_guest::protocol::ScriptStatus;
    use sha2::{Digest, Sha256};
    use std::io::{Seek, SeekFrom};

    const DISK: u64 = 24 * GIB;
    const PEAK: u64 = 51 * GIB;
    const LIMIT: Duration = Duration::from_secs(65 * 60);
    let reason = NativeRefusal::InstallerPhaseUnavailable;
    let held = &baseline._held;
    if !held.full_test {
        return Err(NativeRefusal::QualificationMissing);
    }
    let input = held.network.as_ref().ok_or(reason)?;
    let guest = held.ubuntu_guest.as_ref().map_err(|reason| *reason)?;
    let bundle = held.bundle.as_ref().ok_or(reason)?;
    let budget = InstallerBudget::start(InstallerRecipe::Ubuntu);
    let deadline = budget.clip(LIMIT);
    let workspace = QualifiedWorkspace::claim(baseline, "ubuntu", PEAK)?;
    let cwd = held.trial.path.join("paused-cwd").map_err(|_| reason)?;
    let commands = RecipeCommands {
        baseline,
        workspace: &workspace,
        bundle,
        cwd,
        peak: PEAK,
        cancel,
        deadline,
    };
    commands.live()?;

    let [iso, kernel, initrd] = &input.media;
    let firmware = stock_asset(
        bundle,
        "share/edk2-x86_64-code.fd",
        16 << 20,
        deadline.into_std(),
    )?;
    let vars_seed = stock_asset(
        bundle,
        "share/edk2-i386-vars.fd",
        16 << 20,
        deadline.into_std(),
    )?;
    let receipt = stock_asset(bundle, "RECEIPT.txt", 16 << 10, deadline.into_std())?;
    let external = [iso, kernel, initrd, &firmware, &vars_seed, &receipt];
    let mut sources: Vec<_> = external.iter().map(|pin| &pin.leaf).collect();
    for source in &mut sources {
        source.seek(SeekFrom::Start(0)).map_err(|_| reason)?;
    }
    let [
        iso_in,
        kernel_in,
        initrd_in,
        firmware_in,
        vars_in,
        receipt_in,
    ] = sources.as_mut_slice()
    else {
        unreachable!("fixed recipe assets")
    };
    let external_pins = [
        content(iso)?,
        content(kernel)?,
        content(initrd)?,
        guest.expected,
        content(&firmware)?,
        content(&vars_seed)?,
        content(&receipt)?,
    ];
    let recipe = ubuntu_test_recipe(external_pins);
    let identity = ubuntu_identity(external_pins)?;
    let mut seed_bytes = Vec::new();
    recipe
        .write_test_seed(
            UbuntuAssets {
                iso: Some(iso_in),
                kernel: Some(kernel_in),
                initrd: Some(initrd_in),
                firmware_code: Some(firmware_in),
                firmware_vars: Some(vars_in),
                qemu_receipt: Some(receipt_in),
            },
            baseline.run(),
            &guest.read()?,
            &mut seed_bytes,
        )
        .map_err(|_| reason)?;
    commands.live()?;

    let staged = workspace.stage_bytes("ubuntu-cidata.iso", &seed_bytes)?;
    let seed_id = staged.identity;
    let seed_path = staged.path.clone();
    drop(staged);
    let mut seed = open_material(&seed_path, Some(seed_id), false)?;
    let seeded = hash_at_rest(&mut seed, 40 << 20, deadline.into_std())?;
    let seed_sha: [u8; 32] = Sha256::digest(&seed_bytes).into();
    if seeded.sha256 != seed_sha {
        return Err(reason);
    }
    let vars = workspace.stage_bytes("ubuntu-vars.fd", &held_bytes(&vars_seed, 16 << 20)?)?;
    let vars_id = vars.identity;
    let vars_path = vars.path.clone();
    let (disk, disk_id) = commands.make_disk("ubuntu.qcow2", DISK, None, 1).await?;
    let disk_path = disk.path.clone();
    let stage = budget
        .stage(InstallerPhase::UbuntuInstall, LIMIT)
        .ok_or(reason)?;
    commands
        .install_or_boot(
            BootFiles {
                disk,
                vars,
                backing: None,
                media: vec![
                    (
                        ArtifactRole::UbuntuInstallIso,
                        bound_pin(&iso.path, iso, 8 * GIB)?,
                    ),
                    (ArtifactRole::UbuntuCidata, seed),
                    (
                        ArtifactRole::UbuntuKernel,
                        bound_pin(&kernel.path, kernel, 64 << 20)?,
                    ),
                    (
                        ArtifactRole::UbuntuInitrd,
                        bound_pin(&initrd.path, initrd, 512 << 20)?,
                    ),
                ],
            },
            Some(stage),
            None,
            2,
            |_| Box::pin(async { Err(NativeRefusal::InstallerPhaseMismatch) }),
        )
        .await?;

    let mut golden = open_material(&disk_path, Some(disk_id), false)?;
    let mut golden_vars = open_material(&vars_path, Some(vars_id), false)?;
    let pinned_disk = hash_at_rest(&mut golden, DISK + GIB, deadline.into_std())?;
    let pinned_vars = hash_at_rest(&mut golden_vars, 16 << 20, deadline.into_std())?;
    commands.inspect_golden(&golden, DISK, 5).await?;
    let (overlay, overlay_id) = commands
        .make_disk(
            "ubuntu-verify.qcow2",
            DISK,
            Some(bound_pin(&disk_path, &golden, DISK + GIB)?),
            3,
        )
        .await?;
    let verify_vars = workspace.stage_bytes(
        "ubuntu-verify-vars.fd",
        &held_bytes(&golden_vars, 16 << 20)?,
    )?;
    let verify_vars_id = verify_vars.identity;
    let backing = bound_pin(&disk_path, &golden, DISK + GIB)?;
    let mut nonce = [0; 16];
    let mut boot_identity = [0; 16];
    getrandom::fill(&mut nonce).map_err(|_| reason)?;
    getrandom::fill(&mut boot_identity).map_err(|_| reason)?;
    let (witnesses, targets) = super::network::Witnesses::prepare(&input.witnesses, nonce)
        .await
        .map_err(|_| reason)?;
    let script = installed_witness_script(nonce, &targets).map_err(|_| reason);
    let isolated = match script {
        Ok(script) => {
            let signal = cancel.clone();
            commands
                .install_or_boot(
                    BootFiles {
                        disk: overlay,
                        vars: verify_vars,
                        backing: Some(backing),
                        media: vec![],
                    },
                    None,
                    None,
                    4,
                    |guest| {
                        Box::pin(async move {
                            let execute = |payload: &str| payload.to_owned();
                            if guest
                                .run_script(&signal, execute(UBUNTU_DETACHED_HEALTH), 30)
                                .await
                                != Ok((ScriptStatus::Exited, Some(0)))
                                || guest.run_script(&signal, script, 30).await
                                    != Ok((ScriptStatus::Exited, Some(0)))
                                || guest
                                    .run_script(&signal, execute("/usr/bin/sleep 60"), 2)
                                    .await
                                    != Ok((ScriptStatus::TimedOut, None))
                                || guest.run_script(&signal, execute("exit 7"), 5).await
                                    != Ok((ScriptStatus::Exited, Some(7)))
                            {
                                return Err(NativeRefusal::GuestEndpointUnverified);
                            }
                            Ok(())
                        })
                    },
                )
                .await
                .map(|_| ())
        }
        Err(error) => Err(error),
    };
    let observed = witnesses.settle().await.map_err(|_| reason)?;
    isolated?;
    if observed != 0
        || !host_dns_servers().is_ok_and(|resolvers| input.witnesses.valid(&resolvers))
        || hash_at_rest(&mut golden, DISK + GIB, deadline.into_std())? != pinned_disk
        || hash_at_rest(&mut golden_vars, 16 << 20, deadline.into_std())? != pinned_vars
    {
        return Err(NativeRefusal::NetworkFenceUnverified);
    }

    drop((golden, golden_vars));
    cleaned_workspace(
        workspace,
        &[
            ("ubuntu.qcow2", disk_id),
            ("ubuntu-vars.fd", vars_id),
            ("ubuntu-cidata.iso", seed_id),
            ("ubuntu-verify.qcow2", overlay_id),
            ("ubuntu-verify-vars.fd", verify_vars_id),
        ],
    )?;
    Ok(RecipeProbe {
        recipe: Observation::UbuntuRecipeVerified {
            recipe: identity,
            offline_installer: true,
            pinned_seed: true,
            detached_health: true,
            pair_unchanged: true,
            installed_isolation: true,
        },
        endpoint: Observation::TestInstalledEndpoint {
            recipe: identity,
            boot_identity,
            live_job_and_same_peer: true,
            nonce_bound_script: true,
            script_contained: true,
            clean_stop: true,
        },
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct WindowsEnrollment {
    iso: ReleaseImage,
    driver_medium: ReleaseImage,
    driver_files: [[ReleaseImage; 3]; 3],
    driver_receipt: ReleaseImage,
    guest: ReleaseImage,
    provisioner: AssetDigest,
    winpe_driver_letter: u8,
    index: u32,
    name: String,
    edition_id: String,
    build: String,
    language: String,
    architecture: String,
    esd: bool,
}

pub(super) struct WindowsInputs {
    config: WindowsEnrollment,
    iso: HeldAsset,
    driver_medium: HeldAsset,
    driver_files: [[HeldAsset; 3]; 3],
    driver_receipt: HeldAsset,
    guest: HeldAsset,
}

impl WindowsInputs {
    pub(super) fn open(
        config: Option<WindowsEnrollment>,
        evidence: &LocalPath,
        scope: &test::Scope,
    ) -> Result<Self, NativeRefusal> {
        let reason = NativeRefusal::InstallerPhaseUnavailable;
        let config = config.ok_or(reason)?;
        let held = |input, cap| {
            let asset = HeldAsset::open(Some(input), evidence, cap)?;
            if asset.overlaps(&scope.recipe.root.path) {
                return Err(NativeRefusal::PathCustodyUnverified);
            }
            Ok(asset)
        };
        let iso = held(&config.iso, 32 * GIB)?;
        let driver_medium = held(&config.driver_medium, 2 * GIB)?;
        let driver_receipt = held(&config.driver_receipt, 256 << 10)?;
        let guest = held(&config.guest, 32 << 20)?;
        let rows: [Result<[HeldAsset; 3], NativeRefusal>; 3] =
            config.driver_files.each_ref().map(|row| {
                let [inf, cat, sys] = row;
                Ok([
                    held(inf, 32 << 20)?,
                    held(cat, 32 << 20)?,
                    held(sys, 32 << 20)?,
                ])
            });
        let [storage, serial, network] = rows;
        let driver_files = [storage?, serial?, network?];
        let mut ids = [
            iso.pin.identity,
            driver_medium.pin.identity,
            driver_receipt.pin.identity,
            guest.pin.identity,
        ]
        .to_vec();
        for file in driver_files.iter().flatten() {
            if ids.contains(&file.pin.identity) {
                return Err(reason);
            }
            ids.push(file.pin.identity);
        }
        Ok(Self {
            config,
            iso,
            driver_medium,
            driver_files,
            driver_receipt,
            guest,
        })
    }

    fn prepare(
        &self,
        firmware: &PathPin,
        stock_vars: &PathPin,
        stock_receipt: &PathPin,
    ) -> Result<(crate::vm::template::windows::PreparedWindows, Vec<u8>), NativeRefusal> {
        use crate::vm::template::windows::{
            Edition, InstallImage, WindowsAssets, WindowsPins, WindowsRecipe,
        };
        use std::io::{Seek, SeekFrom};
        let reason = NativeRefusal::InstallerPhaseUnavailable;
        let c = &self.config;
        let edition = Edition {
            index: c.index,
            name: &c.name,
            edition_id: &c.edition_id,
            build: &c.build,
            language: &c.language,
            architecture: &c.architecture,
            container: if c.esd {
                InstallImage::Esd
            } else {
                InstallImage::Wim
            },
        };
        let mut provisioner = [0; 32];
        hex::decode_to_slice(&c.provisioner.sha256, &mut provisioner).map_err(|_| reason)?;
        let recipe = WindowsRecipe {
            pins: Some(WindowsPins {
                iso: asset_pin(self.iso.expected),
                edition,
                driver_medium: asset_pin(self.driver_medium.expected),
                driver_files: self
                    .driver_files
                    .each_ref()
                    .map(|row| row.each_ref().map(|file| asset_pin(file.expected))),
                driver_receipt: asset_pin(self.driver_receipt.expected),
                winpe_driver_letter: c.winpe_driver_letter,
                guest: asset_pin(self.guest.expected),
                provisioner: crate::vm::AssetPin {
                    length: c.provisioner.size,
                    sha256: provisioner,
                },
                firmware_code: asset_pin(content(firmware)?),
                firmware_vars: asset_pin(content(stock_vars)?),
                qemu_receipt: asset_pin(content(stock_receipt)?),
            }),
        };
        let mut drivers = self
            .driver_files
            .each_ref()
            .map(|row| row.each_ref().map(|file| &file.pin.leaf));
        for handle in drivers.iter_mut().flatten() {
            handle.seek(SeekFrom::Start(0)).map_err(|_| reason)?;
        }
        let mut iso = &self.iso.pin.leaf;
        let mut driver_medium = &self.driver_medium.pin.leaf;
        let mut firmware_code = &firmware.leaf;
        let mut firmware_vars = &stock_vars.leaf;
        for handle in [
            &mut iso,
            &mut driver_medium,
            &mut firmware_code,
            &mut firmware_vars,
        ] {
            handle.seek(SeekFrom::Start(0)).map_err(|_| reason)?;
        }
        let mut bytes = Vec::new();
        let prepared = recipe
            .write_test_seed(
                WindowsAssets {
                    iso: &mut iso,
                    edition,
                    driver_medium: &mut driver_medium,
                    driver_files: drivers
                        .each_mut()
                        .map(|row| row.each_mut().map(|file| file as &mut dyn Read)),
                    driver_receipt: &self.driver_receipt.read()?,
                    firmware_code: &mut firmware_code,
                    firmware_vars: &mut firmware_vars,
                    qemu_receipt: &held_bytes(stock_receipt, 16 << 10)?,
                },
                &self.guest.read()?,
                &mut bytes,
            )
            .map_err(|_| reason)?;
        Ok((prepared, bytes))
    }
}

const WINDOWS_IDENTITY: &str = r#"
$ErrorActionPreference='Stop'
$s = @(Get-CimInstance Win32_UserAccount -Filter "LocalAccount=True" | Where-Object SID -match '^S-1-5-21-[0-9]+-[0-9]+-[0-9]+-500$')
if ($s.Count -ne 1) { exit 0 }
$value = switch ($identityKind) {
    0 { $s[0].SID }
    1 { $env:COMPUTERNAME }
    2 { Get-ItemPropertyValue -LiteralPath 'HKLM:\SOFTWARE\Microsoft\Cryptography' -Name MachineGuid }
    default { exit 0 }
}
if ([string]::IsNullOrWhiteSpace($value)) { exit 0 }
$sha=[Security.Cryptography.SHA256]::Create()
$d=$sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($value))
exit (([BitConverter]::ToInt32($d,0) -band 0x3fffffff) + 1)
"#;

const WINDOWS_SIGNED: &str = r#"
$ErrorActionPreference='Stop'
$drivers=@(Get-CimInstance Win32_PnPSignedDriver)
foreach ($service in @('viostor','vioser','netkvm')) {
    $devices=@(Get-CimInstance Win32_PnPEntity -Filter "Service='$service'")
    if ($devices.Count -ne 1 -or $devices[0].ConfigManagerErrorCode -ne 0) { exit 1 }
    $items=@($drivers | Where-Object DeviceID -eq $devices[0].DeviceID)
    if ($items.Count -ne 1 -or !$items[0].IsSigned) { exit 1 }
}
exit 0
"#;

fn windows_witness_script(nonce: [u8; 16], targets: &super::WitnessTargets) -> String {
    use std::fmt::Write;
    let mut script = String::from("$ErrorActionPreference='Stop'\n");
    writeln!(
        script,
        "$nonce=[byte[]]@({})",
        nonce
            .iter()
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(",")
    )
    .expect("String write");
    let mut probes = String::new();
    for (i, socket) in targets.tcp.iter().enumerate() {
        writeln!(
            probes,
            "if (!(Probe '{}' {} $true $nonce {})) {{exit 1}}",
            socket.ip(),
            socket.port(),
            if i == 1 { "$true" } else { "$false" }
        )
        .expect("String write");
    }
    for (i, socket) in targets.udp.iter().enumerate().rev() {
        writeln!(
            probes,
            "if (!(Probe '{}' {} $false $nonce {})) {{exit 1}}",
            socket.ip(),
            socket.port(),
            if i == 1 { "$true" } else { "$false" }
        )
        .expect("String write");
    }
    let dns = crate::vm::guest::network_probe::dns_query(&nonce);
    writeln!(
        probes,
        "$dns=[byte[]]@({})",
        dns.iter().map(u8::to_string).collect::<Vec<_>>().join(",")
    )
    .expect("String write");
    writeln!(
        probes,
        "if (!(Probe '{}' 53 $false $dns $false)) {{exit 1}}",
        targets.dns.ip()
    )
    .expect("String write");
    probes.push_str("if (!(Probe '10.0.2.3' 53 $false $dns $false)) {exit 1}\n");
    script.push_str(r#"
$n=@(Get-NetAdapter | Where-Object Status -eq 'Up')
if ($n.Count -ne 1) {exit 1}
$i=$n[0].ifIndex
$a=@(Get-NetIPAddress -InterfaceIndex $i -AddressFamily IPv4)
if ($a.Count -ne 1 -or $a[0].IPAddress -ne '10.0.2.15' -or $a[0].PrefixLength -ne 24) {exit 1}
$r=@(Get-NetRoute -InterfaceIndex $i -AddressFamily IPv4 | Where-Object DestinationPrefix -eq '0.0.0.0/0')
if (!$r.Count -or @($r | Where-Object NextHop -ne '10.0.2.2').Count) {exit 1}
if (@(Get-NetIPAddress -InterfaceIndex $i -AddressFamily IPv6 | Where-Object IPAddress -notmatch '^fe[89ab]').Count) {exit 1}
if (@(Get-NetRoute -InterfaceIndex $i -AddressFamily IPv6 | Where-Object DestinationPrefix -eq '::/0').Count) {exit 1}

function Probe($ip,$port,$tcp,$data,$v6) {
    $af=if ($v6) {'InterNetworkV6'} else {'InterNetwork'}
    $kind=if ($tcp) {'Stream'} else {'Dgram'}
    $proto=if ($tcp) {'Tcp'} else {'Udp'}
    $s=New-Object Net.Sockets.Socket $af,$kind,$proto
    try {
        if (!$v6) {
            $s.Bind([Net.IPEndPoint]::new([Net.IPAddress]::Parse('10.0.2.15'),0))
        }
        $to=[Net.IPEndPoint]::new([Net.IPAddress]::Parse($ip),$port)
        try {
            if ($tcp) {
                $async=$s.BeginConnect($to,$null,$null)
                if (!$async.AsyncWaitHandle.WaitOne(1000)) {return (!$v6)}
                $s.EndConnect($async)
                return $false
            }
            return (!$v6 -and $s.SendTo($data,$to) -eq $data.Length)
        } catch {
            $e=$_.Exception
            while ($e -and $e -isnot [Net.Sockets.SocketException]) {$e=$e.InnerException}
            if (!$e) {return $false}
            if ($v6) {return ($e.SocketErrorCode -in @('NetworkUnreachable','AddressNotAvailable'))}
            return ($tcp -and $e.SocketErrorCode -in @('ConnectionRefused','ConnectionReset','TimedOut'))
        }
    } finally {$s.Dispose()}
}
"#);
    // Evaluate SendTo before checking the address-family expectation.
    script = script.replace(
        "return (!$v6 -and $s.SendTo($data,$to) -eq $data.Length)",
        "$sent=$s.SendTo($data,$to)\n            return (!$v6 -and $sent -eq $data.Length)",
    );
    script.push_str(&probes);
    script.push_str(r#"
$neighbor=@(Get-NetNeighbor -InterfaceIndex $i -IPAddress '10.0.2.2' | Where-Object State -eq 'Reachable')
if ($neighbor.Count -ne 1) {exit 1}
exit 0
"#);
    script
}

pub(super) async fn run_windows(
    baseline: &Baseline,
    cancel: &Shutdown,
) -> Result<RecipeProbe, NativeRefusal> {
    use crate::vm::qualification_record::{Observation, RecipeIdentity};
    use crate::vm::runtime::{InstallerBudget, InstallerPhase, InstallerRecipe};
    use crate::vm::{ArtifactEvidence, ArtifactSlot, PairSlots};
    use arx_guest::protocol::ScriptStatus;
    use std::rc::Rc;

    const DISK: u64 = 40 * GIB;
    // One candidate and one verification overlay at a time.
    const PEAK: u64 = 83 * GIB;
    const LIMIT: Duration = Duration::from_secs(3 * 60 * 60);
    let reason = NativeRefusal::InstallerPhaseUnavailable;
    let held = &baseline._held;
    if !held.full_test {
        return Err(NativeRefusal::QualificationMissing);
    }
    let network = held.network.as_ref().ok_or(reason)?;
    let setup = held.windows_setup_vars.as_ref().map_err(|reason| *reason)?;
    let input = held.windows_recipe.as_ref().map_err(|reason| *reason)?;
    let bundle = held.bundle.as_ref().ok_or(reason)?;
    let budget = InstallerBudget::start(InstallerRecipe::Windows);
    let deadline = budget.clip(LIMIT);
    let workspace = QualifiedWorkspace::claim(baseline, "windows", PEAK)?;
    let cwd = held.trial.path.join("paused-cwd").map_err(|_| reason)?;
    let commands = RecipeCommands {
        baseline,
        workspace: &workspace,
        bundle,
        cwd,
        peak: PEAK,
        cancel,
        deadline,
    };
    commands.live()?;
    let firmware = stock_asset(
        bundle,
        "share/edk2-x86_64-code.fd",
        16 << 20,
        deadline.into_std(),
    )?;
    let vars_seed = stock_asset(
        bundle,
        "share/edk2-i386-vars.fd",
        16 << 20,
        deadline.into_std(),
    )?;
    let receipt = stock_asset(bundle, "RECEIPT.txt", 16 << 10, deadline.into_std())?;
    let (prepared, seed_bytes) = input.prepare(&firmware, &vars_seed, &receipt)?;
    let identity = RecipeIdentity {
        recipe_id: crate::vm::template::windows_recipe_id(),
        recipe_sha256: prepared.digest,
    };
    let (disk, disk_id) = commands.make_disk("windows.qcow2", DISK, None, 1).await?;
    let disk_path = disk.path.clone();
    let (_, vars) = setup.stage_setup_vars(&workspace)?;
    let vars_id = vars.identity;
    let vars_path = vars.path.clone();
    let staged = workspace.stage_bytes("windows-answer.iso", &seed_bytes)?;
    let seed_id = staged.identity;
    let seed_path = staged.path.clone();
    drop(staged);
    let mut seed = open_material(&seed_path, Some(seed_id), false)?;
    let seed_pin = hash_at_rest(&mut seed, 40 << 20, deadline.into_std())?;
    if seed_pin.sha256 != crate::vm::qualification_record::digest(&seed_bytes) {
        return Err(reason);
    }
    let launch = |phase| budget.stage(phase, LIMIT).ok_or(reason);
    commands
        .install_or_boot(
            BootFiles {
                disk,
                vars,
                backing: None,
                media: vec![
                    (
                        ArtifactRole::WindowsInstallIso,
                        bound_pin(&input.iso.pin.path, &input.iso.pin, 32 * GIB)?,
                    ),
                    (ArtifactRole::WindowsAnswerSeed, seed),
                    (
                        ArtifactRole::WindowsDriverMedium,
                        bound_pin(
                            &input.driver_medium.pin.path,
                            &input.driver_medium.pin,
                            2 * GIB,
                        )?,
                    ),
                ],
            },
            Some(launch(InstallerPhase::WindowsSetup)?),
            None,
            2,
            |_| Box::pin(async { Err(NativeRefusal::InstallerPhaseMismatch) }),
        )
        .await?;
    // The source remains held and immutable. The per-run copy is expected
    // to change when Setup consumes its one-shot BootNext entry.
    setup.read()?;
    let candidate = || -> Result<BootFiles, NativeRefusal> {
        Ok(BootFiles {
            disk: open_material(&disk_path, Some(disk_id), true)?,
            vars: open_material(&vars_path, Some(vars_id), true)?,
            backing: None,
            media: vec![],
        })
    };
    let signal = cancel.clone();
    let readiness = prepared.image.probe(true);
    commands
        .install_or_boot(candidate()?, None, None, 3, |guest| {
            Box::pin(async move {
                if guest.run_script(&signal, readiness, 30).await
                    != Ok((ScriptStatus::Exited, Some(0)))
                    || guest
                        .run_script(&signal, WINDOWS_SIGNED.to_owned(), 30)
                        .await
                        != Ok((ScriptStatus::Exited, Some(0)))
                {
                    return Err(NativeRefusal::GuestEndpointUnverified);
                }
                Ok(())
            })
        })
        .await?;
    let slots = PairSlots {
        disk: ArtifactSlot {
            generation: baseline.run(),
            role: "disk",
            key: [1; 16],
        },
        vars: ArtifactSlot {
            generation: baseline.run(),
            role: "vars",
            key: [2; 16],
        },
    };
    let released = commands
        .install_or_boot(
            candidate()?,
            Some(launch(InstallerPhase::WindowsFinalize)?),
            Some(slots),
            4,
            |_| Box::pin(async { Err(NativeRefusal::InstallerPhaseMismatch) }),
        )
        .await?
        .ok_or(reason)?;
    let mut golden = open_material(&disk_path, Some(disk_id), false)?;
    let mut golden_vars = open_material(&vars_path, Some(vars_id), false)?;
    let pinned_disk = hash_at_rest(&mut golden, DISK + GIB, deadline.into_std())?;
    let pinned_vars = hash_at_rest(&mut golden_vars, 16 << 20, deadline.into_std())?;
    let proof = released
        .observe(
            slots,
            [
                ArtifactEvidence {
                    identity: disk_id,
                    directory: baseline.run(),
                    pin: pinned_disk,
                },
                ArtifactEvidence {
                    identity: vars_id,
                    directory: baseline.run(),
                    pin: pinned_vars,
                },
            ],
            deadline.into_std(),
        )
        .map_err(|_| NativeRefusal::InstallerPhaseUnavailable)?;
    if proof != (true, true) {
        return Err(reason);
    }
    drop(released);
    commands.inspect_golden(&golden, DISK, 9).await?;

    let mut identities = [[0; 3]; 2];
    let mut boot_identity = [0; 16];
    getrandom::fill(&mut boot_identity).map_err(|_| reason)?;
    for (index, identity) in identities.iter_mut().enumerate() {
        let (overlay, overlay_id) = commands
            .make_disk(
                "windows-verify.qcow2",
                DISK,
                Some(bound_pin(&disk_path, &golden, DISK + GIB)?),
                5 + index as u64 * 2,
            )
            .await?;
        let vars = workspace.stage_bytes(
            "windows-verify-vars.fd",
            &held_bytes(&golden_vars, 16 << 20)?,
        )?;
        let verify_vars_id = vars.identity;
        let backing = bound_pin(&disk_path, &golden, DISK + GIB)?;
        let mut nonce = [0; 16];
        getrandom::fill(&mut nonce).map_err(|_| reason)?;
        if !host_dns_servers().is_ok_and(|resolvers| network.witnesses.valid(&resolvers)) {
            return Err(reason);
        }
        let (witnesses, targets) = super::network::Witnesses::prepare(&network.witnesses, nonce)
            .await
            .map_err(|_| reason)?;
        let script = windows_witness_script(nonce, &targets);
        let checked = prepared.image.probe(false);
        let signal = cancel.clone();
        let fingerprint = Rc::new(Cell::new(None));
        let received = fingerprint.clone();
        let verified = commands
            .install_or_boot(
                BootFiles {
                    disk: overlay,
                    vars,
                    backing: Some(backing),
                    media: vec![],
                },
                None,
                None,
                6 + index as u64 * 2,
                |guest| {
                    Box::pin(async move {
                        if guest.run_script(&signal, checked, 30).await
                            != Ok((ScriptStatus::Exited, Some(0)))
                            || guest
                                .run_script(&signal, WINDOWS_SIGNED.to_owned(), 30)
                                .await
                                != Ok((ScriptStatus::Exited, Some(0)))
                            || guest.run_script(&signal, script, 30).await
                                != Ok((ScriptStatus::Exited, Some(0)))
                            || guest
                                .run_script(&signal, "Start-Sleep -Seconds 60".to_owned(), 2)
                                .await
                                != Ok((ScriptStatus::TimedOut, None))
                        {
                            return Err(NativeRefusal::GuestEndpointUnverified);
                        }
                        let mut fingerprints = [0; 3];
                        for (kind, fingerprint) in fingerprints.iter_mut().enumerate() {
                            let script = format!("$identityKind={kind}\n{WINDOWS_IDENTITY}");
                            match guest.run_script(&signal, script, 30).await {
                                Ok((ScriptStatus::Exited, Some(value @ 1..=0x4000_0000))) => {
                                    *fingerprint = value
                                }
                                _ => return Err(NativeRefusal::GuestEndpointUnverified),
                            }
                        }
                        received.set(Some(fingerprints));
                        Ok(())
                    })
                },
            )
            .await;
        let receipts = witnesses.settle().await.map_err(|_| reason)?;
        verified?;
        if receipts != 0
            || !host_dns_servers().is_ok_and(|resolvers| network.witnesses.valid(&resolvers))
        {
            return Err(NativeRefusal::NetworkFenceUnverified);
        }
        *identity = fingerprint.get().ok_or(reason)?;
        sink::discard_owned(&workspace.root.leaf, "windows-verify.qcow2", overlay_id)
            .map_err(|_| reason)?;
        sink::discard_owned(
            &workspace.root.leaf,
            "windows-verify-vars.fd",
            verify_vars_id,
        )
        .map_err(|_| reason)?;
    }
    if identities[0]
        .iter()
        .zip(identities[1])
        .any(|(a, b)| *a == b)
        || hash_at_rest(&mut golden, DISK + GIB, deadline.into_std())? != pinned_disk
        || hash_at_rest(&mut golden_vars, 16 << 20, deadline.into_std())? != pinned_vars
    {
        return Err(reason);
    }
    drop((golden, golden_vars));
    cleaned_workspace(
        workspace,
        &[
            ("windows.qcow2", disk_id),
            ("setup-vars.fd", vars_id),
            ("windows-answer.iso", seed_id),
        ],
    )?;
    Ok(RecipeProbe {
        recipe: Observation::WindowsRecipeVerified {
            recipe: identity,
            offline_setup_and_boot_next: true,
            signed_drivers: true,
            installed_readiness: true,
            disk_only_finalize: true,
            registry_generalized: proof.0,
            state_ini_generalized: proof.1,
            media_free_health: true,
            distinct_clone_identities: true,
            pair_unchanged: true,
            installed_isolation: true,
        },
        endpoint: Observation::TestInstalledEndpoint {
            recipe: identity,
            boot_identity,
            live_job_and_same_peer: true,
            nonce_bound_script: true,
            script_contained: true,
            clean_stop: true,
        },
    })
}

fn ubuntu_test_recipe(pins: [ArtifactPin; 7]) -> crate::vm::UbuntuRecipe {
    use crate::vm::UbuntuRecipe;
    use sha2::{Digest, Sha256};
    let [
        iso,
        kernel,
        initrd,
        guest,
        firmware_code,
        firmware_vars,
        qemu_receipt,
    ] = pins;
    UbuntuRecipe {
        iso: Some(asset_pin(iso)),
        kernel: Some(asset_pin(kernel)),
        initrd: Some(asset_pin(initrd)),
        guest: Some(asset_pin(guest)),
        service: Some(asset_pin(ArtifactPin {
            size: UbuntuRecipe::SERVICE.len() as u64,
            sha256: Sha256::digest(UbuntuRecipe::SERVICE).into(),
        })),
        finalize: Some(asset_pin(ArtifactPin {
            size: UbuntuRecipe::FINALIZE.len() as u64,
            sha256: Sha256::digest(UbuntuRecipe::FINALIZE).into(),
        })),
        nocloud_schema: Some(UbuntuRecipe::NOCLOUD_PROFILE_PIN),
        firmware_code: Some(asset_pin(firmware_code)),
        firmware_vars: Some(asset_pin(firmware_vars)),
        qemu_receipt: Some(asset_pin(qemu_receipt)),
    }
}

fn ubuntu_identity(
    pins: [ArtifactPin; 7],
) -> Result<crate::vm::qualification_record::RecipeIdentity, NativeRefusal> {
    let sha = ubuntu_test_recipe(pins)
        .test_recipe_digest()
        .map_err(|_| NativeRefusal::InstallerPhaseUnavailable)?;
    Ok(crate::vm::qualification_record::RecipeIdentity {
        recipe_id: sha[..16].try_into().expect("digest prefix"),
        recipe_sha256: sha,
    })
}

pub struct RecipeProjection {
    pub(super) install_media: [u8; 32],
    pub(super) recipes: crate::vm::qualification_record::TestRecipes,
    pub(super) firmware: [u8; 32],
    pub(super) guest_protocol: u32,
    pub(super) network_topology: [u8; 32],
    pub(super) run: [u8; 16],
    pub(super) scope: [u8; 32],
}

impl RecipeProjection {
    pub fn bind(&self, dependencies: &mut crate::vm::qualification_record::Dependencies) {
        use crate::vm::qualification_record::Measured;
        dependencies.install_media = Measured::Measured(self.install_media);
        dependencies.recipes = Measured::Measured(self.recipes.key());
        dependencies.firmware = Measured::Measured(self.firmware);
        dependencies.guest_protocol = Measured::Measured(self.guest_protocol);
        dependencies.network_topology = Measured::Measured(self.network_topology);
    }
}

pub(super) fn project_recipes(baseline: &Baseline) -> Result<RecipeProjection, NativeRefusal> {
    use crate::vm::qualification_record::{RecipeIdentity, TestRecipes, digest};
    let reason = NativeRefusal::InstallerPhaseUnavailable;
    let held = &baseline._held;
    let ubuntu_guest = held.ubuntu_guest.as_ref().map_err(|reason| *reason)?;
    let windows = held.windows_recipe.as_ref().map_err(|reason| *reason)?;
    let setup_vars = held.windows_setup_vars.as_ref().map_err(|reason| *reason)?;
    let network = held.network.as_ref().ok_or(reason)?;
    let [iso, kernel, initrd] = &network.media;
    let bundle = held.bundle.as_ref().ok_or(reason)?;
    let deadline = Instant::now() + Duration::from_secs(10);
    let code = stock_asset(bundle, "share/edk2-x86_64-code.fd", 16 << 20, deadline)?;
    let vars = stock_asset(bundle, "share/edk2-i386-vars.fd", 16 << 20, deadline)?;
    let receipt = stock_asset(bundle, "RECEIPT.txt", 16 << 10, deadline)?;
    let firmware = [content(&code)?, content(&vars)?, setup_vars.expected];
    let expected = |pin: ArtifactPin| (pin.size, pin.sha256);
    let prepared = windows.prepare(&code, &vars, &receipt)?.0;
    let recipes = TestRecipes {
        ubuntu: ubuntu_identity([
            content(iso)?,
            content(kernel)?,
            content(initrd)?,
            ubuntu_guest.expected,
            firmware[0],
            firmware[1],
            content(&receipt)?,
        ])?,
        windows: RecipeIdentity {
            recipe_id: crate::vm::template::windows_recipe_id(),
            recipe_sha256: prepared.digest,
        },
    };
    let key = |bytes: Vec<u8>| digest(&bytes);
    Ok(RecipeProjection {
        run: baseline.run(),
        scope: baseline.scope(),
        install_media: key(serde_json::to_vec(&(
            "arx-vm-install-media-v1",
            network.projection,
            expected(windows.iso.expected),
            expected(windows.driver_medium.expected),
            expected(windows.driver_receipt.expected),
        ))
        .map_err(|_| reason)?),
        recipes,
        firmware: key(
            serde_json::to_vec(&("arx-vm-firmware-v1", firmware.map(expected)))
                .map_err(|_| reason)?,
        ),
        guest_protocol: arx_guest::protocol::PROTOCOL_VERSION,
        network_topology: key(serde_json::to_vec(&(
            "arx-vm-test-witnesses-v3",
            crate::vm::qemu_args::NETWORK,
            network.projection,
        ))
        .map_err(|_| reason)?),
    })
}

pub(super) fn revalidate_assets(baseline: &Baseline) -> Result<(), NativeRefusal> {
    let held = &baseline._held;
    let reason = NativeRefusal::InstallerPhaseUnavailable;
    let windows = held.windows_recipe.as_ref().map_err(|r| *r)?;
    let setup = held.windows_setup_vars.as_ref().map_err(|r| *r)?;
    let ubuntu = held.ubuntu_guest.as_ref().map_err(|r| *r)?;
    let network = held.network.as_ref().ok_or(reason)?;
    let assets = [
        &windows.iso,
        &windows.driver_medium,
        &windows.driver_receipt,
        &windows.guest,
        setup,
        ubuntu,
    ]
    .into_iter()
    .chain(windows.driver_files.iter().flatten());
    let release = held._release.as_ref().ok_or(reason)?;
    for (pin, expected) in assets.map(|asset| (&asset.pin, asset.expected)).chain(
        network
            .media
            .iter()
            .chain(release)
            .map(|pin| content(pin).map(|expected| (pin, expected)))
            .collect::<Result<Vec<_>, _>>()?,
    ) {
        inventory::binding(pin)?;
        if pin.mutable || pin.leaf.metadata().map_err(|_| reason)?.len() != expected.size {
            return Err(reason);
        }
    }
    if !host_dns_servers().is_ok_and(|resolvers| network.witnesses.valid(&resolvers)) {
        return Err(NativeRefusal::NetworkFenceUnverified);
    }
    held.bundle
        .as_ref()
        .ok_or(reason)?
        .check(Instant::now() + Duration::from_secs(1))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_vars_fixture_checks_pin_and_copies_into_a_distinct_generation() {
        use std::io::{Seek, SeekFrom, Write};
        let temp = tempfile::tempdir().unwrap();
        let path = local(temp.path().to_str().unwrap()).unwrap();
        let seed = path.join("source.fd").unwrap();
        let evidence = path.join("evidence").unwrap();
        let bytes = b"synthetic pinned setup vars";
        std::fs::write(seed.as_str(), bytes).unwrap();
        let mut enrollment = ReleaseImage {
            path: seed.as_str().to_owned(),
            size: bytes.len() as u64,
            sha256: hex::encode(crate::vm::qualification_record::digest(bytes)),
        };
        let reject = NativeRefusal::InstallerPhaseUnavailable;
        assert_eq!(HeldAsset::open(None, &evidence, 1024).err(), Some(reject));
        enrollment.size += 1;
        assert_eq!(
            HeldAsset::open(Some(&enrollment), &evidence, 1024).err(),
            Some(reject)
        );
        enrollment.size -= 1;
        let expected = std::mem::replace(&mut enrollment.sha256, "13".repeat(32));
        assert_eq!(
            HeldAsset::open(Some(&enrollment), &evidence, 1024).err(),
            Some(reject)
        );
        enrollment.sha256 = expected;
        let source = HeldAsset::open(Some(&enrollment), &evidence, 1024).unwrap();
        let directory = path.join("workspace").unwrap();
        std::fs::create_dir(directory.as_str()).unwrap();
        let workspace = QualifiedWorkspace {
            root: PathPin::open(&directory, true, false).unwrap(),
            charge: File::create(temp.path().join("charge")).unwrap(),
            run: [1; 16],
        };
        let (artifact, mut copy) = source.stage_setup_vars(&workspace).unwrap();
        assert_eq!(artifact.pin(), Some(source.expected));
        assert_ne!(copy.identity, source.pin.identity);
        assert_eq!(held_bytes(&copy, 1024).unwrap(), bytes);
        assert!(
            std::fs::OpenOptions::new()
                .write(true)
                .open(seed.as_str())
                .is_err()
        );
        let mut writer = std::fs::OpenOptions::new()
            .write(true)
            .open(copy.path.as_str())
            .unwrap();
        writer.seek(SeekFrom::Start(0)).unwrap();
        writer.write_all(b"consumed").unwrap();
        drop(writer);
        assert!(copy.verify_content(source.expected, 1024).is_err());
        assert_eq!(source.read().unwrap(), bytes);
    }

    #[test]
    fn installed_witness_scripts_fit_the_same_held_guest_channel_limit() {
        let tcp = [
            "192.168.254.254:65535".parse().unwrap(),
            "[ffff:eeee:dddd:cccc:bbbb:aaaa:9999:8888]:65534"
                .parse()
                .unwrap(),
            "10.0.2.2:65533".parse().unwrap(),
        ];
        let targets = super::super::WitnessTargets {
            tcp,
            udp: tcp,
            dns: "192.168.254.254:53".parse().unwrap(),
        };
        let nonce = [255; 16];
        let linux = crate::vm::template::installed_witness_script(nonce, &targets).unwrap();
        let windows = windows_witness_script(nonce, &targets);
        assert!(linux.ends_with("ARX_NETWORK"));
        assert!(windows.len() <= arx_guest::protocol::MAX_SCRIPT);
        assert!(windows.contains("$sent=$s.SendTo"));
    }

    #[test]
    fn ubuntu_identity_uses_single_embedded_recipe_digest() {
        let pin = |seed: u8| ArtifactPin {
            size: u64::from(seed) + 1,
            sha256: [seed; 32],
        };
        let pins = [pin(1), pin(2), pin(3), pin(4), pin(5), pin(6), pin(7)];
        let identity = ubuntu_identity(pins).unwrap();
        let expected = ubuntu_test_recipe(pins).test_recipe_digest().unwrap();
        assert_eq!(identity.recipe_sha256, expected);
        assert_eq!(identity.recipe_id, expected[..16]);

        let mut other = pins;
        other[3].sha256[0] ^= 1;
        assert_ne!(ubuntu_identity(other).unwrap(), identity);
    }

    #[tokio::test]
    async fn windows_installed_witness_script_behavior_with_fake_sockets_and_cmdlets() {
        use tokio::io::AsyncWriteExt;

        let nonce = [0xef; 16];
        let tcp = [
            "192.0.2.10:42001".parse().unwrap(),
            "[2001:db8::10]:42002".parse().unwrap(),
            "10.0.2.2:42003".parse().unwrap(),
        ];
        let udp = [
            "192.0.2.10:42004".parse().unwrap(),
            "[2001:db8::10]:42005".parse().unwrap(),
            "10.0.2.2:42006".parse().unwrap(),
        ];
        let targets = super::super::WitnessTargets {
            tcp,
            udp,
            dns: "192.0.2.53:53".parse().unwrap(),
        };
        let emitted = windows_witness_script(nonce, &targets);
        let temp = tempfile::tempdir().unwrap();
        let script_path = temp.path().join("witness.ps1");
        std::fs::write(&script_path, &emitted).unwrap();

        let expected_dns = crate::vm::guest::network_probe::dns_query(&nonce);
        let nonce_csv = nonce
            .iter()
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let dns_csv = expected_dns
            .iter()
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(",");

        let harness = format!(
            r#"$ErrorActionPreference = 'Stop'
$global:arxScriptPath = $env:ARX_WITNESS_SCRIPT
$global:arxExpectedNonce = [byte[]]@({nonce_csv})
$global:arxExpectedDns = [byte[]]@({dns_csv})

function Invoke-WitnessCase {{
    param([hashtable]$Config)
    $global:arxCfg = $Config
    $global:arxAttempts = [System.Collections.Generic.List[string]]::new()
    $global:arxUdpAliasSent = $false

    function Get-NetAdapter {{
        return @([pscustomobject]@{{ Status = 'Up'; ifIndex = 7 }})
    }}
    function Get-NetIPAddress {{
        param([int]$InterfaceIndex, [string]$AddressFamily)
        if ($InterfaceIndex -ne 7) {{ throw "wrong ifIndex $InterfaceIndex" }}
        if ($AddressFamily -eq 'IPv4') {{
            if ($global:arxCfg.ContainsKey('ipv4')) {{ return $global:arxCfg.ipv4 }}
            return @([pscustomobject]@{{ IPAddress = '10.0.2.15'; PrefixLength = 24 }})
        }}
        if ($global:arxCfg.ContainsKey('ipv6')) {{ return $global:arxCfg.ipv6 }}
        return @([pscustomobject]@{{ IPAddress = 'fe80::1' }})
    }}
    function Get-NetRoute {{
        param([int]$InterfaceIndex, [string]$AddressFamily)
        if ($InterfaceIndex -ne 7) {{ throw "wrong ifIndex $InterfaceIndex" }}
        if ($AddressFamily -eq 'IPv4') {{
            if ($global:arxCfg.ContainsKey('v4route')) {{ return $global:arxCfg.v4route }}
            return @([pscustomobject]@{{ DestinationPrefix = '0.0.0.0/0'; NextHop = '10.0.2.2' }})
        }}
        if ($global:arxCfg.ContainsKey('v6route')) {{ return $global:arxCfg.v6route }}
        return @()
    }}
    function Get-NetNeighbor {{
        param([int]$InterfaceIndex, [string]$IPAddress)
        if ($InterfaceIndex -ne 7 -or $IPAddress -ne '10.0.2.2') {{ throw 'wrong neighbor query' }}
        if (-not $global:arxUdpAliasSent) {{
            return @([pscustomobject]@{{ State = 'Stale' }})
        }}
        if ($global:arxCfg.ContainsKey('neighbor')) {{ return $global:arxCfg.neighbor }}
        return @([pscustomobject]@{{ State = 'Reachable' }})
    }}
    function New-Object {{
        param([string]$TypeName, [object[]]$ArgumentList)
        if ($TypeName -ne 'Net.Sockets.Socket') {{
            if ($null -eq $ArgumentList) {{
                return Microsoft.PowerShell.Utility\New-Object -TypeName $TypeName
            }}
            return Microsoft.PowerShell.Utility\New-Object -TypeName $TypeName -ArgumentList $ArgumentList
        }}
        $af, $kind, $proto = $ArgumentList
        $sock = [pscustomobject]@{{
            Af = $af
            Kind = $kind
            Proto = $proto
            Bound = $null
            Disposed = $false
        }}
        $sock | Add-Member -MemberType ScriptMethod -Name Bind -Value {{
            param($ep)
            $this.Bound = $ep.Address.ToString()
        }}
        $sock | Add-Member -MemberType ScriptMethod -Name BeginConnect -Value {{
            param($to, $cb, $state)
            $key = "tcp:$($to.Address):$($to.Port)"
            if ($this.Af -eq 'InterNetwork' -and $this.Bound -ne '10.0.2.15') {{ throw 'missing ipv4 bind' }}
            if ($this.Af -eq 'InterNetworkV6' -and $null -ne $this.Bound) {{ throw 'unexpected ipv6 bind' }}
            $global:arxAttempts.Add($key)
            $action = if ($global:arxCfg.overrides -and $global:arxCfg.overrides.ContainsKey($key)) {{ $global:arxCfg.overrides[$key] }} else {{ $null }}
            $waited = $true
            if ($null -eq $action) {{
                if ($key -eq 'tcp:192.0.2.10:42001') {{ $waited = $false }}
                elseif ($key -eq 'tcp:2001:db8::10:42002') {{ $action = 'NetworkUnreachable' }}
                elseif ($key -eq 'tcp:10.0.2.2:42003') {{ $action = 'ConnectionRefused' }}
                else {{ throw "unexpected tcp target $key" }}
            }} elseif ($action -eq 'timeout') {{
                $waited = $false
            }}
            $wh = [pscustomobject]@{{ Waited = $waited }}
            $wh | Add-Member -MemberType ScriptMethod -Name WaitOne -Value {{ param($ms) return $this.Waited }}
            return [pscustomobject]@{{ AsyncWaitHandle = $wh; Action = $action }}
        }}
        $sock | Add-Member -MemberType ScriptMethod -Name EndConnect -Value {{
            param($async)
            if ($async.Action -eq 'connect') {{ return }}
            if ($async.Action) {{
                $code = [int][Enum]::Parse([System.Net.Sockets.SocketError], $async.Action)
                throw ([System.Net.Sockets.SocketException]::new($code))
            }}
        }}
        $sock | Add-Member -MemberType ScriptMethod -Name SendTo -Value {{
            param([byte[]]$data, $to)
            $key = "udp:$($to.Address):$($to.Port)"
            if ($this.Af -eq 'InterNetwork' -and $this.Bound -ne '10.0.2.15') {{ throw 'missing ipv4 bind' }}
            if ($this.Af -eq 'InterNetworkV6' -and $null -ne $this.Bound) {{ throw 'unexpected ipv6 bind' }}
            $global:arxAttempts.Add($key)
            if ($to.Port -eq 53) {{
                if (([Convert]::ToBase64String($data)) -ne ([Convert]::ToBase64String($global:arxExpectedDns))) {{ throw 'bad dns query' }}
            }} else {{
                if (([Convert]::ToBase64String($data)) -ne ([Convert]::ToBase64String($global:arxExpectedNonce))) {{ throw 'bad udp nonce' }}
            }}
            if ($key -eq 'udp:10.0.2.2:42006') {{ $global:arxUdpAliasSent = $true }}
            $action = if ($global:arxCfg.overrides -and $global:arxCfg.overrides.ContainsKey($key)) {{ $global:arxCfg.overrides[$key] }} else {{ $null }}
            if ($null -eq $action) {{
                if ($key -eq 'udp:2001:db8::10:42005') {{
                    throw ([System.Net.Sockets.SocketException]::new([int][System.Net.Sockets.SocketError]::AddressNotAvailable))
                }}
                return $data.Length
            }}
            if ($action -eq 'short') {{ return ($data.Length - 1) }}
            $code = [int][Enum]::Parse([System.Net.Sockets.SocketError], $action)
            throw ([System.Net.Sockets.SocketException]::new($code))
        }}
        $sock | Add-Member -MemberType ScriptMethod -Name Dispose -Value {{ $this.Disposed = $true }}
        return $sock
    }}

    $global:LASTEXITCODE = 0
    & $global:arxScriptPath
    return [pscustomobject]@{{
        ExitCode = $global:LASTEXITCODE
        Attempts = @($global:arxAttempts)
    }}
}}

$expectedAttempts = @(
    'tcp:192.0.2.10:42001',
    'tcp:2001:db8::10:42002',
    'tcp:10.0.2.2:42003',
    'udp:10.0.2.2:42006',
    'udp:2001:db8::10:42005',
    'udp:192.0.2.10:42004',
    'udp:192.0.2.53:53',
    'udp:10.0.2.3:53'
)

# 1. Reachable control and complete 8-attempt set.
$ok = Invoke-WitnessCase @{{}}
if ($ok.ExitCode -ne 0) {{ throw "happy path failed: $($ok.ExitCode)" }}
if (($ok.Attempts -join ',') -ne ($expectedAttempts -join ',')) {{
    throw "wrong attempts: $($ok.Attempts -join ',')"
}}

# 2. Unreachable gateway neighbor fails closed.
$stale = Invoke-WitnessCase @{{ neighbor = @([pscustomobject]@{{ State = 'Stale' }}) }}
if ($stale.ExitCode -ne 1) {{ throw 'stale neighbor should fail' }}

# 3. Local IPv4 errors fail closed.
foreach ($ov in @(
    @{{ 'tcp:192.0.2.10:42001' = 'NetworkUnreachable' }},
    @{{ 'tcp:10.0.2.2:42003' = 'HostUnreachable' }},
    @{{ 'udp:192.0.2.10:42004' = 'NetworkUnreachable' }},
    @{{ 'udp:192.0.2.53:53' = 'short' }}
)) {{
    $r = Invoke-WitnessCase @{{ overrides = $ov }}
    if ($r.ExitCode -ne 1) {{ throw 'local ipv4 error should fail' }}
}}

# 4. Absent/wrong route, global IPv6, unexpected TCP connect, or IPv6 timeout fail closed.
foreach ($case in @(
    @{{ v4route = @() }},
    @{{ v4route = @([pscustomobject]@{{ DestinationPrefix = '0.0.0.0/0'; NextHop = '10.0.2.1' }}) }},
    @{{ ipv4 = @([pscustomobject]@{{ IPAddress = '10.0.2.99'; PrefixLength = 24 }}) }},
    @{{ ipv6 = @([pscustomobject]@{{ IPAddress = '2001:db8::1' }}) }},
    @{{ v6route = @([pscustomobject]@{{ DestinationPrefix = '::/0'; NextHop = 'fe80::1' }}) }},
    @{{ overrides = @{{ 'tcp:192.0.2.10:42001' = 'connect' }} }},
    @{{ overrides = @{{ 'tcp:2001:db8::10:42002' = 'timeout' }} }}
)) {{
    $r = Invoke-WitnessCase $case
    if ($r.ExitCode -ne 1) {{ throw 'negative case should fail' }}
}}
"#
        );

        let powershell = std::path::PathBuf::from(
            std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into()),
        )
        .join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
        assert!(
            powershell.is_absolute(),
            "expected absolute Windows PowerShell path"
        );
        let mut check = tokio::process::Command::new(powershell)
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                "& ([ScriptBlock]::Create([Console]::In.ReadToEnd()))",
            ])
            .env("ARX_WITNESS_SCRIPT", &script_path)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let result = tokio::time::timeout(Duration::from_secs(120), async move {
            let mut input = check.stdin.take().unwrap();
            input.write_all(harness.as_bytes()).await.unwrap();
            drop(input);
            check.wait_with_output().await.unwrap()
        })
        .await
        .expect("PowerShell witness fixture exceeded its test budget");
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }

    #[test]
    fn recipe_launch_hold_binds_permit_run_generation_and_baseline_before_create() {
        use crate::vm::{paths::Artifact, qemu_args::golden_info, qemu_img::CLUSTER};
        let payload = vec![0; 2_520_350];
        let fixture = inventory::tests::Fixture::with_payload("qemu-img.exe", &payload);
        let other = inventory::tests::Fixture::with_payload("qemu-img.exe", &payload);
        let deadline = Instant::now() + Duration::from_secs(5);
        let bundle = fixture.inspect(deadline).unwrap();
        let other = other.inspect(deadline).unwrap();
        let root = tempfile::tempdir().unwrap();
        let recipe_root =
            PathPin::open(&local(root.path().to_str().unwrap()).unwrap(), true, false).unwrap();
        let root_path = recipe_root.path.join("recipe-run").unwrap();
        std::fs::create_dir(root_path.as_str()).unwrap();
        let workspace = QualifiedWorkspace {
            root: PathPin::open(&root_path, true, false).unwrap(),
            charge: File::create(root.path().join("charge")).unwrap(),
            run: [1; 16],
        };
        let cwd = recipe_root.path.join("cwd").unwrap();
        let image = Artifact::new(
            ArtifactRole::QemuImg,
            bundle.root.path.join("qemu-img.exe").unwrap(),
        )
        .unwrap();
        let golden = Artifact::new(
            ArtifactRole::Golden,
            root_path.join("golden.qcow2").unwrap(),
        )
        .unwrap();
        let mut header = vec![0; CLUSTER];
        header[..4].copy_from_slice(b"QFI\xfb");
        for (at, val) in [(4, 3u32), (20, 16), (96, 4), (100, 112)] {
            header[at..at + 4].copy_from_slice(&val.to_be_bytes());
        }
        header[24..32].copy_from_slice(&1_048_576u64.to_be_bytes());
        for (
            full_test,
            foreign_bundle,
            recipe_root,
            baseline_run,
            permit_run,
            permit_generation,
            accepted,
        ) in [
            (true, false, Some(&recipe_root), [1; 16], [1; 16], 1, true),
            (false, false, Some(&recipe_root), [1; 16], [1; 16], 1, false),
            (true, true, Some(&recipe_root), [1; 16], [1; 16], 1, false),
            (
                true,
                false,
                Some(&workspace.root),
                [1; 16],
                [1; 16],
                1,
                false,
            ),
            (true, false, None, [1; 16], [1; 16], 1, false),
            (true, false, Some(&recipe_root), [2; 16], [1; 16], 1, false),
            (true, false, Some(&recipe_root), [1; 16], [2; 16], 1, false),
            (true, false, Some(&recipe_root), [1; 16], [1; 16], 2, false),
        ] {
            let (custody, mut permit) = workspace.hold(1).unwrap();
            permit.run = permit_run;
            permit.generation = permit_generation;
            let outcome = QualifierLaunch::hold(
                permit,
                HeldRecipeScope {
                    full_test,
                    run: baseline_run,
                    bundle: Some(if foreign_bundle { &other } else { &bundle }),
                    recipe_root,
                },
                LaunchInputs {
                    plan: golden_info(&image, &golden, &header).unwrap(),
                    cwd: &cwd,
                    workspace: custody,
                    bundle: &bundle,
                    memory: JOB_MEMORY_BYTES,
                    cpu_rate: JOB_CPU_RATE,
                    installer: None,
                    finalization_slots: None,
                    pins: Vec::new(),
                },
                |_| Ok(()),
            );
            assert_eq!(
                outcome,
                if accepted {
                    Ok(())
                } else {
                    Err(NativeRefusal::QualificationMissing)
                }
            );
        }
    }

    #[test]
    fn inspect_requires_held_workspace_and_both_verified_inputs() {
        use crate::vm::{paths::Artifact, qemu_args::golden_info, qemu_img::CLUSTER};
        use sha2::{Digest, Sha256};
        let fixture = inventory::tests::Fixture::with_payload("qemu-img.exe", &vec![0; 2_520_350]);
        let bundle = fixture
            .inspect(Instant::now() + Duration::from_secs(5))
            .unwrap();
        let root = tempfile::tempdir().unwrap();
        let path = local(root.path().to_str().unwrap()).unwrap();
        let slot = File::create(root.path().join("slot")).unwrap();
        let workspace_root = PathPin::open(&path, true, false).unwrap();
        let custody = WorkspaceCustody::new([1; 16], 1, &workspace_root, &slot).unwrap();
        assert!(WorkspaceCustody::new([0; 16], 1, &workspace_root, &slot).is_err());
        assert!(WorkspaceCustody::new([1; 16], 0, &workspace_root, &slot).is_err());

        let image_path = bundle.root.path.join("qemu-img.exe").unwrap();
        let image = Artifact::new(ArtifactRole::QemuImg, image_path.clone()).unwrap();
        let mut header = vec![0; CLUSTER];
        header[..4].copy_from_slice(b"QFI\xfb");
        for (at, val) in [(4, 3u32), (20, 16), (96, 4), (100, 112)] {
            header[at..at + 4].copy_from_slice(&val.to_be_bytes());
        }
        header[24..32].copy_from_slice(&1_048_576u64.to_be_bytes());
        std::fs::write(root.path().join("golden"), &header).unwrap();
        let disk_path = path.join("golden").unwrap();
        let disk = PathPin::open(&disk_path, false, false).unwrap();
        let digest = ArtifactPin {
            size: header.len() as u64,
            sha256: Sha256::digest(&header).into(),
        };
        let golden = Artifact::new(ArtifactRole::Golden, disk_path)
            .unwrap()
            .with_identity(disk.identity)
            .with_pin(digest);
        let plan = golden_info(&image, &golden, &header).unwrap();
        let mut pins = [PathPin::open(&image_path, false, false).unwrap(), disk];
        let cwd = path.join("cwd").unwrap();
        let check =
            |cwd, pins: &[PathPin]| StockLaunch::verify_inputs(&bundle, cwd, &plan, pins, &custody);
        assert!(check(&cwd, &pins).is_err());
        pins[1].verify_content(digest, digest.size).unwrap();
        assert_eq!(check(&cwd, &pins), Ok(vec![]));
        assert!(check(&path, &pins).is_err());
        assert!(check(&cwd, &pins[..1]).is_err());
    }

    #[test]
    fn qualifier_plan_kind_is_restricted_even_without_a_lease() {
        let permit = QualifierPlanPermit {
            run: [1; 16],
            generation: 1,
        };
        for kind in [
            PlanKind::InspectWritable,
            PlanKind::Mutate,
            PlanKind::StaticWebBoot,
            PlanKind::PublicEgressBoot,
        ] {
            assert_eq!(
                permit.check(permit.run, 1, kind, false),
                Err(NativeRefusal::QualificationMissing)
            );
        }
        assert_eq!(
            permit.check(permit.run, 1, PlanKind::Inspect, false),
            Ok(())
        );
    }

    #[test]
    fn qualifier_refuses_an_offline_lease_even_for_allowed_plan_kinds() {
        let permit = QualifierPlanPermit {
            run: [1; 16],
            generation: 1,
        };
        assert_eq!(
            permit.check(permit.run, 1, PlanKind::Boot, true),
            Err(NativeRefusal::QualificationMissing)
        );
        assert_eq!(
            permit.check(permit.run, 2, PlanKind::Boot, false),
            Err(NativeRefusal::QualificationMissing)
        );
        assert_eq!(
            permit.check([2; 16], 1, PlanKind::Boot, false),
            Err(NativeRefusal::QualificationMissing)
        );
    }
}
