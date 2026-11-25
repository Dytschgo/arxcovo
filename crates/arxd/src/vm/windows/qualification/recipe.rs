//! Production plans derived from the selected TEST recipe and reserved slots.

use super::launch::{artifact, header, held_bytes, prepare_ubuntu};
use super::production::{ProductionWorkspace, WorkspaceBinding, check_pair, slot_path};
use super::*;
use crate::vm::paths::Artifact;
use crate::vm::qemu_args::{BootPurpose, BootRecipe};
use crate::vm::qualification_record::{Hash, RecipeIdentity, TestRecipes, digest};
use crate::vm::runtime::{InstallerPhase, LaunchAuthority};
use crate::vm::{ArtifactEvidence, ArtifactSlot, PairSlots};
use std::io::Write;

const SEED_LIMIT: u64 = 40 << 20;
pub(super) const TEST_JOB_MEMORY: usize = 6 * GIB as usize;
const PATH: NativeRefusal = NativeRefusal::PathCustodyUnverified;
const PHASE: NativeRefusal = NativeRefusal::InstallerPhaseMismatch;

#[derive(Clone, Copy)]
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

fn recipe_selected(
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

impl ForegroundQualification {
    pub(crate) fn stage_installer<'a>(
        &self,
        reservation: InstallerReservation<'a>,
        mut authorize: impl FnMut(StageTarget) -> Result<LaunchAuthority, NativeRefusal>,
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
        if !recipe_selected(recipe, Some(phase), self.evidence().recipes) {
            return Err(PHASE);
        }
        check_deadline(deadline)?;
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
        let mut stage = |slot, bytes: &[u8], limit, mutable| {
            stage_input(workspace, slot, bytes, limit, mutable, |content| {
                let captured = self.launch_live(TEST_JOB_MEMORY)?;
                let authority = authorize(StageTarget {
                    workspace: binding,
                    seal_sha256,
                    recipe,
                    phase,
                    slot,
                    content,
                    deadline,
                })?;
                check_deadline(deadline)?;
                fresh_observation(captured, Instant::now(), NativeRefusal::ReservesUnverified)?;
                Ok(authority)
            })
        };
        let seed = stage(seed, &seed_bytes, SEED_LIMIT, false)?;
        let vars = stage(pair.vars, &vars_bytes, MAX_SETUP_VARS_BYTES, true)?;
        check_deadline(deadline)?;
        Ok(StagedInstaller {
            reservation,
            seal_sha256,
            seed,
            vars,
            media,
        })
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
            ) => {
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
            (None | Some(InstallerPhase::WindowsFinalize), None) => {
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

    pub(crate) fn installer_checks(
        &self,
        recipe: RecipeIdentity,
        before_finalize: bool,
    ) -> Result<Vec<String>, NativeRefusal> {
        let recipes = self.evidence().recipes;
        if recipe == recipes.ubuntu && !before_finalize {
            return Ok(vec![crate::vm::template::UBUNTU_DETACHED_HEALTH.to_owned()]);
        }
        let mut checks = vec![self.windows_health(recipe, before_finalize)?];
        if before_finalize {
            checks.push(launch::WINDOWS_SIGNED.to_owned());
        }
        Ok(checks)
    }

    pub(crate) fn firmware_pin(&self) -> Result<ArtifactPin, NativeRefusal> {
        self.recipes
            .lock()
            .map_err(|_| PHASE)?
            .code
            .verified_content
            .ok_or(PHASE)
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

fn check_deadline(deadline: Instant) -> Result<(), NativeRefusal> {
    (Instant::now() < deadline)
        .then_some(())
        .ok_or(NativeRefusal::InstallerDeadlineExpired)
}

fn stage_input<A>(
    workspace: &ProductionWorkspace,
    slot: ArtifactSlot,
    bytes: &[u8],
    limit: u64,
    mutable: bool,
    authorize: impl FnOnce(ArtifactPin) -> Result<A, NativeRefusal>,
) -> Result<PathPin, NativeRefusal> {
    let size = bytes.len() as u64;
    if !(1..=limit).contains(&size) || slot.generation != workspace.binding().generation {
        return Err(PATH);
    }
    let path = slot_path(&workspace.root.path, slot)?;
    let leaf = path.as_str().rsplit('\\').next().ok_or(PATH)?;
    let content = ArtifactPin {
        size,
        sha256: digest(bytes),
    };
    WorkspaceCustody::production(&workspace.root, &workspace.lease)?;
    let _authority = authorize(content)?;
    let created =
        open_relative(&workspace.root.leaf, leaf, Node::File, true, true).map_err(|_| PATH)?;
    let id = file_identity(&created, false).map_err(|_| PATH)?.0;
    (&created)
        .write_all(bytes)
        .and_then(|()| created.sync_all())
        .map_err(|_| PATH)?;
    // Bridge the writable creation handle and the final read-only file
    // pin without releasing the exclusion against rename or replacement.
    let mut pin = workspace.pin_slot(slot, id, true)?;
    drop(created);
    if !mutable {
        pin = workspace.pin_slot(slot, id, false)?;
    }
    pin.verify_content(content, limit)?;
    Ok(pin)
}

#[cfg(test)]
mod tests;
