use super::*;
use crate::vm::{
    BuildAdmission, Capacity, ClockReading, DiskClaim, HostObservation, RequestId, Status,
    VmPolicy,
    static_web::{StaticName, StaticState},
};
use std::{cell::Cell, fs};

const NOW: i64 = 1_800_000_000_000;
const PRINCIPAL: &str = "alice";
const CLAIM: DiskClaim = DiskClaim {
    images: 100,
    staging: 10,
    metadata: 0,
};
const HOST: HostObservation = HostObservation {
    owner_idle: Some(true),
    logical_cpus: Some(8),
    available_ram: Some(64 << 30),
    capacity: Some(Capacity {
        free_bytes: 250 << 30,
        managed_bytes: 0,
    }),
};

struct TestClock;

impl Clock for TestClock {
    fn now(&self) -> ClockReading {
        ClockReading {
            utc_ms: NOW,
            monotonic: Instant::now(),
        }
    }
}

fn key(nonce: u8) -> RequestId {
    RequestId {
        key: [nonce; 16],
        issued_at_ms: NOW,
    }
}

fn id(byte: u8) -> FileIdentity {
    FileIdentity {
        volume: 7,
        file: [byte; 16],
    }
}

fn evidence(file: u8, directory: u8, content: u8) -> ArtifactEvidence {
    ArtifactEvidence {
        identity: id(file),
        directory: [directory; 16],
        pin: ArtifactPin {
            size: 4,
            sha256: [content; 32],
        },
    }
}

fn admit_copy(
    controller: &VmController<TestClock>,
) -> anyhow::Result<(Id, PairSlots, PendingOperation, Id, [ArtifactEvidence; 2])> {
    let recipe = hex::encode(crate::vm::WEB_RECIPE_ID);
    let digest = [21; 32];
    let spec = BuildRequest {
        principal: PRINCIPAL,
        key: key(1),
        recipe: &recipe,
        digest,
        retained: DiskClaim {
            staging: 0,
            ..CLAIM
        },
        transient: CLAIM,
    };
    let BuildAdmission::Fresh(build, worker, _) = controller
        .admit_build(spec, HOST, TEST_RESOURCES, |_, _| None)?
        .expect("template admission")
    else {
        panic!("fresh template required");
    };
    let [disk, vars] = [evidence(10, 2, 4), evidence(11, 2, 5)];
    let leaves = [
        (build.pair.disk, disk),
        (build.pair.vars, vars),
        (build.seed, evidence(12, 2, 6)),
        (build.verification.disk, evidence(13, 3, 7)),
        (build.verification.vars, evidence(14, 3, 8)),
    ];
    let creates = leaves.map(|(slot, _)| (slot, None));
    controller.build_evidence(&worker, build.id, &creates)?;
    let identifies = leaves.map(|(slot, file)| (slot, Some(file)));
    controller.build_evidence(&worker, build.id, &identifies)?;
    assert_eq!(
        controller.finish_build(&worker, *build, spec, [disk, vars])?,
        Status::Succeeded
    );
    let binding = store::Binding {
        principal: PRINCIPAL,
        key: key(2),
        subject: crate::vm::Subject::Template(build.id, [15; 32]),
        action: 12,
    };
    let (vm, slots, worker, reference) = controller
        .admit_static(
            binding,
            &StaticName::parse("copy-fixture").unwrap(),
            build.id,
            (build.pair, [disk, vars]),
            (digest, None),
            CLAIM,
            HOST,
            TEST_RESOURCES,
        )?
        .expect("copy admission");
    Ok((
        vm,
        slots,
        worker,
        reference.id,
        [evidence(30, 22, 4), evidence(31, 22, 5)],
    ))
}

#[test]
fn first_boot_pins_the_exact_private_generation_ca_and_later_boots_cannot_replace_it()
-> anyhow::Result<()> {
    use windows_sys::Win32::Security::TOKEN_OWNER;

    let dir = crate::service::application::tests::private_temp();
    let principal = crate::local::current_identity()?;
    let (data, creator) = super::super::tests::custody::default_owner_sid();
    // Like production workspaces, new files and directories inherit an ACL
    // for their creator's actual default owner. An elevated CI token may use
    // Administrators as its default owner.
    let path = super::super::tests::create_dir_sddl(
        dir.path(),
        "workspace",
        &format!("O:{principal}D:P(A;;FA;;;{principal})(A;OICI;FA;;;{creator})"),
    );
    // The workspace root is held with read-only sharing, as in production.
    let root = PathPin::open(&path, true, false).unwrap();
    // SAFETY: token_data supplies TokenOwner with the SID in this live buffer.
    let owner = unsafe { (*data.as_ptr().cast::<TOKEN_OWNER>()).Owner };
    let ca = b"first boot CA fixture";
    let pin = |bytes| pin_generation_ca(&root, owner, bytes, || Ok(()));

    // The refusal comes from the final pre-publication check, after the
    // private temporary copy was written, and leaves no entry behind.
    let checked = Cell::new(false);
    let refused = pin_generation_ca(&root, owner, ca, || {
        checked.set(true);
        Err(PATH)
    });
    assert_eq!(refused, Err(PATH));
    assert!(checked.get());
    let ca_dir = dir.path().join("workspace").join("origin-ca");
    let path = ca_dir.join("origin-ca.der");
    assert!(!path.exists());
    assert_eq!(fs::read_dir(&ca_dir)?.count(), 0);

    let (certificate, proof) = pin(ca).unwrap();
    assert!(
        certificate.ends_with(
            std::path::Path::new("workspace")
                .join("origin-ca")
                .join("origin-ca.der")
        )
    );
    assert_eq!(fs::read(&certificate)?, ca);
    assert_eq!(proof.sha256, crate::vm::qualification_record::digest(ca));
    let held_dir = open_relative(&root.leaf, "origin-ca", Node::Directory, false, false)?;
    assert!(sink::held_private(&held_dir, owner));
    assert!(has_exact_leaf(&held_dir, "origin-ca"));
    drop(held_dir);
    let hold = || {
        let ca_dir = open_relative(&root.leaf, "origin-ca", Node::Directory, false, false)?;
        open_relative(&ca_dir, "origin-ca.der", Node::File, false, false)
    };
    let first = hold()?;
    assert!(sink::held_private(&first, owner));
    assert!(has_exact_leaf(&first, "origin-ca.der"));
    let identity = file_identity(&first, false)?.0;
    assert_eq!(pin(ca).unwrap().1, proof);
    assert_eq!(pin(b"substituted CA"), Err(PATH));
    assert_eq!(file_identity(&hold()?, false)?.0, identity);
    assert_eq!(fs::read(path)?, ca);
    Ok(())
}

