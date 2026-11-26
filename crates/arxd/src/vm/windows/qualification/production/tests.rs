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

#[path = "custody_tests.rs"]
pub(in crate::vm::windows::qualification) mod custody;

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
        &hex::encode(generation),
        &format!("O:{owner_text}D:P(A;OICI;FA;;;{owner_text})"),
    );
    let lease = create_file_sddl(
        &workspace_path,
        GENERATION_LEASE,
        &format!("O:{owner_text}D:P(A;;FA;;;{owner_text})"),
    );
    let production = PathPin::open(&production_path, true, false).unwrap();
    let expected = fixture_binding(&production, generation, &workspace_path, &lease);
    let ws = hold_workspace_under(&production, sid_ptr(&owner_data), expected).unwrap();
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

fn fixture_binding(
    production: &PathPin,
    generation: Id,
    directory: &LocalPath,
    lease: &LocalPath,
) -> WorkspaceBinding {
    WorkspaceBinding {
        production: production.identity,
        generation,
        workspace: PathPin::open(directory, true, false).unwrap().identity,
        lease: file_identity(&File::open(lease.as_str()).unwrap(), false)
            .unwrap()
            .0,
    }
}

#[test]
fn hold_workspace_enforces_exact_names_ids_acls_and_exclusive_lease() {
    let temp = tempfile::tempdir().unwrap();
    let (production, held, owner_data, owner) = held_test_workspace(temp.path(), [0xab; 16]);
    let expected = held.binding();
    let acquire = |binding| hold_workspace_under(&production, sid_ptr(&owner_data), binding);
    assert!(acquire(expected).is_err(), "second lock must be refused");
    let directory = held.root.path.clone();
    let lease = directory.join(GENERATION_LEASE).unwrap();
    drop(held);
    assert_eq!(acquire(expected).unwrap().binding(), expected);
    let different_file = |mut id: FileIdentity| {
        id.file[0] ^= 1;
        id
    };
    for wrong in [
        WorkspaceBinding {
            generation: [0; 16],
            ..expected
        },
        WorkspaceBinding {
            generation: [0xef; 16],
            ..expected
        },
        WorkspaceBinding {
            production: identity(7),
            ..expected
        },
        WorkspaceBinding {
            workspace: different_file(expected.workspace),
            ..expected
        },
        WorkspaceBinding {
            lease: different_file(expected.lease),
            ..expected
        },
    ] {
        assert!(acquire(wrong).is_err(), "{wrong:?}");
    }

    std::fs::write(lease.as_str(), [1]).unwrap();
    assert!(acquire(expected).is_err(), "populated lease");
    std::fs::write(lease.as_str(), []).unwrap();
    let other_name = directory.join("Generation.LEASE").unwrap();
    let intermediate = directory.join("temporary").unwrap();
    std::fs::rename(lease.as_str(), intermediate.as_str()).unwrap();
    assert!(acquire(expected).is_err(), "missing lease");
    std::fs::rename(intermediate.as_str(), other_name.as_str()).unwrap();
    assert!(acquire(expected).is_err(), "case-changed lease");
    std::fs::rename(other_name.as_str(), intermediate.as_str()).unwrap();
    let new_lease = create_file_sddl(
        &directory,
        GENERATION_LEASE,
        &format!("O:{owner}D:P(A;;FA;;;{owner})"),
    );
    assert!(acquire(expected).is_err(), "substituted lease");
    let new_binding = fixture_binding(&production, expected.generation, &directory, &new_lease);
    let alias = directory.join("lease-link").unwrap();
    std::fs::hard_link(new_lease.as_str(), alias.as_str()).unwrap();
    assert!(acquire(new_binding).is_err(), "hard-linked lease");
    std::fs::remove_file(alias.as_str()).unwrap();
    assert_eq!(acquire(new_binding).unwrap().binding(), new_binding);

    // Seed the next controls before pinning their parent. Windows refuses
    // a directory rename while its parent is pinned without delete sharing.
    let production_path = production.path.clone();
    drop(production);
    let parent = Path::new(production_path.as_str());
    let uppercase_generation = [0xac; 16];
    let uppercase = create_dir_sddl(
        parent,
        &hex::encode_upper(uppercase_generation),
        &format!("O:{owner}D:P(A;OICI;FA;;;{owner})"),
    );
    let uppercase_lease = create_file_sddl(
        &uppercase,
        GENERATION_LEASE,
        &format!("O:{owner}D:P(A;;FA;;;{owner})"),
    );
    let shared_generation = [0xad; 16];
    let shared = create_dir_sddl(
        parent,
        &hex::encode(shared_generation),
        &format!("O:{owner}D:P(A;OICI;FA;;;{owner})(A;OICI;FR;;;WD)"),
    );
    let shared_lease = create_file_sddl(
        &shared,
        GENERATION_LEASE,
        &format!("O:{owner}D:P(A;;FA;;;{owner})"),
    );
    let production = PathPin::open(&production_path, true, false).unwrap();
    let acquire = |binding| hold_workspace_under(&production, sid_ptr(&owner_data), binding);
    let uppercase_binding = fixture_binding(
        &production,
        uppercase_generation,
        &uppercase,
        &uppercase_lease,
    );
    assert!(
        acquire(uppercase_binding).is_err(),
        "case-changed directory"
    );
    let shared_binding = fixture_binding(&production, shared_generation, &shared, &shared_lease);
    assert!(acquire(shared_binding).is_err(), "non-private directory");
}

