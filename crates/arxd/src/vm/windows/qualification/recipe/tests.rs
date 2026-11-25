use super::*;
use crate::vm::windows::qualification::production::tests::{
    held_test_workspace, unbacked_qcow2_header,
};

fn asset(role: ArtifactRole, name: &str, value: u8) -> Artifact {
    Artifact::new(
        role,
        LocalPath::parse(&format!(r"D:\fixture\{name}")).unwrap(),
    )
    .unwrap()
    .with_identity(FileIdentity {
        volume: 1,
        file: [value; 16],
    })
    .with_pin(ArtifactPin {
        size: 64,
        sha256: [value; 32],
    })
}

fn ubuntu_boot(media: &[(ArtifactRole, Artifact); 4]) -> BootPurpose<'_> {
    let [(_, iso), (_, cidata), (_, kernel), (_, initrd)] = media;
    BootPurpose::UbuntuInstall {
        iso,
        cidata,
        kernel,
        initrd,
    }
}

fn windows_boot(media: &[(ArtifactRole, Artifact); 3]) -> BootPurpose<'_> {
    let [(_, iso), (_, answer), (_, drivers)] = media;
    BootPurpose::WindowsSetup {
        iso,
        answer,
        drivers,
    }
}

#[test]
fn boot_plan_refuses_self_consistently_repinned_recipe_inputs() {
    use ArtifactRole::*;
    let image = asset(Qemu, "qemu.exe", 1);
    let data = asset(Data, "share", 2);
    let code = asset(FirmwareCode, "code.fd", 3);
    let stock_vars = asset(Vars, "vars.fd", 4);
    let setup_vars = asset(WindowsSetupVars, "setup.fd", 5);
    let disk = asset(TestInstallDisk, "target.qcow2", 6);
    let ubuntu = [
        (UbuntuInstallIso, asset(UbuntuInstallIso, "ubuntu.iso", 7)),
        (UbuntuCidata, asset(UbuntuCidata, "cidata.iso", 8)),
        (UbuntuKernel, asset(UbuntuKernel, "vmlinuz", 9)),
        (UbuntuInitrd, asset(UbuntuInitrd, "initrd", 10)),
    ];
    let windows = [
        (
            WindowsInstallIso,
            asset(WindowsInstallIso, "windows.iso", 11),
        ),
        (
            WindowsAnswerSeed,
            asset(WindowsAnswerSeed, "answer.iso", 12),
        ),
        (
            WindowsDriverMedium,
            asset(WindowsDriverMedium, "drivers.iso", 13),
        ),
    ];
    let header = unbacked_qcow2_header(1 << 30);
    let serial = SerialName::generate().unwrap();
    let base = BootRecipe {
        image: &image,
        data: &data,
        firmware: &code,
        vars: &stock_vars,
        disk: &disk,
        disk_header: &header,
        backing: None,
        backing_header: None,
        serial: &serial,
        boot: BootPurpose::WindowsFinalize,
        ram_mib: 4096,
        vcpus: 2,
    };
    let selected_ubuntu = BootRecipe {
        boot: ubuntu_boot(&ubuntu),
        ..base
    };
    let selected_windows = BootRecipe {
        boot: windows_boot(&windows),
        vars: &setup_vars,
        ..base
    };
    let expected_ubuntu = selected_ubuntu.plan().unwrap();
    let expected_windows = selected_windows.plan().unwrap();
    assert_eq!(
        check_boot_plan(&expected_ubuntu, &selected_ubuntu.plan().unwrap()),
        Ok(())
    );
    assert_eq!(
        check_boot_plan(&expected_windows, &selected_windows.plan().unwrap()),
        Ok(())
    );
    let foreign = ArtifactPin {
        size: 64,
        sha256: [42; 32],
    };
    for i in 0..ubuntu.len() {
        let mut substituted = ubuntu.clone();
        substituted[i].1 = substituted[i].1.clone().with_pin(foreign);
        let forged = BootRecipe {
            boot: ubuntu_boot(&substituted),
            ..base
        };
        let plan = forged.plan().unwrap();
        assert_eq!(plan.args(), expected_ubuntu.args());
        assert_eq!(
            check_boot_plan(&expected_ubuntu, &plan),
            Err(PHASE),
            "{:?}",
            ubuntu[i].0
        );
    }
    for i in 0..windows.len() {
        let mut substituted = windows.clone();
        substituted[i].1 = substituted[i].1.clone().with_pin(foreign);
        let forged = BootRecipe {
            boot: windows_boot(&substituted),
            ..selected_windows
        };
        let plan = forged.plan().unwrap();
        assert_eq!(plan.args(), expected_windows.args());
        assert_eq!(
            check_boot_plan(&expected_windows, &plan),
            Err(PHASE),
            "{:?}",
            windows[i].0
        );
    }
    let forged_vars = setup_vars.clone().with_pin(foreign);
    let forged = BootRecipe {
        vars: &forged_vars,
        ..selected_windows
    };
    let plan = forged.plan().unwrap();
    assert_eq!(plan.args(), expected_windows.args());
    assert_eq!(check_boot_plan(&expected_windows, &plan), Err(PHASE));

    let replaced_seed = windows[1].1.clone().with_identity(FileIdentity {
        volume: 1,
        file: [99; 16],
    });
    let mut substituted = windows.clone();
    substituted[1].1 = replaced_seed;
    let forged = BootRecipe {
        boot: windows_boot(&substituted),
        ..selected_windows
    };
    assert_eq!(
        check_boot_plan(&expected_windows, &forged.plan().unwrap()),
        Err(PHASE)
    );
}

