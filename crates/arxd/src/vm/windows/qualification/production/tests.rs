use super::*;
use crate::vm::{
    paths::{Artifact, ArtifactPin, ArtifactRole, FileIdentity, LocalPath},
    qemu_args::{
        BootPurpose, BootRecipe, SerialName, StaticNetwork, golden_info, qualifier_create,
    },
    runtime::InstallerPhase,
    static_web::OriginPortLease,
};
use std::os::windows::ffi::OsStrExt;
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
};

fn identity(id: u8) -> FileIdentity {
    FileIdentity {
        volume: 1,
        file: [id; 16],
    }
}

fn workspace() -> WorkspaceBinding {
    WorkspaceBinding {
        generation: [1; 16],
        production: identity(1),
        workspace: identity(2),
        lease: identity(3),
    }
}

fn pair(generation: Id) -> PairSlots {
    PairSlots {
        disk: ArtifactSlot {
            generation,
            role: "disk",
            key: [1; 16],
        },
        vars: ArtifactSlot {
            generation,
            role: "vars",
            key: [2; 16],
        },
    }
}

fn current_user_sid() -> (Vec<usize>, String) {
    let token = process_token(unsafe { GetCurrentProcess() }, TOKEN_QUERY).unwrap();
    let data = token_data(&token, TokenUser).unwrap();
    let sid = unsafe { (*data.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    let mut text = null_mut();
    assert_ne!(unsafe { ConvertSidToStringSidW(sid, &mut text) }, 0);
    let len = (0..).take_while(|&i| unsafe { *text.add(i) } != 0).count();
    let string = String::from_utf16(unsafe { std::slice::from_raw_parts(text, len) }).unwrap();
    unsafe { LocalFree(text.cast()) };
    (data, string)
}

fn sid_ptr(data: &[usize]) -> PSID {
    unsafe { (*data.as_ptr().cast::<TOKEN_USER>()).User.Sid }
}

fn create_dir_sddl(parent: &Path, name: &str, sddl: &str) -> LocalPath {
    let path = parent.join(name);
    let mut descriptor = null_mut();
    let wide_sddl: Vec<u16> = OsStr::new(sddl).encode_wide().chain(Some(0)).collect();
    assert_ne!(
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                wide_sddl.as_ptr(),
                1,
                &mut descriptor,
                null_mut(),
            )
        },
        0
    );
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    let ok = unsafe { CreateDirectoryW(wide(&path).as_ptr(), &attributes) };
    unsafe { LocalFree(descriptor) };
    assert_ne!(ok, 0, "{}", io::Error::last_os_error());
    LocalPath::parse(path.to_str().unwrap()).unwrap()
}

fn create_file_sddl(parent: &LocalPath, name: &str, sddl: &str) -> LocalPath {
    let path = parent.join(name).unwrap();
    let mut descriptor = null_mut();
    let wide_sddl: Vec<u16> = OsStr::new(sddl).encode_wide().chain(Some(0)).collect();
    assert_ne!(
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                wide_sddl.as_ptr(),
                1,
                &mut descriptor,
                null_mut(),
            )
        },
        0
    );
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    let handle = unsafe {
        CreateFileW(
            wide(path.as_str()).as_ptr(),
            FILE_GENERIC_WRITE,
            FILE_SHARE_READ,
            &attributes,
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL,
            null_mut(),
        )
    };
    unsafe { LocalFree(descriptor) };
    let file = File::from(unsafe { owned(handle) }.unwrap());
    drop(file);
    path
}

fn private_dir(parent: &Path, name: &str) -> (Vec<usize>, String, LocalPath) {
    let (data, text) = current_user_sid();
    let path = create_dir_sddl(parent, name, &format!("O:{text}D:P(A;OICI;FA;;;{text})"));
    (data, text, path)
}

pub(in crate::vm::windows::qualification) fn unbacked_qcow2_header(virtual_size: u64) -> Vec<u8> {
    let mut cluster = vec![0u8; 65_536];
    cluster[..4].copy_from_slice(b"QFI\xfb");
    cluster[4..8].copy_from_slice(&3u32.to_be_bytes());
    cluster[20..24].copy_from_slice(&16u32.to_be_bytes());
    cluster[24..32].copy_from_slice(&virtual_size.to_be_bytes());
    cluster[96..100].copy_from_slice(&4u32.to_be_bytes());
    cluster[100..104].copy_from_slice(&104u32.to_be_bytes());
    cluster
}

fn pin_for(bytes: &[u8]) -> ArtifactPin {
    ArtifactPin {
        size: bytes.len() as u64,
        sha256: crate::vm::qualification_record::digest(bytes),
    }
}

fn slot_path(root: &LocalPath, slot: ArtifactSlot) -> LocalPath {
    root.join(&format!("{}-{}", slot.role, hex::encode(slot.key)))
        .unwrap()
}

fn synthetic_artifact(role: ArtifactRole, path: LocalPath, id: u8) -> Artifact {
    Artifact::new(role, path)
        .unwrap()
        .with_identity(identity(id))
        .with_pin(ArtifactPin {
            size: 64,
            sha256: [id; 32],
        })
}

pub(in crate::vm::windows::qualification) fn held_test_workspace(
    temp: &Path,
    generation: Id,
) -> (PathPin, ProductionWorkspace, Vec<usize>, String) {
    let (owner_data, owner_text, production_path) = private_dir(temp, "production");
    let workspace_path = create_dir_sddl(
        Path::new(production_path.as_str()),
        "ws",
        &format!("O:{owner_text}D:P(A;OICI;FA;;;{owner_text})"),
    );
    create_file_sddl(
        &workspace_path,
        "slot.lease",
        &format!("O:{owner_text}D:P(A;;FA;;;{owner_text})"),
    );
    let production = PathPin::open(&production_path, true, false).unwrap();
    let ws = hold_workspace_under(
        &production,
        sid_ptr(&owner_data),
        &workspace_path,
        "slot.lease",
        generation,
    )
    .unwrap();
    (production, ws, owner_data, owner_text)
}

