//! Production plans derived from the selected TEST recipe and reserved slots.

use super::launch::{artifact, header, held_bytes, prepare_ubuntu};
use super::production::{CustodyCheckpoints, ProductionWorkspace, WorkspaceBinding, check_pair};
use super::*;
use crate::vm::paths::Artifact;
use crate::vm::qemu_args::{BootPurpose, BootRecipe};
use crate::vm::qualification_record::{Hash, RecipeIdentity, TestRecipes, digest};
use crate::vm::runtime::{InstallerPhase, StageAuthority};
use crate::vm::{ArtifactEvidence, ArtifactSlot, PairSlots};

const SEED_LIMIT: u64 = 40 << 20;
pub(super) const TEST_JOB_MEMORY: usize = 6 * GIB as usize;
const PATH: NativeRefusal = NativeRefusal::PathCustodyUnverified;
const PHASE: NativeRefusal = NativeRefusal::InstallerPhaseMismatch;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct StageTarget {
    pub workspace: WorkspaceBinding,
    pub seal_sha256: Hash,
    pub recipe: RecipeIdentity,
    pub phase: InstallerPhase,
    pub slot: ArtifactSlot,
    pub content: ArtifactPin,
    pub deadline: Instant,
}

pub(crate) struct InstallerReservation<'a> {
    pub workspace: &'a ProductionWorkspace,
    pub recipe: RecipeIdentity,
    pub phase: InstallerPhase,
    pub pair: PairSlots,
    pub seed: ArtifactSlot,
    pub deadline: Instant,
}

/// Retains the workspace lease and immutable seed. The writable VARS stays
/// pinned by identity and is independently content-checked at checked create.
pub(crate) struct StagedInstaller<'a> {
    reservation: InstallerReservation<'a>,
    seal_sha256: Hash,
    seed: PathPin,
    vars: PathPin,
    media: Vec<Artifact>,
}

impl StagedInstaller<'_> {
    pub(crate) fn artifacts(&self) -> [(ArtifactSlot, ArtifactEvidence); 2] {
        [
            (self.reservation.seed, &self.seed),
            (self.reservation.pair.vars, &self.vars),
        ]
        .map(|(slot, pin)| {
            (
                slot,
                ArtifactEvidence {
                    identity: pin.identity,
                    directory: self.reservation.workspace.binding().workspace.file,
                    pin: pin.verified_content.expect("staging verified the content"),
                },
            )
        })
    }
}

pub(crate) struct BootReservation<'a> {
    pub workspace: &'a ProductionWorkspace,
    pub recipe: RecipeIdentity,
    pub phase: Option<InstallerPhase>,
    pub pair: PairSlots,
    pub identities: [FileIdentity; 2],
    /// A ledger-pinned golden for an installed linked clone.
    pub backing: Option<&'a Artifact>,
    pub staged: Option<&'a StagedInstaller<'a>>,
    /// The authorized content of an installed boot's VARS copy. The checked
    /// launch reconfirms it under writer exclusion until QEMU opens it.
    pub vars_content: Option<ArtifactPin>,
}

pub(crate) struct QualifiedBoot<'a> {
    binding: WorkspaceBinding,
    seal_sha256: Hash,
    recipe: RecipeIdentity,
    pair: PairSlots,
    phase: Option<InstallerPhase>,
    cwd: LocalPath,
    serial: SerialName,
    image: Artifact,
    data: Artifact,
    firmware: Artifact,
    disk: Artifact,
    vars: Artifact,
    disk_header: Vec<u8>,
    backing: Option<Artifact>,
    backing_header: Option<Vec<u8>>,
    media: Vec<Artifact>,
    _reservation: BootReservation<'a>,
    _pair_pins: [PathPin; 2],
    _backing_pin: Option<PathPin>,
}

