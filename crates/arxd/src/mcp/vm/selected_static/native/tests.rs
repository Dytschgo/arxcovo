use super::*;
use crate::store::vm::{self as store, backup_owner_fixture, static_vm};
use crate::vm::{Capacity, paths::ArtifactPin};
use std::cell::Cell;

const NOW: i64 = 1_800_000_000_000;
const HOST: HostObservation = HostObservation {
    owner_idle: Some(true),
    logical_cpus: Some(8),
    available_ram: Some(64 << 30),
    capacity: Some(Capacity {
        free_bytes: 250 << 30,
        managed_bytes: 0,
    }),
};

fn fixture() -> anyhow::Result<(tempfile::TempDir, Controller, Id, Origin)> {
    let (dir, db, target, generation) = backup_owner_fixture()?;
    let origin_generation = db.transact(|db| {
        db.execute(
            "UPDATE vm_static_instances SET state=1 WHERE id=?1",
            [target.vm],
        )?;
        static_vm::origin_generation(db, target.vm)
    })?;
    drop(db);
    let clock = ForegroundClock {
        utc: Some(Arc::new(std::sync::atomic::AtomicI64::new(NOW))),
    };
    let controller = VmController::open(dir.path(), VmPolicy::default(), clock)?;
    let origin = Origin {
        generation,
        origin_generation,
        boot: [73; 16],
        port: 49152.try_into().unwrap(),
        ca_path: dir.path().join("origin-ca.der"),
        ca_receipt: ArtifactPin {
            size: 1,
            sha256: [7; 32],
        },
        retired: Default::default(),
    };
    Ok((dir, controller, target.vm, origin))
}

fn admit(
    controller: &Controller,
    vm: Id,
    nonce: u8,
    operation: Operation<'_>,
) -> anyhow::Result<(PendingOperation, Id)> {
    let request = Request {
        principal: "alice",
        vm,
        request_id: RequestId {
            key: [nonce; 16],
            issued_at_ms: NOW,
        },
        operation,
        disk: DiskClaim::default(),
    };
    let Admission::Admitted(worker) = controller.admit(request, HOST, TEST_RESOURCES)? else {
        panic!("expected fresh admission");
    };
    let receipt = controller.foreground_operation_ref("alice", worker.activity_id())?;
    Ok((worker, receipt))
}

fn state(controller: &Controller, vm: Id) -> anyhow::Result<Option<StaticState>> {
    controller
        .native_store()
        .read_only(|db| static_vm::owned_state(db, "alice", vm))
}

fn media(controller: &Controller, generation: Id) -> anyhow::Result<Vec<String>> {
    controller.native_store().read_only(|db| {
        let mut rows = db.prepare(
            "SELECT role||hex(object_key)||state||hex(volume_id)||hex(directory_id)||
                    hex(file_id)||hex(content_digest)||bytes
             FROM vm_artifacts WHERE generation_id=?1 ORDER BY role",
        )?;
        Ok(rows
            .query_map([generation], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?)
    })
}

#[test]
fn web_start_does_not_enroll_compose_and_refuses_retired_origin_or_route() -> anyhow::Result<()> {
    let (_dir, controller, vm, origin) = fixture()?;
    let (start, receipt) = admit(&controller, vm, 1, Operation::Start)?;
    let db = controller.native_store();
    let retired = Origin {
        retired: Default::default(),
        ..origin.clone()
    };
    retired.retired.retire();
    assert!(
        controller
            .finish_static_web_start(&start, vm, &retired, || true)
            .is_err()
    );
    let checks = Cell::new(true);
    assert!(
        controller
            .finish_static_web_start(&start, vm, &origin, || checks.replace(false))
            .is_err()
    );
    assert_eq!(state(&controller, vm)?, Some(StaticState::Stopped));
    assert_eq!(
        controller
            .operation_status("alice", receipt)?
            .unwrap()
            .state,
        Status::Pending
    );

    let compose_boot_count = || {
        db.read_only(|db| {
            store::count(
                db,
                "SELECT count(*) FROM vm_compose_boots WHERE vm=?1",
                [vm],
            )
        })
    };
    let before = compose_boot_count()?;
    controller.finish_static_web_start(&start, vm, &origin, || true)?;
    assert_eq!(state(&controller, vm)?, Some(StaticState::Running));
    assert_eq!(
        controller
            .operation_status("alice", receipt)?
            .unwrap()
            .state,
        Status::Succeeded
    );
    assert_eq!(compose_boot_count()?, before);
    assert_eq!(
        db.read_only(|db| {
            store::count(
                db,
                "SELECT count(*) FROM vm_compose_boots WHERE vm=?1 AND retired=0",
                [vm],
            )
        })?,
        0
    );
    Ok(())
}

#[test]
fn stop_outcomes_and_lost_run_preserve_committed_media() -> anyhow::Result<()> {
    for (request, disposition, expected, status) in [
        (
            true,
            StopDisposition::CleanRelease,
            StaticState::Stopped,
            Status::Succeeded,
        ),
        (
            true,
            StopDisposition::DirtyRelease,
            StaticState::Recovering,
            Status::Failed,
        ),
        (
            true,
            StopDisposition::ChargedQuarantine,
            StaticState::Recovering,
            Status::Interrupted,
        ),
        (
            false,
            StopDisposition::CleanRelease,
            StaticState::Recovering,
            Status::Interrupted,
        ),
    ] {
        let (_dir, controller, vm, origin) = fixture()?;
        let (start, _) = admit(&controller, vm, 1, Operation::Start)?;
        controller.finish_static_web_start(&start, vm, &origin, || true)?;
        let before = media(&controller, origin.generation)?;
        assert_eq!(before.len(), 2);

        let stop = if request {
            Some(admit(&controller, vm, 2, Operation::Stop)?)
        } else {
            None
        };
        settle_static_stop(
            &controller,
            vm,
            stop.as_ref().map(|(worker, _)| worker),
            disposition,
        )?;
        assert_eq!(state(&controller, vm)?, Some(expected));
        if let Some((_, receipt)) = stop {
            assert_eq!(
                controller
                    .operation_status("alice", receipt)?
                    .unwrap()
                    .state,
                status
            );
        }
        let db = controller.native_store();
        let claims = db.read_only(|db| static_vm::recovery_claims(db))?;
        if expected == StaticState::Recovering {
            assert_eq!(claims.len(), 1);
            assert_eq!(
                (claims[0].vm, claims[0].generation),
                (vm, origin.generation)
            );
        } else {
            assert!(claims.is_empty());
        }
        assert_eq!(media(&controller, origin.generation)?, before);
        assert_eq!(
            db.read_only(|db| {
                store::count(
                    db,
                    "SELECT count(*) FROM vm_compose_volumes WHERE vm=?1",
                    [vm],
                )
            })?,
            1
        );
    }
    Ok(())
}