#[test]
fn hold_workspace_refuses_a_non_private_lease_in_a_private_directory() {
    let temp = tempfile::tempdir().unwrap();
    let (production, held, owner_data, owner) = held_test_workspace(temp.path(), [0xcd; 16]);
    let generation = held.binding().generation;
    let directory = held.root.path.clone();
    drop(held);
    std::fs::remove_file(directory.join(GENERATION_LEASE).unwrap().as_str()).unwrap();
    let path = create_file_sddl(
        &directory,
        GENERATION_LEASE,
        &format!("O:{owner}D:P(A;;FA;;;{owner})(A;;FR;;;WD)"),
    );
    let expected = fixture_binding(&production, generation, &directory, &path);
    assert!(hold_workspace_under(&production, sid_ptr(&owner_data), expected).is_err());
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

    let pins = hold_plan_inputs(&plan, Instant::now() + Duration::from_secs(5), &|| false).unwrap();
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
        hold_plan_inputs(
            &unpinned_plan,
            Instant::now() + Duration::from_secs(5),
            &|| false
        )
        .err(),
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
        hold_plan_inputs(
            &bad_fw_plan,
            Instant::now() + Duration::from_secs(5),
            &|| false
        )
        .err(),
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
        hold_plan_inputs(
            &bad_data_plan,
            Instant::now() + Duration::from_secs(5),
            &|| false
        )
        .err(),
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

fn start_pair_at(
    workspace: &ProductionWorkspace,
    slots: PairSlots,
    bytes: &[u8],
) -> (StartPair, [LocalPath; 2]) {
    use sha2::{Digest, Sha256};
    let paths = [slots.disk, slots.vars].map(|slot| slot_path(&workspace.root.path, slot));
    let evidence = paths.each_ref().map(|path| {
        std::fs::write(path.as_str(), bytes).unwrap();
        ArtifactEvidence {
            identity: PathPin::open(path, false, false).unwrap().identity,
            directory: workspace.binding().workspace.file,
            pin: ArtifactPin {
                size: bytes.len() as u64,
                sha256: Sha256::digest(bytes).into(),
            },
        }
    });
    (StartPair { slots, evidence }, paths)
}

fn start_pair(workspace: &ProductionWorkspace) -> (StartPair, [LocalPath; 2]) {
    let slots = pair(workspace.binding().generation);
    start_pair_at(workspace, slots, b"stopped synthetic bytes")
}

#[test]
fn start_refusal_observes_the_already_locked_pair_through_the_consumer() {
    let temp = tempfile::tempdir().unwrap();
    let (production, ws, owner, _) = held_test_workspace(temp.path(), [1; 16]);
    let (active, paths) = start_pair(&ws);
    let binding = ws.binding();
    let writer = |path: &LocalPath| std::fs::OpenOptions::new().write(true).open(path.as_str());
    let acquire = |binding| hold_workspace_under(&production, sid_ptr(&owner), binding);
    let deadline = || Instant::now() + Duration::from_secs(5);

    // Validation, plan construction, and the last pre-create refresh all
    // return through the held failure seam.
    for reason in [
        NativeRefusal::PathCustodyUnverified,
        NativeRefusal::QualificationMissing,
        NativeRefusal::ReservesUnverified,
        NativeRefusal::JobUnverified,
    ] {
        let mut refused =
            StartRefusal::while_held(&ws, Some(active), || Err::<(), _>(reason.into()))
                .err()
                .unwrap();
        assert_eq!(refused.reason(), reason);
        refused.with_observation(
            deadline(),
            &|| false,
            |_| panic!("tried to reacquire the already locked lease"),
            |observed| {
                let observed = observed.expect("held observation");
                assert!(std::ptr::eq(observed.workspace(), &ws));
                assert_eq!(observed.pair(), Some(active));
                assert!(acquire(binding).is_err());
                let released = observed.release.expect("unchanged, exclusive pair");
                assert!(released.matches_start(binding, active));
                assert!(paths.iter().all(|path| writer(path).is_err()));
                drop(released);
                assert!(paths.iter().all(|path| writer(path).is_ok()));
            },
        );
    }

    let observe = |pair, cutoff, stop: &dyn Fn() -> bool| {
        let mut refused = StartRefusal::before_create(
            Some(&ws),
            binding,
            Some(pair),
            NativeRefusal::QualificationMissing,
        );
        refused.with_observation(
            cutoff,
            stop,
            |_| panic!("tried to reacquire the already locked lease"),
            |observed| {
                let held = observed.expect("retain held workspace even without clean release");
                assert!(std::ptr::eq(held.workspace, &ws));
                assert!(held.release().is_none());
            },
        );
    };
    observe(active, deadline(), &|| true);
    observe(active, Instant::now(), &|| false);
    let mut wrong_directory = active;
    wrong_directory.evidence[0].directory[0] ^= 1;
    observe(wrong_directory, deadline(), &|| false);

    // A later caller deadline cannot extend the cutoff fixed at construction.
    let mut expired = StartRefusal::before_create(
        Some(&ws),
        binding,
        Some(active),
        NativeRefusal::QualificationMissing,
    );
    expired.scope.cutoff = Instant::now();
    expired.with_observation(
        deadline(),
        &|| false,
        |_| panic!("tried to reacquire the already locked lease"),
        |observed| assert!(observed.expect("held").release().is_none()),
    );

    let open_writer = writer(&paths[1]).unwrap();
    observe(active, deadline(), &|| false);
    drop(open_writer);
    // Preserve the file identity and length while changing its digest.
    std::fs::write(
        paths[0].as_str(),
        vec![255; active.evidence[0].pin.size as usize],
    )
    .unwrap();
    observe(active, deadline(), &|| false);
    drop(ws);
    assert!(acquire(binding).is_ok());
}

#[test]
fn start_refusal_proof_is_bound_to_the_requested_pair_and_its_digests() {
    let temp = tempfile::tempdir().unwrap();
    let (_production, ws, _, _) = held_test_workspace(temp.path(), [4; 16]);
    let binding = ws.binding();
    let (first, _) = start_pair(&ws);
    let mut slots = pair(binding.generation);
    slots.disk.key = [3; 16];
    slots.vars.key = [4; 16];
    let (second, _) = start_pair_at(&ws, slots, b"second synthetic pair under the lease");
    assert_ne!(first.evidence[0].identity, second.evidence[0].identity);
    let mut changed = first;
    changed.evidence[1].pin.sha256[0] ^= 1;
    let mixed = StartPair {
        slots: first.slots,
        evidence: second.evidence,
    };
    let deadline = || Instant::now() + Duration::from_secs(5);
    let observe = |binding, pair, consume: &dyn Fn(Option<HeldStartRefusal<'_>>)| {
        let mut refused = StartRefusal::before_create(
            Some(&ws),
            binding,
            Some(pair),
            NativeRefusal::QualificationMissing,
        );
        refused.with_observation(
            deadline(),
            &|| false,
            |_| panic!("tried to reacquire the already locked lease"),
            consume,
        );
    };

    // Each real pair under the same lease yields proof for only that pair
    // and its exact digests. The identity-only live matcher refuses it.
    for (requested, other) in [(first, second), (second, first)] {
        observe(binding, requested, &|observed| {
            let held = observed.expect("held observation");
            assert_eq!(held.pair(), Some(requested));
            let released = held.release().expect("unchanged requested pair");
            assert!(released.matches_start(binding, requested));
            for refused in [other, changed, mixed] {
                assert!(!released.matches_start(binding, refused));
            }
            let ids = requested.evidence.map(|file| file.identity);
            assert!(!released.matches(binding, requested.slots, ids));
        });
    }
    // Changed digests for the same identities, or another pair's files under
    // these slots, never produce clean proof.
    for refused in [changed, mixed] {
        observe(binding, refused, &|observed| {
            assert!(observed.expect("held").release().is_none());
        });
    }
    // A refusal cannot be rebound to another workspace scope.
    let mut foreign = binding;
    foreign.lease.file[0] ^= 1;
    observe(foreign, first, &|observed| assert!(observed.is_none()));

    // The plan's pair files must be the files named by the bound evidence.
    let ids = |pair: StartPair| pair.evidence.map(|file| file.identity);
    assert!(check_start_evidence(None, None).is_ok());
    assert!(check_start_evidence(Some(first), Some((first.slots, ids(first)))).is_ok());
    for (start, bound) in [
        (Some(first), Some((first.slots, ids(second)))),
        (Some(first), Some((second.slots, ids(first)))),
        (Some(changed), Some((first.slots, ids(second)))),
        (Some(first), None),
        (None, Some((first.slots, ids(first)))),
    ] {
        assert!(check_start_evidence(start, bound).is_err());
    }
}

#[test]
fn start_refusal_before_acquisition_spends_its_single_attempt() {
    use std::cell::RefCell;
    let temp = tempfile::tempdir().unwrap();
    let (production, ws, owner, _) = held_test_workspace(temp.path(), [2; 16]);
    let (active, paths) = start_pair(&ws);
    let binding = ws.binding();
    let attempts = RefCell::new(Vec::new());
    let acquire = |requested| {
        attempts.borrow_mut().push(requested);
        hold_workspace_under(&production, sid_ptr(&owner), requested)
    };
    let deadline = || Instant::now() + Duration::from_secs(5);
    let refuse = || {
        StartRefusal::before_create(
            None,
            binding,
            Some(active),
            NativeRefusal::PathCustodyUnverified,
        )
    };

    // A failed attempt spends the token even after the holder goes away.
    let mut contended = refuse();
    contended.with_observation(deadline(), &|| false, acquire, |obs| {
        assert!(obs.is_none(), "a foreign holder must remain exclusive");
    });
    drop(ws);
    contended.with_observation(deadline(), &|| false, acquire, |obs| {
        assert!(obs.is_none(), "a spent refusal reopened its acquisition");
    });
    assert_eq!(*attempts.borrow(), [binding]);

    // Cancellation or the expired original cutoff spend it without opening.
    let mut cancelled = refuse();
    cancelled.with_observation(deadline(), &|| true, acquire, |obs| assert!(obs.is_none()));
    let mut expired = refuse();
    expired.scope.cutoff = Instant::now();
    expired.with_observation(deadline(), &|| false, acquire, |obs| assert!(obs.is_none()));
    for refused in [&mut cancelled, &mut expired] {
        refused.with_observation(deadline(), &|| false, acquire, |obs| assert!(obs.is_none()));
    }
    assert_eq!(*attempts.borrow(), [binding]);

    // One successful later attempt opens only the bound scope and keeps that
    // lease for the refusal's lifetime without reopening it.
    let mut later = refuse();
    later.with_observation(deadline(), &|| false, acquire, |obs| {
        let held = obs.expect("one later acquisition");
        assert_eq!(held.workspace().binding(), binding);
        let released = held.release.unwrap();
        assert!(released.matches_start(binding, active));
        assert!(hold_workspace_under(&production, sid_ptr(&owner), binding).is_err());
        for path in &paths {
            assert!(
                std::fs::OpenOptions::new()
                    .write(true)
                    .open(path.as_str())
                    .is_err()
            );
        }
    });
    later.with_observation(
        deadline(),
        &|| false,
        |_| panic!("reopened the lease it already holds"),
        |obs| assert!(obs.expect("still held").release().is_some()),
    );
    assert_eq!(*attempts.borrow(), [binding, binding]);
    assert!(hold_workspace_under(&production, sid_ptr(&owner), binding).is_err());
    drop(later);
    assert!(hold_workspace_under(&production, sid_ptr(&owner), binding).is_ok());
}

fn refused_child() -> Child {
    match crate::vm::windows::tests::fixture_launch(0) {
        Some((child, _stdio)) => child,
        // Runners that deny nested Jobs still exercise the refusal with an
        // empty Job and a synthetic signaled exit handle.
        None => Child {
            job: Job::new(256 * 1024 * 1024, None).unwrap(),
            process: unsafe { owned(CreateEventW(null(), 1, 1, null())) }.unwrap(),
            suspended: None,
        },
    }
}

#[tokio::test]
async fn start_refusal_after_create_never_proves_an_unchanged_start() {
    let temp = tempfile::tempdir().unwrap();
    let (production, ws, owner, _) = held_test_workspace(temp.path(), [3; 16]);
    let (active, _) = start_pair(&ws);
    let binding = ws.binding();
    let assert_fenced = |refused: &mut StartRefusal<'_>| {
        refused.with_observation(
            Instant::now() + Duration::from_secs(5),
            &|| false,
            |_| panic!("post-create refusal tried to acquire the lease again"),
            |obs| {
                let held = obs.expect("exact workspace observation");
                assert!(
                    held.release.is_none(),
                    "post-create refusal cannot mint clean proof"
                );
                assert_eq!(held.workspace().binding(), binding);
                assert!(hold_workspace_under(&production, sid_ptr(&owner), binding).is_err());
            },
        );
    };

    let spawn_failed = |stage| {
        HoldRefusal::from_spawn((
            at(stage)(io::Error::other("injected create edge refusal")),
            Job::new(256 * 1024 * 1024, None).unwrap(),
        ))
    };
    let pre_create = StartRefusal::while_held(&ws, Some(active), || {
        Err::<(), _>(spawn_failed(SpawnStage::Create))
    })
    .err()
    .unwrap();
    assert_eq!(pre_create.creation(), StartCreation::NeverCreated);
    let mut unknown = StartRefusal::while_held(&ws, Some(active), || {
        Err::<(), _>(spawn_failed(SpawnStage::Verify))
    })
    .err()
    .unwrap();
    assert_fenced(&mut unknown);
    assert_eq!(unknown.creation(), StartCreation::Unconfirmed);
    assert!(
        unknown
            .settle_created(tokio::time::Instant::now())
            .await
            .is_err()
    );

    for fail_in_attach in [false, true] {
        let child = refused_child();
        let (release, blocker) = std::sync::mpsc::channel();
        let mut refused = StartRefusal::while_held(&ws, Some(active), || {
            attach_checked_child(
                child,
                |_| {
                    if fail_in_attach {
                        Ok(())
                    } else {
                        Err(NativeRefusal::JobUnverified)
                    }
                },
                |workers| {
                    assert!(fail_in_attach);
                    workers.workers.push(
                        Worker::start("refused-start-fixture", move || {
                            blocker.recv().map_err(io::Error::other)
                        })
                        .unwrap(),
                    );
                    Err(NativeRefusal::QmpUnverified)
                },
            )
        })
        .err()
        .unwrap();
        assert_fenced(&mut refused);
        assert_eq!(refused.creation(), StartCreation::Created);
        if fail_in_attach {
            assert!(
                refused
                    .settle_created(tokio::time::Instant::now())
                    .await
                    .is_err()
            );
            let LaunchCreation::Created(child) = &refused.failure.creation else {
                panic!("lost created-child custody");
            };
            assert_eq!(child.workers.workers.len(), 1);
        }
        let _ = release.send(());
        refused
            .settle_created(tokio::time::Instant::now() + Duration::from_secs(5))
            .await
            .unwrap();
        assert!(!refused.worker_failed());
        assert_fenced(&mut refused);
    }
}