impl QualifiedBoot<'_> {
    pub(crate) fn cwd(&self) -> &LocalPath {
        &self.cwd
    }

    pub(crate) fn serial(&self) -> &SerialName {
        &self.serial
    }

    pub(crate) fn planner(&self) -> QualifiedBootPlan<'_> {
        let boot = match (self.phase, self.media.as_slice()) {
            (Some(InstallerPhase::UbuntuInstall), [iso, cidata, kernel, initrd]) => {
                BootPurpose::UbuntuInstall {
                    iso,
                    cidata,
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
            _ => unreachable!("validated recipe inputs"),
        };
        QualifiedBootPlan(BootRecipe {
            image: &self.image,
            data: &self.data,
            firmware: &self.firmware,
            vars: &self.vars,
            disk: &self.disk,
            disk_header: &self.disk_header,
            backing: self.backing.as_ref(),
            backing_header: self.backing_header.as_deref(),
            serial: &self.serial,
            boot,
            ram_mib: 4096,
            vcpus: 2,
        })
    }

    pub(super) fn recipe_identity(&self) -> RecipeIdentity {
        self.recipe
    }

    pub(super) fn verify(
        &self,
        binding: WorkspaceBinding,
        seal_sha256: Hash,
        selected: TestRecipes,
        pair: Option<PairSlots>,
        cwd: &LocalPath,
        plan: &CommandPlan<'_>,
    ) -> Result<(), NativeRefusal> {
        check_boot_binding(self.binding, binding, self.seal_sha256, seal_sha256)?;
        if !recipe_selected(self.recipe, self.phase, selected)
            || pair != Some(self.pair)
            || cwd != self.cwd()
        {
            return Err(PHASE);
        }
        let planner = self.planner();
        let expected = planner.plan()?;
        check_boot_plan(&expected, plan)
    }
}

pub(crate) struct QualifiedBootPlan<'a>(BootRecipe<'a>);

impl QualifiedBootPlan<'_> {
    pub(crate) fn plan(&self) -> Result<CommandPlan<'_>, NativeRefusal> {
        self.0.plan().map_err(|_| PHASE)
    }
}

fn check_boot_binding(
    issued: WorkspaceBinding,
    actual: WorkspaceBinding,
    issued_seal: Hash,
    selected_seal: Hash,
) -> Result<(), NativeRefusal> {
    (issued == actual && issued_seal == selected_seal)
        .then_some(())
        .ok_or(PATH)
}

fn check_boot_plan(
    expected: &CommandPlan<'_>,
    supplied: &CommandPlan<'_>,
) -> Result<(), NativeRefusal> {
    let inputs = expected.inputs();
    let offered = supplied.inputs();
    (expected.kind() == supplied.kind()
        && expected.image() == supplied.image()
        && expected.args() == supplied.args()
        && inputs.len() == offered.len()
        && inputs
            .iter()
            .zip(offered)
            .all(|((role, expected), (other_role, supplied))| {
                role == other_role
                    && expected.path_for(*role) == supplied.path_for(*role)
                    && expected.identity() == supplied.identity()
                    && expected.pin() == supplied.pin()
            }))
    .then_some(())
    .ok_or(PHASE)
}

/// Shared by staging and the TEST issuer: each installer phase belongs to
/// one selected recipe, and a boot without a phase to either.
pub(crate) fn recipe_selected(
    recipe: RecipeIdentity,
    phase: Option<InstallerPhase>,
    selected: TestRecipes,
) -> bool {
    match phase {
        Some(InstallerPhase::UbuntuInstall) => recipe == selected.ubuntu,
        Some(InstallerPhase::WindowsSetup | InstallerPhase::WindowsFinalize) => {
            recipe == selected.windows
        }
        None => recipe == selected.ubuntu || recipe == selected.windows,
    }
}

/// Shared by staging and the TEST issuer: a staged write targets only the
/// build's seed or VARS slot, and only in a phase that stages installer
/// media. Windows finalization stages nothing.
pub(crate) fn stage_supported(slot: ArtifactSlot, phase: InstallerPhase) -> bool {
    matches!(slot.role, "seed" | "vars")
        && matches!(
            phase,
            InstallerPhase::UbuntuInstall | InstallerPhase::WindowsSetup
        )
}