#[tokio::test]
async fn helper_release_retains_writer_exclusion_and_rejects_dropped_custody() {
    let temp = tempfile::tempdir_in(std::env::temp_dir()).unwrap();
    let (_production, ws, _, _) = held_test_workspace(temp.path(), [1; 16]);
    let slot = pair(ws.binding().generation).disk;
    let path = slot_path(&ws.root.path, slot);
    std::fs::write(path.as_str(), b"helper result").unwrap();
    let pin = PathPin::open(&path, false, true).unwrap();
    let observed = || StopEvidence {
        process_exited: true,
        job_empty: true,
        workers_joined: true,
        guest_shutdown: true,
        clean_process_exit: true,
        ..StopEvidence::default()
    };
    let custody = WorkspaceCustody::production(&ws.root, &ws.lease).unwrap();
    let inputs = [(ArtifactRole::Staging, pin.identity)];
    let targets = ReleaseTargets::Helper {
        kind: PlanKind::Mutate,
        inputs: inputs.to_vec(),
    };
    let pins: std::sync::Arc<[PathPin]> = vec![pin].into();
    assert!(
        ReleasedGeneration::checked(
            ws.binding(),
            targets.clone(),
            custody,
            (pins.clone(), vec![]),
        )
        .is_none()
    );
    let other_path = ws.root.path.join("unrelated").unwrap();
    std::fs::write(other_path.as_str(), b"another object").unwrap();
    let other = PathPin::open(&other_path, false, false).unwrap();
    assert!(
        ReleasedGeneration::checked(
            ws.binding(),
            targets.clone(),
            custody,
            (pins.clone(), vec![other.leaf]),
        )
        .is_none()
    );

    let writers = || std::fs::OpenOptions::new().write(true).open(path.as_str());
    let open_writer = writers().unwrap();
    let take_custody = || {
        let pins = pins.clone();
        move || Ok((pins.clone(), vec![pins[0].reopen_without_writer()?]))
    };
    let deadline = || tokio::time::Instant::now() + Duration::from_secs(5);
    let (disposition, held) =
        finish_release_held(observed(), deadline(), take_custody(), || true).await;
    assert_eq!(disposition, StopDisposition::ChargedQuarantine);
    assert!(held.is_none());
    drop(open_writer);
    let (disposition, held) =
        finish_release_held(observed(), deadline(), take_custody(), || true).await;
    assert_eq!(disposition, StopDisposition::CleanRelease);
    let proof = ReleasedGeneration::checked(ws.binding(), targets, custody, held.unwrap()).unwrap();
    assert!(proof.matches_helper(ws.binding(), PlanKind::Mutate, &inputs));
    let mut different = ws.binding();
    different.generation = [2; 16];
    assert!(!proof.matches_helper(different, PlanKind::Mutate, &inputs));
    assert!(!proof.matches_helper(ws.binding(), PlanKind::InspectWritable, &inputs));
    assert!(writers().is_err());
    drop(proof);
    assert!(writers().is_ok());
}

#[test]
fn consumed_permit_requires_the_exact_seal_generation_workspace_and_kind() {
    let workspace = workspace();
    let permit = LaunchPermitBinding {
        workspace,
        seal_sha256: [7; 32],
        kind: PlanKind::Boot,
    };
    assert_eq!(check_permit(permit, permit), Ok(()));
    for changed in [
        WorkspaceBinding {
            generation: [4; 16],
            ..workspace
        },
        WorkspaceBinding {
            workspace: identity(4),
            ..workspace
        },
        WorkspaceBinding {
            production: identity(4),
            ..workspace
        },
        WorkspaceBinding {
            lease: identity(4),
            ..workspace
        },
    ] {
        assert_eq!(
            check_permit(
                permit,
                LaunchPermitBinding {
                    workspace: changed,
                    ..permit
                }
            ),
            Err(NativeRefusal::QualificationMissing)
        );
    }
    assert_eq!(
        check_permit(
            permit,
            LaunchPermitBinding {
                seal_sha256: [8; 32],
                ..permit
            }
        ),
        Err(NativeRefusal::QualificationMissing)
    );
    for other_kind in [
        PlanKind::Inspect,
        PlanKind::InspectWritable,
        PlanKind::Mutate,
        PlanKind::InstallerBoot(InstallerPhase::UbuntuInstall),
        PlanKind::InstallerBoot(InstallerPhase::WindowsSetup),
        PlanKind::InstallerBoot(InstallerPhase::WindowsFinalize),
        PlanKind::StaticComposeInstaller,
        PlanKind::StaticComposeDetached,
    ] {
        assert_eq!(
            check_permit(
                permit,
                LaunchPermitBinding {
                    kind: other_kind,
                    ..permit
                }
            ),
            Err(NativeRefusal::QualificationMissing)
        );
    }
    let setup = LaunchPermitBinding {
        kind: PlanKind::InstallerBoot(InstallerPhase::WindowsSetup),
        ..permit
    };
    let finalize = LaunchPermitBinding {
        kind: PlanKind::InstallerBoot(InstallerPhase::WindowsFinalize),
        ..permit
    };
    let ubuntu = LaunchPermitBinding {
        kind: PlanKind::InstallerBoot(InstallerPhase::UbuntuInstall),
        ..permit
    };
    assert_eq!(check_permit(setup, setup), Ok(()));
    assert_eq!(
        check_permit(setup, finalize),
        Err(NativeRefusal::QualificationMissing)
    );
    assert_eq!(
        check_permit(setup, ubuntu),
        Err(NativeRefusal::QualificationMissing)
    );
    assert_eq!(
        check_permit(finalize, ubuntu),
        Err(NativeRefusal::QualificationMissing)
    );
}