#[test]
fn stage_requires_exact_reservation_and_preserves_the_staged_identity() {
    let temp = tempfile::tempdir_in(std::env::temp_dir()).unwrap();
    let (_production, ws, _, _) = held_test_workspace(temp.path(), [1; 16]);
    let binding = ws.binding();
    let slot = ArtifactSlot {
        role: "seed",
        key: [3; 16],
        generation: binding.generation,
    };
    let pair = PairSlots {
        disk: ArtifactSlot {
            role: "disk",
            key: [1; 16],
            ..slot
        },
        vars: ArtifactSlot {
            role: "vars",
            key: [2; 16],
            ..slot
        },
    };
    assert_eq!(check_stage_slots(binding, pair, slot), Ok(()));
    assert_eq!(
        check_stage_slots(
            binding,
            pair,
            ArtifactSlot {
                role: "static_disk",
                ..slot
            }
        ),
        Err(PATH)
    );
    assert_eq!(
        check_stage_slots(
            binding,
            pair,
            ArtifactSlot {
                key: pair.vars.key,
                ..slot
            }
        ),
        Err(PATH)
    );
    let wrong = ArtifactSlot {
        generation: [2; 16],
        ..slot
    };
    assert_eq!(check_stage_slots(binding, pair, wrong), Err(PATH));
    let path = slot_path(&ws.root.path, slot).unwrap();
    let bound = |slots: &[ArtifactSlot]| SlotAuthority::for_test(ws.binding(), slots);
    assert!(
        stage_input(&ws, wrong, b"seed", 10, false, |_| panic!(
            "wrong generation"
        ))
        .is_err()
    );
    assert!(stage_input(&ws, slot, b"seed", 3, false, |_| panic!("over limit")).is_err());
    assert!(stage_input(&ws, slot, b"seed", 10, false, |_| Err(PHASE)).is_err());
    // A permit for another slot, or for a different workspace, fails at the
    // effect edge before the object is created.
    assert!(stage_input(&ws, slot, b"seed", 10, false, |_| Ok(bound(&[pair.vars]))).is_err());
    assert!(
        stage_input(&ws, slot, b"seed", 10, false, |_| Ok(
            SlotAuthority::for_test(
                WorkspaceBinding {
                    generation: [2; 16],
                    ..ws.binding()
                },
                &[slot]
            )
        ))
        .is_err()
    );
    assert!(!std::path::Path::new(path.as_str()).exists());
    let staged = stage_input(&ws, slot, b"seed", 10, false, |pin| {
        assert_eq!(pin.sha256, digest(b"seed"));
        assert!(!std::path::Path::new(path.as_str()).exists());
        Ok(bound(&[slot]))
    })
    .unwrap();
    assert_eq!(std::fs::read(path.as_str()).unwrap(), b"seed");
    assert!(
        std::fs::OpenOptions::new()
            .write(true)
            .open(path.as_str())
            .is_err()
    );
    assert!(stage_input(&ws, slot, b"new", 10, false, |_| Ok(bound(&[slot]))).is_err());
    assert_eq!(
        ws.pin_slot(slot, staged.identity, false).unwrap().identity,
        staged.identity
    );

    let seal = [9; 32];
    assert_eq!(check_boot_binding(binding, binding, seal, seal), Ok(()));
    for changed in [
        WorkspaceBinding {
            generation: [2; 16],
            ..binding
        },
        WorkspaceBinding {
            workspace: binding.production,
            ..binding
        },
        WorkspaceBinding {
            lease: binding.workspace,
            ..binding
        },
    ] {
        assert_eq!(check_boot_binding(binding, changed, seal, seal), Err(PATH));
    }
    assert_eq!(
        check_boot_binding(binding, binding, seal, [8; 32]),
        Err(PATH)
    );
}