impl ForegroundQualification<'_> {
    pub(crate) fn stage_installer<'a>(
        &self,
        reservation: InstallerReservation<'a>,
        ledger: &dyn CustodyCheckpoints,
        cancelled: &dyn Fn() -> bool,
        mut authorize: impl FnMut(StageTarget) -> Result<StageAuthority, NativeRefusal>,
    ) -> Result<StagedInstaller<'a>, NativeRefusal> {
        let InstallerReservation {
            workspace,
            recipe,
            phase,
            pair,
            seed,
            deadline,
        } = reservation;
        let binding = self.check_workspace(workspace)?;
        check_stage_slots(binding, pair, seed)?;
        if !recipe_selected(recipe, Some(phase), self.evidence().recipes)
            || ![seed, pair.vars]
                .into_iter()
                .all(|slot| stage_supported(slot, phase))
        {
            return Err(PHASE);
        }
        super::launch::check_preparation(deadline, cancelled)?;
        self.fresh()?;
        let (media, seed_bytes, vars_bytes) = {
            // The held source handles are seekable and can be shared by
            // foreground workers. Serialize reads that move their cursors.
            let recipes = self.recipes.lock().map_err(|_| PHASE)?;
            match phase {
                InstallerPhase::UbuntuInstall => {
                    let [iso, kernel, initrd] = &self.held.network.as_ref().ok_or(PHASE)?.media;
                    let guest = self.held.ubuntu_guest.as_ref().map_err(|reason| *reason)?;
                    let (measured, bytes) = prepare_ubuntu(
                        [iso, kernel, initrd],
                        [&recipes.code, &recipes.vars, &recipes.receipt],
                        guest,
                        binding.generation,
                    )?;
                    if measured != recipe {
                        return Err(PHASE);
                    }
                    (
                        vec![
                            artifact(ArtifactRole::UbuntuInstallIso, iso)?,
                            artifact(ArtifactRole::UbuntuKernel, kernel)?,
                            artifact(ArtifactRole::UbuntuInitrd, initrd)?,
                        ],
                        bytes,
                        held_bytes(&recipes.vars, MAX_SETUP_VARS_BYTES)?,
                    )
                }
                InstallerPhase::WindowsSetup => {
                    let inputs = self
                        .held
                        .windows_recipe
                        .as_ref()
                        .map_err(|reason| *reason)?;
                    let setup = self
                        .held
                        .windows_setup_vars
                        .as_ref()
                        .map_err(|reason| *reason)?;
                    if recipes.windows.digest != recipe.recipe_sha256 {
                        return Err(PHASE);
                    }
                    (
                        vec![
                            artifact(ArtifactRole::WindowsInstallIso, &inputs.iso.pin)?,
                            artifact(ArtifactRole::WindowsDriverMedium, &inputs.driver_medium.pin)?,
                        ],
                        recipes.answer.clone(),
                        setup.read()?,
                    )
                }
                InstallerPhase::WindowsFinalize => return Err(PHASE),
            }
        };
        let seal_sha256 = self.seal_sha256();
        stage_prepared(
            reservation,
            ledger,
            seal_sha256,
            media,
            [&seed_bytes, &vars_bytes],
            cancelled,
            |target| {
                let captured = self.launch_live(TEST_JOB_MEMORY)?;
                let authority = authorize(target)?;
                if authority.target() != &target {
                    return Err(PHASE);
                }
                super::launch::check_preparation(deadline, cancelled)?;
                fresh_observation(captured, Instant::now(), NativeRefusal::ReservesUnverified)?;
                Ok(authority)
            },
        )
    }

    pub(crate) fn prepare_boot<'a>(
        &self,
        reservation: BootReservation<'a>,
    ) -> Result<QualifiedBoot<'a>, NativeRefusal> {
        let BootReservation {
            workspace,
            recipe,
            phase,
            pair,
            identities,
            backing,
            staged,
            vars_content,
        } = reservation;
        let binding = self.check_workspace(workspace)?;
        check_pair(binding, pair)?;
        if !recipe_selected(recipe, phase, self.evidence().recipes)
            || (phase.is_some() && (backing.is_some() || pair.disk.role != "disk"))
        {
            return Err(PHASE);
        }
        let seal_sha256 = self.seal_sha256();
        let (vars_role, vars_pin, media) = match (phase, staged) {
            (
                Some(phase @ (InstallerPhase::UbuntuInstall | InstallerPhase::WindowsSetup)),
                Some(staged),
            ) if vars_content.is_none() => {
                let issued = &staged.reservation;
                check_boot_binding(
                    issued.workspace.binding(),
                    binding,
                    staged.seal_sha256,
                    seal_sha256,
                )?;
                if issued.pair != pair
                    || issued.recipe != recipe
                    || issued.phase != phase
                    || staged.vars.identity != identities[1]
                {
                    return Err(PHASE);
                }
                let (vars_role, seed_role) = if phase == InstallerPhase::WindowsSetup {
                    (
                        ArtifactRole::WindowsSetupVars,
                        ArtifactRole::WindowsAnswerSeed,
                    )
                } else {
                    (ArtifactRole::Vars, ArtifactRole::UbuntuCidata)
                };
                let mut media = staged.media.clone();
                media.insert(1, artifact(seed_role, &staged.seed)?);
                (vars_role, staged.vars.verified_content, media)
            }
            (None, None) => (ArtifactRole::Vars, vars_content, Vec::new()),
            (Some(InstallerPhase::WindowsFinalize), None) if vars_content.is_none() => {
                (ArtifactRole::Vars, None, Vec::new())
            }
            _ => return Err(PHASE),
        };
        let disk = workspace.pin_slot(pair.disk, identities[0], true)?;
        let vars = workspace.pin_slot(pair.vars, identities[1], true)?;
        let mut vars_arg = artifact(vars_role, &vars)?;
        if let Some(pin) = vars_pin {
            vars_arg = vars_arg.with_pin(pin);
        }
        let disk_role = if phase.is_some() {
            ArtifactRole::TestInstallDisk
        } else {
            ArtifactRole::GuestDisk
        };
        let disk_arg = artifact(disk_role, &disk)?;
        let disk_header = header(&disk)?;
        let backing_pin = backing
            .map(|golden| {
                let path = golden.path_for(ArtifactRole::Golden).map_err(|_| PATH)?;
                let pin = PathPin::open(path, false, false)?;
                if Some(pin.identity) != golden.identity()
                    || golden.pin().is_none()
                    || !self.production_root().path.contains(path)
                    || pin.identity.volume != binding.production.volume
                    || !pin._ancestors.iter().any(|ancestor| {
                        file_identity(ancestor, true).is_ok_and(|(id, _)| id == binding.production)
                    })
                {
                    return Err(PATH);
                }
                Ok(pin)
            })
            .transpose()?;
        let backing_header = backing_pin.as_ref().map(header).transpose()?;
        let bundle = self.held.bundle.as_ref().ok_or(PHASE)?;
        let image = PathPin::open(
            &bundle
                .root
                .path
                .join("qemu-system-x86_64.exe")
                .map_err(|_| PATH)?,
            false,
            false,
        )?;
        let data = PathPin::open(
            &bundle.root.path.join("share").map_err(|_| PATH)?,
            true,
            false,
        )?;
        let recipes = self.recipes.lock().map_err(|_| PHASE)?;
        Ok(QualifiedBoot {
            binding,
            seal_sha256,
            recipe,
            pair,
            phase,
            cwd: self.command_cwd()?,
            serial: SerialName::generate().map_err(|_| PHASE)?,
            image: artifact(ArtifactRole::Qemu, &image)?,
            data: artifact(ArtifactRole::Data, &data)?,
            firmware: artifact(ArtifactRole::FirmwareCode, &recipes.code)?,
            disk: disk_arg,
            vars: vars_arg,
            disk_header,
            backing: backing.cloned(),
            backing_header,
            media,
            _reservation: reservation,
            _pair_pins: [disk, vars],
            _backing_pin: backing_pin,
        })
    }

    pub(crate) fn helper_image(&self) -> Result<Artifact, NativeRefusal> {
        let bundle = self.held.bundle.as_ref().ok_or(PHASE)?;
        let path = bundle.root.path.join("qemu-img.exe").map_err(|_| PATH)?;
        let pin = PathPin::open(&path, false, false)?;
        if pin.identity != bundle.identity("qemu-img.exe")? {
            return Err(NativeRefusal::DistributionUnverified);
        }
        artifact(ArtifactRole::QemuImg, &pin)
    }

    pub(crate) fn command_cwd(&self) -> Result<LocalPath, NativeRefusal> {
        self.held.trial.path.join("paused-cwd").map_err(|_| PATH)
    }

    /// The probe stays bound to the Windows image used to prepare the seed.
    pub(crate) fn windows_health(
        &self,
        recipe: RecipeIdentity,
        validation: bool,
    ) -> Result<String, NativeRefusal> {
        if recipe != self.evidence().recipes.windows {
            return Err(PHASE);
        }
        Ok(self
            .recipes
            .lock()
            .map_err(|_| PHASE)?
            .windows
            .image
            .probe(validation))
    }

    fn check_workspace(
        &self,
        workspace: &ProductionWorkspace,
    ) -> Result<WorkspaceBinding, NativeRefusal> {
        let binding = workspace.binding();
        if binding.production != self.production_root().identity {
            return Err(PATH);
        }
        WorkspaceCustody::production(&workspace.root, &workspace.lease)?;
        Ok(binding)
    }
}