#[test]
fn launch_permit_fence_requires_uninhabited_authority_and_non_echoed_target() {
    struct RefusingPermit;

    impl LaunchPermit for RefusingPermit {
        fn consume(
            self,
            _target: LaunchTarget<'_, '_>,
        ) -> Result<(LaunchPermitBinding, crate::vm::runtime::LaunchAuthority), NativeRefusal>
        {
            Err(NativeRefusal::QualificationMissing)
        }
    }

    // Type-level fence assertions: these fail to compile if LaunchPermit::consume
    // drops LaunchAuthority or if NativeLaunch drops its `_authority` field.
    fn assert_consume_returns_authority<P: LaunchPermit>(
        permit: P,
        target: LaunchTarget<'_, '_>,
    ) -> Result<(LaunchPermitBinding, crate::vm::runtime::LaunchAuthority), NativeRefusal> {
        permit.consume(target)
    }
    fn assert_launch_retains_authority(
        launch: NativeLaunch<'_>,
    ) -> crate::vm::runtime::LaunchAuthority {
        launch._authority
    }
    let _ = assert_launch_retains_authority;

    // While LaunchAuthority is uninhabited, Option<LaunchAuthority> cannot
    // hold a Some value; NativeLaunch's `_authority` field is checked above.
    assert_eq!(size_of::<Option<crate::vm::runtime::LaunchAuthority>>(), 0);

    // Explicitly construct LaunchTarget without `..` so reintroducing an
    // echoed `binding` field fails compilation.
    let temp = tempfile::tempdir_in(std::env::temp_dir()).unwrap();
    let root = local(temp.path().to_str().unwrap()).unwrap();
    let image = synthetic_artifact(
        ArtifactRole::QemuImg,
        root.join("qemu-img.exe").unwrap(),
        10,
    );
    let golden = synthetic_artifact(ArtifactRole::Golden, root.join("golden.qcow2").unwrap(), 11);
    let header = unbacked_qcow2_header(1 << 30);
    let plan = golden_info(&image, &golden, &header).unwrap();
    let target = LaunchTarget {
        plan: &plan,
        cwd: &root,
        pair: None,
        installer: None,
        recipe: None,
        memory: 256 << 20,
        cpu_rate: 1000,
    };
    assert_eq!(
        assert_consume_returns_authority(RefusingPermit, target).err(),
        Some(NativeRefusal::QualificationMissing)
    );
}

#[test]
fn hold_workspace_enforces_containment_generation_acls_and_exclusive_lease() {
    let temp = tempfile::tempdir_in(std::env::temp_dir()).unwrap();
    let (owner_data, owner_text, production_path) = private_dir(temp.path(), "production");
    let owner = sid_ptr(&owner_data);
    let private_sddl = format!("O:{owner_text}D:P(A;OICI;FA;;;{owner_text})");
    let shared_sddl = format!("O:{owner_text}D:P(A;OICI;FA;;;{owner_text})(A;OICI;FR;;;WD)");
    let production = PathPin::open(&production_path, true, false).unwrap();

    // Valid private workspace + private lease succeeds and binds all identities.
    let valid_ws = create_dir_sddl(Path::new(production_path.as_str()), "ws", &private_sddl);
    let valid_lease = create_file_sddl(
        &valid_ws,
        "slot.lease",
        &format!("O:{owner_text}D:P(A;;FA;;;{owner_text})"),
    );
    let generation = [0x22; 16];
    let held =
        hold_workspace_under(&production, owner, &valid_ws, "slot.lease", generation).unwrap();
    assert_eq!(held.binding().generation, generation);
    assert_eq!(held.binding().production, production.identity);
    assert_eq!(
        held.binding().workspace,
        PathPin::open(&valid_ws, true, false).unwrap().identity
    );
    assert_eq!(
        held.binding().lease,
        file_identity(&File::open(valid_lease.as_str()).unwrap(), false)
            .unwrap()
            .0
    );

    // Second concurrent lock on the same lease file is refused.
    assert_eq!(
        hold_workspace_under(&production, owner, &valid_ws, "slot.lease", generation).err(),
        Some(NativeRefusal::PathCustodyUnverified)
    );
    drop(held);
    // Dropping the workspace releases the lease lock so a fresh hold succeeds.
    let reheld =
        hold_workspace_under(&production, owner, &valid_ws, "slot.lease", generation).unwrap();
    drop(reheld);

    // Equal root (production itself) is refused.
    create_file_sddl(
        &production_path,
        "root.lease",
        &format!("O:{owner_text}D:P(A;;FA;;;{owner_text})"),
    );
    assert_eq!(
        hold_workspace_under(
            &production,
            owner,
            &production_path,
            "root.lease",
            generation
        )
        .err(),
        Some(NativeRefusal::PathCustodyUnverified)
    );

    // Outside root (sibling directory) is refused.
    let outside = create_dir_sddl(temp.path(), "outside", &private_sddl);
    create_file_sddl(
        &outside,
        "slot.lease",
        &format!("O:{owner_text}D:P(A;;FA;;;{owner_text})"),
    );
    assert_eq!(
        hold_workspace_under(&production, owner, &outside, "slot.lease", generation).err(),
        Some(NativeRefusal::PathCustodyUnverified)
    );

    // Zero generation is refused.
    assert_eq!(
        hold_workspace_under(&production, owner, &valid_ws, "slot.lease", [0; 16]).err(),
        Some(NativeRefusal::PathCustodyUnverified)
    );

    // Unsafe lease name is refused.
    for bad_lease in ["", "..", r"sub\lease", "CON"] {
        assert_eq!(
            hold_workspace_under(&production, owner, &valid_ws, bad_lease, generation).err(),
            Some(NativeRefusal::PathCustodyUnverified),
            "{bad_lease}"
        );
    }

    // Non-descendant identity (textual path matches, but held production identity
    // does not appear among opened ancestor handles) is refused.
    let mut forged_production = PathPin::open(&production_path, true, false).unwrap();
    forged_production.identity.file[0] ^= 0xff;
    assert_eq!(
        hold_workspace_under(
            &forged_production,
            owner,
            &valid_ws,
            "slot.lease",
            generation
        )
        .err(),
        Some(NativeRefusal::PathCustodyUnverified)
    );

    // Non-private workspace ACL is refused.
    let shared_ws = create_dir_sddl(
        Path::new(production_path.as_str()),
        "shared-ws",
        &shared_sddl,
    );
    create_file_sddl(
        &shared_ws,
        "slot.lease",
        &format!("O:{owner_text}D:P(A;;FA;;;{owner_text})"),
    );
    assert_eq!(
        hold_workspace_under(&production, owner, &shared_ws, "slot.lease", generation).err(),
        Some(NativeRefusal::PathCustodyUnverified)
    );

    // Private workspace with a non-private lease ACL is refused.
    let mixed_ws = create_dir_sddl(
        Path::new(production_path.as_str()),
        "mixed-ws",
        &private_sddl,
    );
    create_file_sddl(
        &mixed_ws,
        "shared.lease",
        &format!("O:{owner_text}D:P(A;;FA;;;{owner_text})(A;;FR;;;WD)"),
    );
    assert_eq!(
        hold_workspace_under(&production, owner, &mixed_ws, "shared.lease", generation).err(),
        Some(NativeRefusal::PathCustodyUnverified)
    );
}

