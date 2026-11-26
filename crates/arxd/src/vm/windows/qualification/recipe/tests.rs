use super::*;
use crate::store::vm::native::Leaf as Custody;
use crate::vm::windows::qualification::production::{
    WRITE_FAULT, WriteFault, slot_path,
    tests::{
        custody::{Fault, Fixture},
        unbacked_qcow2_header,
    },
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
    let fixture = Fixture::new();
    let slots = fixture.builds[0];
    let recorder = fixture.recorder(slots.seed.generation);
    let ws = fixture.reserve(&recorder, slots.seed.generation).unwrap();
    recorder.log.borrow_mut().clear();
    let binding = ws.binding();
    let (slot, pair) = (slots.seed, slots.pair);
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
    let leaf = |slot, bytes: &'static [u8], limit| Leaf {
        slot,
        bytes,
        limit,
        mutable: false,
    };
    let path = slot_path(&ws.root.path, slot).unwrap();
    assert!(
        stage_input::<()>(&ws, &recorder, leaf(wrong, b"seed", 10), |_| panic!(
            "wrong generation"
        ))
        .is_err()
    );
    assert!(
        stage_input::<()>(&ws, &recorder, leaf(slot, b"seed", 3), |_| panic!(
            "over limit"
        ))
        .is_err()
    );
    assert!(stage_input::<()>(&ws, &recorder, leaf(slot, b"seed", 10), |_| Err(PHASE)).is_err());
    // Authorization precedes the durable intent.
    assert!(recorder.log.borrow().is_empty());
    assert!(!std::path::Path::new(path.as_str()).exists());
    let staged = stage_input(&ws, &recorder, leaf(slot, b"seed", 10), |pin| {
        assert_eq!(pin.sha256, digest(b"seed"));
        assert!(!std::path::Path::new(path.as_str()).exists());
        Ok(())
    })
    .unwrap();
    assert_eq!(
        *recorder.log.borrow(),
        [
            ("intend_slot", None),
            ("slot_identified", Some(0)),
            ("slot_verified", Some(4)),
        ]
    );
    assert_eq!(
        fixture.leaf(slot),
        Custody::Verified(staged.identity.file, pin_of(b"seed"))
    );
    assert_eq!(std::fs::read(path.as_str()).unwrap(), b"seed");
    assert!(
        std::fs::OpenOptions::new()
            .write(true)
            .open(path.as_str())
            .is_err()
    );
    assert!(stage_input(&ws, &recorder, leaf(slot, b"new", 10), |_| Ok(())).is_err());
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

fn pin_of(bytes: &[u8]) -> ArtifactPin {
    ArtifactPin {
        size: bytes.len() as u64,
        sha256: digest(bytes),
    }
}

/// T15/T17: the seed is verified in the ledger before the VARS intent, so
/// each VARS fault leaves the seed accounted, also after a reopen.
#[test]
fn seed_stays_accounted_when_the_second_leaf_fails() {
    let mut fixture = Fixture::new();
    let cases: [(Option<Fault>, Option<WriteFault>); 5] = [
        (None, None),
        (Some(Fault::Before("intend_slot")), None),
        (Some(Fault::Before("slot_identified")), None),
        (None, Some(WriteFault::Partial)),
        (None, Some(WriteFault::Sync)),
    ];
    let mut outcomes = Vec::new();
    for ((fault, write), slots) in cases.into_iter().zip(fixture.builds.clone()) {
        let generation = slots.seed.generation;
        let recorder = fixture.recorder(generation);
        let ws = fixture.reserve(&recorder, generation).unwrap();
        recorder.log.borrow_mut().clear();
        let leaves = [
            Leaf {
                slot: slots.seed,
                bytes: b"seed",
                limit: 10,
                mutable: false,
            },
            Leaf {
                slot: slots.pair.vars,
                bytes: b"vars-initial",
                limit: MAX_SETUP_VARS_BYTES,
                mutable: true,
            },
        ];
        let staged = stage_leaves(&ws, &recorder, leaves, |slot, _| {
            if slot == slots.pair.vars {
                recorder.fault.set(fault);
                WRITE_FAULT.set(write);
            }
            Ok(())
        });
        WRITE_FAULT.set(None);
        let names = recorder.names();
        assert_eq!(staged.is_ok(), fault.is_none() && write.is_none());
        assert_eq!(
            names[..4],
            ["intend_slot", "slot_identified", "slot_verified", "intend_slot"]
        );
        let seed = fixture.leaf(slots.seed);
        assert!(
            matches!(seed, Custody::Verified(_, pin) if pin == pin_of(b"seed")),
            "{fault:?} {write:?}"
        );
        let vars = fixture.leaf(slots.pair.vars);
        let vars_path = slot_path(&ws.root.path, slots.pair.vars).unwrap();
        let bytes = std::fs::read(vars_path.as_str()).ok();
        match (fault, write) {
            (None, None) => {
                assert!(matches!(vars, Custody::Verified(..)));
                assert_eq!(bytes.as_deref(), Some(&b"vars-initial"[..]));
            }
            (Some(Fault::Before("intend_slot")), _) => {
                assert_eq!(vars, Custody::NeverAttempted);
                assert_eq!(bytes, None);
            }
            (Some(_), _) => {
                assert_eq!(vars, Custody::Unidentified);
                assert_eq!(bytes.as_deref(), Some(&b""[..]));
            }
            (None, Some(fault)) => {
                assert!(matches!(vars, Custody::Identified(_)), "{fault:?}");
                let written = if fault == WriteFault::Partial { 6 } else { 12 };
                assert_eq!(bytes.map(|bytes| bytes.len()), Some(written));
            }
        }
        outcomes.push((slots, seed, vars));
    }
    fixture.reopen();
    for (slots, seed, vars) in outcomes {
        assert_eq!(fixture.leaf(slots.seed), seed);
        assert_eq!(fixture.leaf(slots.pair.vars), vars);
    }
}