/// Stages the recipe's already prepared media using one consumed permit for
/// each exact leaf. The fixture supplies bounded byte slices at this boundary.
pub(super) fn stage_prepared<'a>(
    reservation: InstallerReservation<'a>,
    ledger: &dyn CustodyCheckpoints,
    seal_sha256: Hash,
    media: Vec<Artifact>,
    [seed_bytes, vars_bytes]: [&[u8]; 2],
    cancelled: &dyn Fn() -> bool,
    mut authorize: impl FnMut(StageTarget) -> Result<StageAuthority, NativeRefusal>,
) -> Result<StagedInstaller<'a>, NativeRefusal> {
    let InstallerReservation {
        workspace,
        pair,
        seed,
        recipe,
        phase,
        deadline,
    } = reservation;
    let binding = workspace.binding();
    check_stage_slots(binding, pair, seed)?;
    if !stage_supported(seed, phase) {
        return Err(PHASE);
    }
    let check = || super::launch::check_preparation(deadline, cancelled);
    let leaves = [
        Leaf {
            slot: seed,
            bytes: seed_bytes,
            limit: SEED_LIMIT,
            mutable: false,
        },
        Leaf {
            slot: pair.vars,
            bytes: vars_bytes,
            limit: MAX_SETUP_VARS_BYTES,
            mutable: true,
        },
    ];
    let [seed, vars] = stage_leaves(workspace, ledger, leaves, &check, |slot, content| {
        check()?;
        let target = StageTarget {
            workspace: binding,
            seal_sha256,
            recipe,
            phase,
            slot,
            content,
            deadline,
        };
        let authority = authorize(target)?;
        if authority.target() != &target {
            return Err(PHASE);
        }
        check()?;
        Ok(authority)
    })?;
    check()?;
    Ok(StagedInstaller {
        reservation,
        seal_sha256,
        seed,
        vars,
        media,
    })
}