#[test]
fn both_reserved_slots_must_match_the_workspace_generation() {
    let workspace = workspace();
    assert_eq!(check_pair(workspace, pair(workspace.generation)), Ok(()));
    let mut overlay = pair(workspace.generation);
    overlay.disk.role = "verification_overlay";
    assert_eq!(check_pair(workspace, overlay), Ok(()));
    assert_eq!(
        check_pair(workspace, pair([4; 16])),
        Err(NativeRefusal::PathCustodyUnverified)
    );
    let mut mixed = pair(workspace.generation);
    mixed.vars.generation = [4; 16];
    assert_eq!(
        check_pair(workspace, mixed),
        Err(NativeRefusal::PathCustodyUnverified)
    );
    let mut bad_disk_role = pair(workspace.generation);
    bad_disk_role.disk.role = "seed";
    assert_eq!(
        check_pair(workspace, bad_disk_role),
        Err(NativeRefusal::PathCustodyUnverified)
    );
    let mut bad_vars_role = pair(workspace.generation);
    bad_vars_role.vars.role = "disk";
    assert_eq!(
        check_pair(workspace, bad_vars_role),
        Err(NativeRefusal::PathCustodyUnverified)
    );
    let mut same_key = pair(workspace.generation);
    same_key.vars.key = same_key.disk.key;
    assert_eq!(
        check_pair(workspace, same_key),
        Err(NativeRefusal::PathCustodyUnverified)
    );
}