#[test]
fn static_copy_commit_rolls_back_on_bad_binding_or_watch_retirement() -> anyhow::Result<()> {
    use store::static_vm;

    let dir = crate::service::application::tests::private_temp();
    let controller = VmController::open(dir.path(), VmPolicy::default(), TestClock)?;
    let (vm, slots, worker, operation, proof) = admit_copy(&controller)?;
    let scope = native::Scope {
        generation: slots.disk.generation,
        production: id(21),
        workspace: id(22),
        lease: id(23),
    };
    let db = controller.native_store();
    let assert_pending = || -> anyhow::Result<()> {
        db.read_only(|db| {
            assert_eq!(
                static_vm::owned_state(db, PRINCIPAL, vm)?,
                Some(StaticState::Creating)
            );
            assert_eq!(
                store::count(db, "SELECT count(*) FROM vm_static_custody", [])?,
                0
            );
            assert_eq!(
                store::count(
                    db,
                    "SELECT count(*) FROM vm_artifacts WHERE generation_id=?1 AND state='planned'",
                    [scope.generation]
                )?,
                2
            );
            assert_eq!(
                store::count(
                    db,
                    "SELECT count(*) FROM vm_snapshot_release WHERE generation=?1",
                    [scope.generation]
                )?,
                0
            );
            Ok(())
        })?;
        assert_eq!(
            controller
                .operation_status(PRINCIPAL, operation)?
                .unwrap()
                .state,
            Status::Pending
        );
        Ok(())
    };

    let invalid = native::Scope {
        lease: scope.workspace,
        ..scope
    };
    let error = controller
        .commit_static_copy(&worker, slots, invalid, proof, || true)
        .unwrap_err();
    assert!(
        error.to_string().contains("CHECK constraint failed"),
        "{error}"
    );
    assert_pending()?;

    let current = Cell::new(true);
    let error = controller
        .commit_static_copy(&worker, slots, scope, proof, || current.replace(false))
        .unwrap_err();
    assert_eq!(error.to_string(), "static copy retired");
    assert_pending()?;

    controller.commit_static_copy(&worker, slots, scope, proof, || true)?;
    db.read_only(|db| {
        assert_eq!(
            static_vm::native::custody(db, PRINCIPAL, vm, StaticState::Stopped)?,
            Some((scope, slots, proof))
        );
        Ok(())
    })?;
    assert_eq!(
        controller
            .operation_status(PRINCIPAL, operation)?
            .unwrap()
            .state,
        Status::Succeeded
    );
    Ok(())
}

#[test]
fn interrupted_static_copy_retains_media_and_charge_across_reopen() -> anyhow::Result<()> {
    use store::static_vm;

    for touched in [false, true] {
        let dir = crate::service::application::tests::private_temp();
        let controller = VmController::open(dir.path(), VmPolicy::default(), TestClock)?;
        let (vm, slots, worker, operation, _) = admit_copy(&controller)?;
        let db = controller.native_store();
        let charge = db.read_only(|db| store::disk_bytes(db))?.unwrap();
        let candidate = dir.path().join(hex::encode(slots.disk.generation));
        let leaf = candidate.join(disposal::leaf_name(slots.disk));
        if touched {
            fs::create_dir(&candidate)?;
            fs::write(&leaf, b"interrupted media")?;
        }

        settle_copy_failure(&controller, &worker, slots, touched)?;
        drop(controller);
        let controller = VmController::open(dir.path(), VmPolicy::default(), TestClock)?;
        let db = controller.native_store();
        db.read_only(|db| {
            let charge_after = if touched { charge } else { charge - CLAIM.total().unwrap() };
            assert_eq!(store::disk_bytes(db)?, Some(charge_after));
            let state = touched.then_some(StaticState::Failed);
            assert_eq!(static_vm::owned_state(db, PRINCIPAL, vm)?, state);
            assert_eq!(store::count(db, "SELECT count(*) FROM vm_artifacts WHERE generation_id=?1 AND state='quarantined'", [slots.disk.generation])?, if touched { 2 } else { 0 });
            assert_eq!(store::count(db, "SELECT count(*) FROM vm_static_custody", [])?, 0);
            Ok(())
        })?;
        let status = controller.operation_status(PRINCIPAL, operation)?.unwrap();
        assert_eq!(status.state, Status::Failed);
        assert_eq!(
            status.cleanup,
            Some(if touched { "unconfirmed" } else { "confirmed" })
        );
        if touched {
            assert_eq!(fs::read(&leaf)?, b"interrupted media");
        } else {
            assert!(!candidate.exists());
        }
    }
    Ok(())
}