fn check_stage_slots(
    binding: WorkspaceBinding,
    pair: PairSlots,
    seed: ArtifactSlot,
) -> Result<(), NativeRefusal> {
    check_pair(binding, pair)?;
    if pair.disk.role != "disk"
        || seed.role != "seed"
        || seed.generation != binding.generation
        || [pair.disk.key, pair.vars.key, [0; 16]].contains(&seed.key)
    {
        return Err(PATH);
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct Leaf<'b> {
    slot: ArtifactSlot,
    bytes: &'b [u8],
    limit: u64,
    mutable: bool,
}

/// Each leaf has its own authorization and checkpoints. The immutable first
/// leaf is verified in the ledger before the second leaf's intent exists, so
/// a failure on the second leaf leaves the first accounted. The mutable
/// second leaf stays identified with its content verified in the held pin.
fn stage_leaves<A>(
    workspace: &ProductionWorkspace,
    ledger: &dyn CustodyCheckpoints,
    leaves: [Leaf<'_>; 2],
    check: &dyn Fn() -> Result<(), NativeRefusal>,
    mut authorize: impl FnMut(ArtifactSlot, ArtifactPin) -> Result<A, NativeRefusal>,
) -> Result<[PathPin; 2], NativeRefusal> {
    let [first, second] = leaves;
    let first = stage_input(workspace, ledger, first, check, |pin| {
        authorize(first.slot, pin)
    })?;
    let second = stage_input(workspace, ledger, second, check, |pin| {
        authorize(second.slot, pin)
    })?;
    Ok([first, second])
}

fn stage_input<A>(
    workspace: &ProductionWorkspace,
    ledger: &dyn CustodyCheckpoints,
    leaf: Leaf<'_>,
    check: &dyn Fn() -> Result<(), NativeRefusal>,
    authorize: impl FnOnce(ArtifactPin) -> Result<A, NativeRefusal>,
) -> Result<PathPin, NativeRefusal> {
    let Leaf {
        slot,
        bytes,
        limit,
        mutable,
    } = leaf;
    let binding = workspace.binding();
    let size = bytes.len() as u64;
    if !(1..=limit).contains(&size) || slot.generation != binding.generation {
        return Err(PATH);
    }
    let content = ArtifactPin {
        size,
        sha256: digest(bytes),
    };
    WorkspaceCustody::production(&workspace.root, &workspace.lease)?;
    let _authority = authorize(content)?;
    // Intent, create-new and identity are durable before the first byte.
    let mut writer = workspace.create_slot(slot, ledger)?;
    for chunk in bytes.chunks(256 << 10) {
        check()?;
        writer.write_all(chunk)?;
    }
    check()?;
    writer.sync()?;
    check()?;
    let mut pin = writer.into_pin(mutable)?;
    pin.verify_content(content, limit)?;
    check()?;
    // A mutable leaf keeps its verified initial content only in the held pin.
    // Its single durable digest is committed after its final writable phase.
    if !mutable {
        ledger.slot_verified(
            binding,
            slot,
            ArtifactEvidence {
                identity: pin.identity,
                directory: binding.workspace.file,
                pin: content,
            },
        )?;
    }
    Ok(pin)
}

#[cfg(test)]
mod tests;