#[test]
fn pair_ids_maps_boot_and_installer_roles_and_rejects_mismatches() {
    let temp = tempfile::tempdir_in(std::env::temp_dir()).unwrap();
    let root = local(temp.path().to_str().unwrap()).unwrap();
    let header = unbacked_qcow2_header(1 << 30);
    let serial = SerialName::generate().unwrap();
    let slots = pair([1; 16]);
    let qemu = synthetic_artifact(ArtifactRole::Qemu, root.join("qemu.exe").unwrap(), 10);
    let data = synthetic_artifact(ArtifactRole::Data, root.join("share").unwrap(), 11);
    let firmware = synthetic_artifact(
        ArtifactRole::FirmwareCode,
        root.join("OVMF_CODE.fd").unwrap(),
        12,
    );

    // 1. Installed Boot -> GuestDisk + Vars.
    let boot_disk = synthetic_artifact(ArtifactRole::GuestDisk, slot_path(&root, slots.disk), 21);
    let boot_vars = synthetic_artifact(ArtifactRole::Vars, slot_path(&root, slots.vars), 22);
    let boot_recipe = BootRecipe {
        image: &qemu,
        data: &data,
        firmware: &firmware,
        vars: &boot_vars,
        disk: &boot_disk,
        disk_header: &header,
        backing: None,
        backing_header: None,
        serial: &serial,
        boot: BootPurpose::Installed,
        ram_mib: 1024,
        vcpus: 2,
    };
    let boot_plan = boot_recipe.plan().unwrap();
    assert_eq!(
        pair_ids(&boot_plan, &root, slots),
        Ok([identity(21), identity(22)])
    );

    // Wrong slot path (different key) is refused.
    let mut wrong_disk = slots;
    wrong_disk.disk.key = [9; 16];
    assert_eq!(
        pair_ids(&boot_plan, &root, wrong_disk),
        Err(NativeRefusal::PathCustodyUnverified)
    );
    let mut wrong_vars = slots;
    wrong_vars.vars.key = [9; 16];
    assert_eq!(
        pair_ids(&boot_plan, &root, wrong_vars),
        Err(NativeRefusal::PathCustodyUnverified)
    );
    // Wrong workspace root is refused.
    let other_root = root.join("other").unwrap();
    assert_eq!(
        pair_ids(&boot_plan, &other_root, slots),
        Err(NativeRefusal::PathCustodyUnverified)
    );

    // Missing artifact identity is refused.
    let unpinned_disk =
        Artifact::new(ArtifactRole::GuestDisk, slot_path(&root, slots.disk)).unwrap();
    let unpinned_recipe = BootRecipe {
        image: &qemu,
        data: &data,
        firmware: &firmware,
        vars: &boot_vars,
        disk: &unpinned_disk,
        disk_header: &header,
        backing: None,
        backing_header: None,
        serial: &serial,
        boot: BootPurpose::Installed,
        ram_mib: 1024,
        vcpus: 2,
    };
    let unpinned_plan = unpinned_recipe.plan().unwrap();
    assert_eq!(
        pair_ids(&unpinned_plan, &root, slots),
        Err(NativeRefusal::PathCustodyUnverified)
    );

    // 2. WindowsSetup -> TestInstallDisk + WindowsSetupVars.
    let install_disk = synthetic_artifact(
        ArtifactRole::TestInstallDisk,
        slot_path(&root, slots.disk),
        31,
    );
    let setup_vars = synthetic_artifact(
        ArtifactRole::WindowsSetupVars,
        slot_path(&root, slots.vars),
        32,
    );
    let win_iso = synthetic_artifact(
        ArtifactRole::WindowsInstallIso,
        root.join("win.iso").unwrap(),
        33,
    );
    let win_answer = synthetic_artifact(
        ArtifactRole::WindowsAnswerSeed,
        root.join("answer.iso").unwrap(),
        34,
    );
    let win_drivers = synthetic_artifact(
        ArtifactRole::WindowsDriverMedium,
        root.join("drivers.iso").unwrap(),
        35,
    );
    let setup_recipe = BootRecipe {
        image: &qemu,
        data: &data,
        firmware: &firmware,
        vars: &setup_vars,
        disk: &install_disk,
        disk_header: &header,
        backing: None,
        backing_header: None,
        serial: &serial,
        boot: BootPurpose::WindowsSetup {
            iso: &win_iso,
            answer: &win_answer,
            drivers: &win_drivers,
        },
        ram_mib: 2048,
        vcpus: 2,
    };
    let setup_plan = setup_recipe.plan().unwrap();
    assert_eq!(
        pair_ids(&setup_plan, &root, slots),
        Ok([identity(31), identity(32)])
    );

    // 3. UbuntuInstall and WindowsFinalize -> TestInstallDisk + Vars.
    let install_vars = synthetic_artifact(ArtifactRole::Vars, slot_path(&root, slots.vars), 42);
    let ub_iso = synthetic_artifact(
        ArtifactRole::UbuntuInstallIso,
        root.join("ubuntu.iso").unwrap(),
        43,
    );
    let ub_cidata = synthetic_artifact(
        ArtifactRole::UbuntuCidata,
        root.join("cidata.iso").unwrap(),
        44,
    );
    let ub_kernel = synthetic_artifact(
        ArtifactRole::UbuntuKernel,
        root.join("vmlinuz").unwrap(),
        45,
    );
    let ub_initrd =
        synthetic_artifact(ArtifactRole::UbuntuInitrd, root.join("initrd").unwrap(), 46);
    let ubuntu_recipe = BootRecipe {
        image: &qemu,
        data: &data,
        firmware: &firmware,
        vars: &install_vars,
        disk: &install_disk,
        disk_header: &header,
        backing: None,
        backing_header: None,
        serial: &serial,
        boot: BootPurpose::UbuntuInstall {
            iso: &ub_iso,
            cidata: &ub_cidata,
            kernel: &ub_kernel,
            initrd: &ub_initrd,
        },
        ram_mib: 2048,
        vcpus: 2,
    };
    let ubuntu_plan = ubuntu_recipe.plan().unwrap();
    assert_eq!(
        pair_ids(&ubuntu_plan, &root, slots),
        Ok([identity(31), identity(42)])
    );

    let finalize_recipe = BootRecipe {
        image: &qemu,
        data: &data,
        firmware: &firmware,
        vars: &install_vars,
        disk: &install_disk,
        disk_header: &header,
        backing: None,
        backing_header: None,
        serial: &serial,
        boot: BootPurpose::WindowsFinalize,
        ram_mib: 2048,
        vcpus: 2,
    };
    let finalize_plan = finalize_recipe.plan().unwrap();
    assert_eq!(
        pair_ids(&finalize_plan, &root, slots),
        Ok([identity(31), identity(42)])
    );

    // Non-boot plans (e.g. Inspect) are refused by pair_ids.
    let qimg = synthetic_artifact(
        ArtifactRole::QemuImg,
        root.join("qemu-img.exe").unwrap(),
        50,
    );
    let golden = synthetic_artifact(ArtifactRole::Golden, root.join("golden.qcow2").unwrap(), 51);
    let inspect_plan = golden_info(&qimg, &golden, &header).unwrap();
    assert_eq!(
        pair_ids(&inspect_plan, &root, slots),
        Err(NativeRefusal::PathCustodyUnverified)
    );
}