#[tokio::test]
async fn refused_attach_settles_after_one_worker_fails_and_another_exits() {
    let temp = tempfile::tempdir().unwrap();
    let (_production, ws, _, _) = held_test_workspace(temp.path(), [5; 16]);
    let (active, _) = start_pair(&ws);
    let (finish, finished) = std::sync::mpsc::channel::<()>();
    let mut refused = StartRefusal::while_held(&ws, Some(active), || {
        attach_checked_child(
            refused_child(),
            |_| Ok(()),
            |workers| {
                // The failing worker is joined first.
                workers.workers.push(
                    Worker::start("refused-start-error", || {
                        Err(io::Error::other("injected stdio worker failure"))
                    })
                    .unwrap(),
                );
                workers.workers.push(
                    Worker::start("refused-start-exit", move || {
                        let _ = finished.recv();
                        Ok(())
                    })
                    .unwrap(),
                );
                Err(NativeRefusal::QmpUnverified)
            },
        )
    })
    .err()
    .unwrap();
    assert_eq!(refused.creation(), StartCreation::Created);
    let workers = |refused: &StartRefusal<'_>| match &refused.failure.creation {
        LaunchCreation::Created(child) => child.workers.workers.len(),
        _ => panic!("lost created-child custody"),
    };

    // An unfinished worker keeps both handles, including the failed one.
    assert!(
        refused
            .settle_created(tokio::time::Instant::now())
            .await
            .is_err()
    );
    assert_eq!(workers(&refused), 2);
    assert!(!refused.worker_failed());

    // Once both end, settlement drains every handle and confirms, while the
    // worker failure stays separately visible. A repeat settlement agrees.
    finish.send(()).unwrap();
    let deadline = || tokio::time::Instant::now() + Duration::from_secs(5);
    refused.settle_created(deadline()).await.unwrap();
    assert_eq!(workers(&refused), 0);
    assert!(refused.worker_failed());
    refused.settle_created(deadline()).await.unwrap();
    assert!(refused.worker_failed());
    refused.with_observation(
        Instant::now() + Duration::from_secs(5),
        &|| false,
        |_| panic!("post-create refusal tried to acquire the lease again"),
        |obs| assert!(obs.expect("held").release().is_none()),
    );
}