#[test]
fn hold_plan_inputs_pins_files_and_enforces_read_only_content_digests() {
    let temp = tempfile::tempdir_in(std::env::temp_dir()).unwrap();
    let root = local(temp.path().to_str().unwrap()).unwrap();
    let header = unbacked_qcow2_header(1 << 30);
    let serial = SerialName::generate().unwrap();

    let qemu_path = root.join("qemu.exe").unwrap();
    let data_path = root.join("share").unwrap();
    let fw_path = root.join("OVMF_CODE.fd").unwrap();
    let vars_path = root.join("vars.fd").unwrap();
    let disk_path = root.join("disk.qcow2").unwrap();
    std::fs::write(qemu_path.as_str(), b"qemu-bin").unwrap();
    std::fs::create_dir(data_path.as_str()).unwrap();
    let fw_bytes = b"synthetic-ovmf-code";
    std::fs::write(fw_path.as_str(), fw_bytes).unwrap();
    std::fs::write(vars_path.as_str(), b"vars").unwrap();
    std::fs::write(disk_path.as_str(), &header).unwrap();

    let qemu = Artifact::new(ArtifactRole::Qemu, qemu_path).unwrap();
    let data = Artifact::new(ArtifactRole::Data, data_path).unwrap();
    let fw_pin = pin_for(fw_bytes);
    let firmware = Artifact::new(ArtifactRole::FirmwareCode, fw_path.clone())
        .unwrap()
        .with_pin(fw_pin);
    let vars = Artifact::new(ArtifactRole::Vars, vars_path).unwrap();
    let disk = Artifact::new(ArtifactRole::GuestDisk, disk_path).unwrap();

    let plan_recipe = BootRecipe {
        image: &qemu,
        data: &data,
        firmware: &firmware,
        vars: &vars,
        disk: &disk,
        disk_header: &header,
        backing: None,
        backing_header: None,
        serial: &serial,
        boot: BootPurpose::Installed,
        ram_mib: 1024,
        vcpus: 2,
    };
    let plan = plan_recipe.plan().unwrap();

    let pins = hold_plan_inputs(&plan).unwrap();
    assert_eq!(pins.len(), 5);
    assert_eq!(pins[0].verified_content, None); // Qemu binary verified via bundle identity
    assert_eq!(pins[1].verified_content, None); // Data directory
    assert_eq!(pins[2].verified_content, Some(fw_pin)); // FirmwareCode content-pinned
    assert!(pins[3].mutable); // Vars writable
    assert!(pins[4].mutable); // GuestDisk writable

    // Missing content pin on read-only FirmwareCode is refused.
    let unpinned_fw = Artifact::new(ArtifactRole::FirmwareCode, fw_path.clone()).unwrap();
    let unpinned_recipe = BootRecipe {
        image: &qemu,
        data: &data,
        firmware: &unpinned_fw,
        vars: &vars,
        disk: &disk,
        disk_header: &header,
        backing: None,
        backing_header: None,
        serial: &serial,
        boot: BootPurpose::Installed,
        ram_mib: 1024,
        vcpus: 2,
    };
    let unpinned_plan = unpinned_recipe.plan().unwrap();
    assert_eq!(
        hold_plan_inputs(&unpinned_plan).err(),
        Some(NativeRefusal::PathCustodyUnverified)
    );

    // Mismatched content digest on read-only FirmwareCode is refused.
    let bad_fw = Artifact::new(ArtifactRole::FirmwareCode, fw_path)
        .unwrap()
        .with_pin(pin_for(b"wrong-ovmf-code!!!!"));
    let bad_fw_recipe = BootRecipe {
        image: &qemu,
        data: &data,
        firmware: &bad_fw,
        vars: &vars,
        disk: &disk,
        disk_header: &header,
        backing: None,
        backing_header: None,
        serial: &serial,
        boot: BootPurpose::Installed,
        ram_mib: 1024,
        vcpus: 2,
    };
    let bad_fw_plan = bad_fw_recipe.plan().unwrap();
    assert_eq!(
        hold_plan_inputs(&bad_fw_plan).err(),
        Some(NativeRefusal::PathCustodyUnverified)
    );

    // Data role pointing to a regular file instead of a directory is refused.
    let file_as_data_path = root.join("not-a-dir").unwrap();
    std::fs::write(file_as_data_path.as_str(), b"x").unwrap();
    let file_as_data = Artifact::new(ArtifactRole::Data, file_as_data_path).unwrap();
    let bad_data_recipe = BootRecipe {
        image: &qemu,
        data: &file_as_data,
        firmware: &firmware,
        vars: &vars,
        disk: &disk,
        disk_header: &header,
        backing: None,
        backing_header: None,
        serial: &serial,
        boot: BootPurpose::Installed,
        ram_mib: 1024,
        vcpus: 2,
    };
    let bad_data_plan = bad_data_recipe.plan().unwrap();
    assert_eq!(
        hold_plan_inputs(&bad_data_plan).err(),
        Some(NativeRefusal::PathCustodyUnverified)
    );
}

#[test]
fn native_launch_pre_spawn_validation_refuses_invalid_bounds_roots_and_plan_kinds() {
    let temp = tempfile::tempdir_in(std::env::temp_dir()).unwrap();
    let generation = [0x33; 16];
    let (production, ws, _, _) = held_test_workspace(temp.path(), generation);
    let ws_root = ws.root.path.clone();
    let header = unbacked_qcow2_header(1 << 30);
    let serial = SerialName::generate().unwrap();
    let slots = pair(generation);

    let qemu = synthetic_artifact(ArtifactRole::Qemu, ws_root.join("qemu.exe").unwrap(), 10);
    let qimg = synthetic_artifact(
        ArtifactRole::QemuImg,
        ws_root.join("qemu-img.exe").unwrap(),
        11,
    );
    let data = synthetic_artifact(ArtifactRole::Data, ws_root.join("share").unwrap(), 12);
    let firmware = synthetic_artifact(
        ArtifactRole::FirmwareCode,
        ws_root.join("OVMF_CODE.fd").unwrap(),
        13,
    );
    let boot_disk =
        synthetic_artifact(ArtifactRole::GuestDisk, slot_path(&ws_root, slots.disk), 21);
    let boot_vars = synthetic_artifact(ArtifactRole::Vars, slot_path(&ws_root, slots.vars), 22);
    let golden = synthetic_artifact(
        ArtifactRole::Golden,
        ws_root.join("golden.qcow2").unwrap(),
        23,
    );

    let boot_recipe = BootRecipe {
        image: &qemu,
        data: &data,
        firmware: &firmware,
        vars: &boot_vars,
        disk: &boot_disk,
        disk_header: &header,
        backing: None,
        backing_header: None,
        serial: &serial,
        boot: BootPurpose::Installed,
        ram_mib: 1024,
        vcpus: 2,
    };

    // Positive control: valid boot & valid inspect pass pre-spawn validation.
    let valid_boot = boot_recipe.plan().unwrap();
    let validated = validate_launch_request(
        production.identity,
        &ws,
        &valid_boot,
        Some(slots),
        6 * GIB as usize,
        JOB_CPU_RATE,
    )
    .unwrap();
    assert_eq!(validated.kind, PlanKind::Boot);
    assert_eq!(
        validated.bound_pair,
        Some((slots, [identity(21), identity(22)]))
    );
    assert_eq!(validated.finalization_slots, None);

    let inspect_plan = golden_info(&qimg, &golden, &header).unwrap();
    let validated_inspect =
        validate_launch_request(production.identity, &ws, &inspect_plan, None, 1, 1).unwrap();
    assert_eq!(validated_inspect.kind, PlanKind::Inspect);
    assert_eq!(validated_inspect.bound_pair, None);

    // WindowsFinalize carries finalization_slots.
    let install_disk = synthetic_artifact(
        ArtifactRole::TestInstallDisk,
        slot_path(&ws_root, slots.disk),
        31,
    );
    let finalize_recipe = BootRecipe {
        image: &qemu,
        data: &data,
        firmware: &firmware,
        vars: &boot_vars,
        disk: &install_disk,
        disk_header: &header,
        backing: None,
        backing_header: None,
        serial: &serial,
        boot: BootPurpose::WindowsFinalize,
        ram_mib: 2048,
        vcpus: 2,
    };
    let finalize_plan = finalize_recipe.plan().unwrap();
    let validated_finalize = validate_launch_request(
        production.identity,
        &ws,
        &finalize_plan,
        Some(slots),
        2 * GIB as usize,
        JOB_CPU_RATE,
    )
    .unwrap();
    assert_eq!(
        validated_finalize.kind,
        PlanKind::InstallerBoot(InstallerPhase::WindowsFinalize)
    );
    assert_eq!(validated_finalize.finalization_slots, Some(slots));

    // Wrong production root identity is refused.
    let mut wrong_prod = production.identity;
    wrong_prod.file[0] ^= 1;
    assert_eq!(
        validate_launch_request(
            wrong_prod,
            &ws,
            &valid_boot,
            Some(slots),
            2 * GIB as usize,
            JOB_CPU_RATE
        )
        .err(),
        Some(NativeRefusal::QualificationMissing)
    );

    // Memory bounds (0 and > 6 GiB) are refused.
    for bad_memory in [0, 6 * GIB as usize + 1] {
        assert_eq!(
            validate_launch_request(
                production.identity,
                &ws,
                &valid_boot,
                Some(slots),
                bad_memory,
                JOB_CPU_RATE
            )
            .err(),
            Some(NativeRefusal::QualificationMissing),
            "memory={bad_memory}"
        );
    }

    // CPU rate bounds (0 and > JOB_CPU_RATE) are refused.
    for bad_cpu in [0, JOB_CPU_RATE + 1] {
        assert_eq!(
            validate_launch_request(
                production.identity,
                &ws,
                &valid_boot,
                Some(slots),
                2 * GIB as usize,
                bad_cpu
            )
            .err(),
            Some(NativeRefusal::QualificationMissing),
            "cpu_rate={bad_cpu}"
        );
    }

    // StaticWebBoot is refused.
    let port_lease = OriginPortLease::acquire().unwrap();
    let static_disk = synthetic_artifact(
        ArtifactRole::StaticDisk,
        slot_path(&ws_root, slots.disk),
        41,
    );
    let static_vars = synthetic_artifact(
        ArtifactRole::StaticVars,
        slot_path(&ws_root, slots.vars),
        42,
    );
    let static_recipe = BootRecipe {
        image: &qemu,
        data: &data,
        firmware: &firmware,
        vars: &static_vars,
        disk: &static_disk,
        disk_header: &header,
        backing: None,
        backing_header: None,
        serial: &serial,
        boot: BootPurpose::InstalledStaticWeb(&port_lease, StaticNetwork::RestrictedOrigin),
        ram_mib: 1024,
        vcpus: 2,
    };
    let static_plan = static_recipe.plan().unwrap();
    assert_eq!(static_plan.kind(), PlanKind::StaticWebBoot);
    for pair_arg in [None, Some(slots)] {
        assert_eq!(
            validate_launch_request(
                production.identity,
                &ws,
                &static_plan,
                pair_arg,
                2 * GIB as usize,
                JOB_CPU_RATE
            )
            .err(),
            Some(NativeRefusal::QualificationMissing)
        );
    }

    // PublicEgressBoot is refused.
    let egress_qemu = synthetic_artifact(
        ArtifactRole::PublicEgressQemu,
        ws_root.join("qemu-egress.exe").unwrap(),
        43,
    );
    let egress_policy = synthetic_artifact(
        ArtifactRole::PublicEgressPolicy,
        ws_root.join("egress-policy.json").unwrap(),
        44,
    );
    let public_egress_recipe = BootRecipe {
        image: &egress_qemu,
        data: &data,
        firmware: &firmware,
        vars: &static_vars,
        disk: &static_disk,
        disk_header: &header,
        backing: None,
        backing_header: None,
        serial: &serial,
        boot: BootPurpose::InstalledStaticWeb(
            &port_lease,
            StaticNetwork::PublicEgress(&egress_policy),
        ),
        ram_mib: 1024,
        vcpus: 2,
    };
    let public_egress_plan = public_egress_recipe.plan().unwrap();
    assert_eq!(public_egress_plan.kind(), PlanKind::PublicEgressBoot);
    for pair_arg in [None, Some(slots)] {
        assert_eq!(
            validate_launch_request(
                production.identity,
                &ws,
                &public_egress_plan,
                pair_arg,
                2 * GIB as usize,
                JOB_CPU_RATE
            )
            .err(),
            Some(NativeRefusal::QualificationMissing)
        );
    }

    // QualifierCreate is refused.
    let staging = synthetic_artifact(
        ArtifactRole::Staging,
        ws_root.join("staging.qcow2").unwrap(),
        45,
    );
    let qualifier_plan = qualifier_create(&qimg, &staging, None, 1 << 30, 2 * (1 << 30)).unwrap();
    assert_eq!(qualifier_plan.kind(), PlanKind::QualifierCreate);
    for pair_arg in [None, Some(slots)] {
        assert_eq!(
            validate_launch_request(
                production.identity,
                &ws,
                &qualifier_plan,
                pair_arg,
                2 * GIB as usize,
                JOB_CPU_RATE
            )
            .err(),
            Some(NativeRefusal::QualificationMissing)
        );
    }

    // Boot and InstallerBoot without pair are refused; Inspect with pair is refused.
    assert_eq!(
        validate_launch_request(
            production.identity,
            &ws,
            &valid_boot,
            None,
            2 * GIB as usize,
            JOB_CPU_RATE
        )
        .err(),
        Some(NativeRefusal::QualificationMissing)
    );
    assert_eq!(
        validate_launch_request(
            production.identity,
            &ws,
            &finalize_plan,
            None,
            2 * GIB as usize,
            JOB_CPU_RATE
        )
        .err(),
        Some(NativeRefusal::QualificationMissing)
    );
    assert_eq!(
        validate_launch_request(
            production.identity,
            &ws,
            &inspect_plan,
            Some(slots),
            2 * GIB as usize,
            JOB_CPU_RATE
        )
        .err(),
        Some(NativeRefusal::QualificationMissing)
    );
}