#[test]
fn bounded_hash_checks_stop_between_chunks_and_before_accepting_eof() {
    use std::cell::Cell;
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("image");
    let bytes = vec![42; 3 * (256 << 10)];
    std::fs::write(&file, &bytes).unwrap();
    let file = File::open(file).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    assert!(launch::hash_held(&file, bytes.len() as u64, deadline, &|| false).is_ok());
    for stop_at in [3, 5, 6] {
        let checks = Cell::new(0);
        let stop = || {
            checks.set(checks.get() + 1);
            checks.get() >= stop_at
        };
        assert!(launch::hash_held(&file, bytes.len() as u64, deadline, &stop).is_err());
        assert_eq!(checks.get(), stop_at);
    }
}

#[test]
fn production_start_admission_stays_closed_with_an_enabled_profile() {
    use crate::vm::runtime::{CandidateRuntime, NativeMutation};
    let profile = crate::config::VmRuntimeProfile::parse(
        br#"{
        "enabled":true,"owner_sid":"S-1-5-21-1-2-3-1001","supervisor_sid":"S-1-5-21-1-2-3-1002",
        "bundle":"E:/pinned bundle","volume":"D:/","state":"D:/test_vms/state",
        "disk_ceiling_bytes":10737418240,"manifest":"qemu-11.1.3-windows-x64"
    }"#,
    )
    .unwrap();
    assert_eq!(
        CandidateRuntime::new(Some(&profile)).dispatch::<()>(NativeMutation::Boot, |_| {
            panic!("production start reached its launch effect")
        }),
        Err(NativeRefusal::QualificationMissing)
    );
}