#[test]
fn plan_gate_enforces_pair_and_offline_lease_matrix_for_every_plan_kind() {
    let all_kinds = [
        PlanKind::Boot,
        PlanKind::StaticWebBoot,
        PlanKind::PublicEgressBoot,
        PlanKind::StaticComposeInstaller,
        PlanKind::StaticComposeDetached,
        PlanKind::InstallerBoot(InstallerPhase::UbuntuInstall),
        PlanKind::InstallerBoot(InstallerPhase::WindowsSetup),
        PlanKind::InstallerBoot(InstallerPhase::WindowsFinalize),
        PlanKind::Inspect,
        PlanKind::InspectWritable,
        PlanKind::QualifierCreate,
        PlanKind::Mutate,
    ];
    for kind in all_kinds {
        for has_pair in [false, true] {
            for has_offline_lease in [false, true] {
                let expected = matches!(
                    (kind, has_pair, has_offline_lease),
                    (PlanKind::Boot | PlanKind::InstallerBoot(_), true, false)
                        | (PlanKind::Inspect, false, false)
                        | (PlanKind::InspectWritable | PlanKind::Mutate, false, true)
                );
                assert_eq!(
                    check_plan_gate(kind, has_pair, has_offline_lease).is_ok(),
                    expected,
                    "kind={kind:?}, has_pair={has_pair}, has_offline_lease={has_offline_lease}"
                );
            }
        }
    }
}

#[test]
fn launch_live_reserve_check_includes_whpx_overhead() {
    let memory = 6 * GIB as usize;
    let exact = RAM_RESERVE + memory as u64 + WHPX_RAM_OVERHEAD;
    assert_eq!(exact, RAM_RESERVE + 7 * GIB);
    assert!(launch_reserves_sufficient(2, exact, memory));
    assert!(!launch_reserves_sufficient(2, exact - 1, memory));
    assert!(!launch_reserves_sufficient(
        2,
        RAM_RESERVE + memory as u64,
        memory
    ));
    assert!(!launch_reserves_sufficient(1, exact, memory));
    assert!(!launch_reserves_sufficient(8, u64::MAX, usize::MAX));
}

#[tokio::test]
async fn released_proof_binds_generation_and_keeps_the_writer_exclusion() {
    let temp = tempfile::tempdir().unwrap();
    let root = local(temp.path().to_str().unwrap()).unwrap();
    let workspace = workspace();
    let slots = pair(workspace.generation);
    let paths = [slots.disk, slots.vars].map(|slot| {
        root.join(&format!("{}-{}", slot.role, hex::encode(slot.key)))
            .unwrap()
    });
    for path in &paths {
        std::fs::write(path.as_str(), b"synthetic").unwrap();
    }
    let pins = paths
        .each_ref()
        .map(|path| PathPin::open(path, false, true).unwrap());
    let ids = pins.each_ref().map(|pin| pin.identity);
    let root = PathPin::open(&root, true, false).unwrap();
    let lease = std::fs::File::open(paths[0].as_str()).unwrap();
    let workspace_custody = WorkspaceCustody::production(&root, &lease).unwrap();
    let (disposition, held) = finish_release_held(
        StopEvidence {
            process_exited: true,
            job_empty: true,
            workers_joined: true,
            ..StopEvidence::default()
        },
        tokio::time::Instant::now() + Duration::from_secs(2),
        move || {
            let guards = pins
                .iter()
                .map(PathPin::reopen_without_writer)
                .collect::<io::Result<Vec<_>>>()?;
            Ok((std::sync::Arc::from(pins), guards))
        },
        || true,
    )
    .await;
    assert_eq!(disposition, StopDisposition::DirtyRelease);
    let proof = ReleasedGeneration {
        binding: workspace,
        targets: ReleaseTargets::Pair { pair: slots, ids },
        _workspace: workspace_custody,
        _held: held.unwrap(),
    };
    assert!(proof.matches(workspace, slots, ids));
    assert!(!proof.matches(
        WorkspaceBinding {
            generation: [4; 16],
            ..workspace
        },
        slots,
        ids
    ));
    assert!(!proof.matches(workspace, pair([4; 16]), ids));
    assert!(!proof.matches(workspace, slots, [ids[1], ids[0]]));
    for path in &paths {
        assert!(
            std::fs::OpenOptions::new()
                .write(true)
                .open(path.as_str())
                .is_err()
        );
    }
    drop(proof);
    for path in &paths {
        assert!(
            std::fs::OpenOptions::new()
                .write(true)
                .open(path.as_str())
                .is_ok()
        );
    }
}
