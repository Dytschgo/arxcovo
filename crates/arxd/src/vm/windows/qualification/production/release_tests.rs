use super::tests::custody::Fixture;
use super::*;
use crate::store::vm::{
    self as store, Binding, VmStore,
    native::{
        Origin,
        release::{StopReceipt, Target},
    },
};
use crate::vm::{ClockReading, RequestId, Status, Subject};
use anyhow::Result;
use rusqlite::{Connection, params};

const VM: Id = [93; 16];
const TEMPLATE: Id = [94; 16];
const EPOCH: Id = [95; 16];
const CHARGE: u64 = 4096;
const BEFORE: &[u8] = b"before boot";
const AFTER: &[u8] = b"released after boot";

fn clone_pair(fixture: &Fixture) -> Result<(ProductionWorkspace, PairSlots, [FileIdentity; 2])> {
    let (activity, slots) = fixture.store().transact(|db| {
        db.execute(
            "INSERT INTO vm_templates(id,guest,recipe_digest,disk_digest,vars_digest,bytes)
             VALUES(?1,1,?2,?2,?2,1)",
            params![TEMPLATE, [1u8; 32]],
        )?;
        db.execute(
            "INSERT INTO vm_instances(id,template,principal,expires_ms) VALUES(?1,?2,'alice',1000)",
            params![VM, TEMPLATE],
        )?;
        let activity = Binding {
            principal: "alice",
            key: RequestId::parse("00000000000000640000000000000001").unwrap(),
            subject: Subject::Template(TEMPLATE, [1; 32]),
            action: 1,
        }
        .record(db, 100, Some(VM), Some(1000))?
        .receipt
        .activity_id;
        db.execute(
            "INSERT INTO vm_disk_claims(activity_id,vm,persistent,staging) VALUES(?1,?2,?3,0)",
            params![activity, VM, CHARGE],
        )?;
        Ok((activity, store::plan_clone(db, VM)?))
    })?;
    let ledger = LedgerCheckpoints {
        store: fixture.store(),
        origin: Origin::Clone,
    };
    let ws = fixture.reserve(&ledger, slots.disk.generation).unwrap();
    let file = |slot| {
        let mut writer = ws.create_slot(slot, &ledger).unwrap();
        writer.write_all(BEFORE).unwrap();
        writer.sync().unwrap();
        let evidence = ArtifactEvidence {
            identity: writer.identity(),
            directory: ws.binding().workspace.file,
            pin: crate::vm::paths::ArtifactPin {
                size: BEFORE.len() as u64,
                sha256: crate::vm::qualification_record::digest(BEFORE),
            },
        };
        ledger.slot_verified(ws.binding(), slot, evidence).unwrap();
        evidence
    };
    let evidence = [file(slots.disk), file(slots.vars)];
    fixture.store().transact(|db| {
        assert_eq!(
            store::settle_clone(db, activity, slots, Some(evidence), true, 100, None)?,
            Status::Succeeded
        );
        db.execute("UPDATE vm_instances SET state=2 WHERE id=?1", [VM])?;
        store::snapshot::invalidate_release(db, VM, EPOCH)
    })?;
    for slot in [slots.disk, slots.vars] {
        std::fs::write(slot_path(&ws.root.path, slot).unwrap().as_str(), AFTER)?;
    }
    Ok((ws, slots, evidence.map(|file| file.identity)))
}

pub(super) fn proof<'a>(
    ws: &'a ProductionWorkspace,
    slots: PairSlots,
    ids: [FileIdentity; 2],
    disposition: StopDisposition,
) -> ReleasedGeneration<'a> {
    let pins: std::sync::Arc<[PathPin]> = [
        ws.pin_slot(slots.disk, ids[0], true).unwrap(),
        ws.pin_slot(slots.vars, ids[1], true).unwrap(),
    ]
    .into();
    let guards = pins
        .iter()
        .map(PathPin::reopen_without_writer)
        .collect::<io::Result<_>>()
        .unwrap();
    let mut proof = ReleasedGeneration::checked(
        ws.binding(),
        ReleaseTargets::Pair { pair: slots, ids },
        WorkspaceCustody::production(&ws.root, &ws.lease).unwrap(),
        (pins, guards),
    )
    .unwrap();
    proof.stop_disposition = Some(disposition);
    proof
}

fn observe<'a>(proof: &'a ReleasedGeneration<'_>) -> custody::TestRelease<'a> {
    proof
        .observe_test_release(
            Target {
                vm: VM,
                generation: proof.binding.generation,
                epoch: EPOCH,
            },
            Instant::now() + Duration::from_secs(10),
            &|| false,
        )
        .unwrap()
}

fn admit_stop(store: &VmStore, deadline: Instant) -> Result<(i64, StopReceipt)> {
    store.transact(|db| {
        let id = Binding {
            principal: "alice",
            key: RequestId::parse("00000000000000640000000000000002").unwrap(),
            subject: Subject::Vm(VM),
            action: 4,
        }
        .record(db, 100, Some(VM), None)?
        .receipt
        .activity_id;
        Ok((id, StopReceipt::at_admission(db, VM, id, deadline)?))
    })
}

fn clock(monotonic: Instant) -> ClockReading {
    ClockReading {
        utc_ms: 102,
        monotonic,
    }
}

type ReceiptState = (i64, Option<i64>, Option<i64>, Option<bool>, Option<i64>);
fn receipt(db: &Connection, id: i64) -> Result<ReceiptState> {
    Ok(db.query_row(
        "SELECT a.status,a.finished_ms,a.terminal_order,a.cleanup_confirmed,o.failure_reason
         FROM activity a JOIN vm_operations o ON o.activity_id=a.id WHERE a.id=?1",
        [id],
        |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        },
    )?)
}

fn assert_release(
    db: &Connection,
    generation: Id,
    disposition: Option<&str>,
    bytes: &[u8],
    state: i64,
) -> Result<()> {
    assert_eq!(
        db.query_row(
            "SELECT disposition FROM vm_snapshot_release WHERE generation=?1",
            [generation],
            |row| row.get::<_, Option<String>>(0),
        )?
        .as_deref(),
        disposition
    );
    let digest = crate::vm::qualification_record::digest(bytes);
    assert_eq!(
        store::count(
            db,
            "SELECT count(*) FROM vm_artifacts WHERE generation_id=?1 AND state='active'
         AND content_digest=?2 AND bytes=?3",
            params![generation, digest, bytes.len() as u64],
        )?,
        2
    );
    assert_eq!(db.query_row(
        "SELECT v.state,d.persistent,d.staging FROM vm_instances v JOIN vm_disk_claims d ON d.vm=v.id
         WHERE v.id=?1 AND d.snapshot IS NULL",
        [VM], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, u64>(1)?, row.get::<_, u64>(2)?)),
    )?, (state, CHARGE, 0));
    Ok(())
}

#[test]
fn test_release_clock_crossing_commits_without_stop_success() -> Result<()> {
    let fixture = Fixture::new();
    let (ws, slots, ids) = clone_pair(&fixture)?;
    let held = proof(&ws, slots, ids, StopDisposition::CleanRelease);
    let observed = observe(&held);
    let deadline = Instant::now() + Duration::from_secs(10);
    let (id, stop) = admit_stop(fixture.store(), deadline)?;
    let other = Connection::open(fixture.state.path().join("activity.sqlite3"))?;
    other.busy_timeout(Duration::ZERO)?;
    // The fake clock advances to cutoff when the commit holds the write
    // transaction. Sampling it before acquiring SQLite gives an on-time stop.
    let status = observed.commit(fixture.store(), Some(&stop), || {
        let reading = if let Err(error) = other.execute_batch("BEGIN IMMEDIATE") {
            assert_eq!(
                error.sqlite_error_code(),
                Some(rusqlite::ErrorCode::DatabaseBusy)
            );
            deadline
        } else {
            other.execute_batch("ROLLBACK").unwrap();
            deadline - Duration::from_nanos(1)
        };
        clock(reading)
    })?;
    assert_eq!(status, Some(Status::Interrupted));
    fixture.store().transact(|db| {
        assert_release(db, ws.binding().generation, Some("clean"), AFTER, 3)?;
        assert_eq!(receipt(db, id)?.1, Some(102));
        assert_eq!(receipt(db, id)?.3, Some(false));
        Ok(())
    })?;
    assert_eq!(
        observed.commit(fixture.store(), Some(&stop), || panic!(
            "terminal stop clock"
        ))?,
        Some(Status::Interrupted)
    );
    Ok(())
}

#[test]
fn test_release_reaper_first_preserves_the_entire_receipt() -> Result<()> {
    let fixture = Fixture::new();
    let (ws, slots, ids) = clone_pair(&fixture)?;
    let held = proof(&ws, slots, ids, StopDisposition::DirtyRelease);
    let observed = observe(&held);
    let (id, stop) = admit_stop(fixture.store(), Instant::now())?;
    let old = fixture.store().transact(|db| {
        store::recover(db, 1000)?;
        store::expiry_claims(db, 1000)?;
        let old = receipt(db, id)?;
        assert_eq!(old.0, Status::Interrupted.code());
        Ok(old)
    })?;
    assert_eq!(
        observed.commit(fixture.store(), Some(&stop), || panic!(
            "terminal stop clock"
        ))?,
        Some(Status::Interrupted)
    );
    fixture.store().transact(|db| {
        assert_eq!(receipt(db, id)?, old);
        assert_release(db, ws.binding().generation, Some("dirty"), AFTER, 4)?;
        assert_eq!(store::expiry_intents(db)?.len(), 1);
        Ok(())
    })
}

#[test]
fn test_release_rollback_and_lost_ack_keep_exclusion_and_pins() -> Result<()> {
    let mut fixture = Fixture::new();
    let (ws, slots, ids) = clone_pair(&fixture)?;
    let held = proof(&ws, slots, ids, StopDisposition::CleanRelease);
    let observed = observe(&held);
    let now = Instant::now();
    let (id, stop) = admit_stop(fixture.store(), now + Duration::from_secs(10))?;
    let generation = ws.binding().generation;
    fixture.store().transact(|db| {
        db.execute_batch(
            "CREATE TRIGGER fail_test_stop BEFORE UPDATE OF status ON vm_operations
             WHEN OLD.action=4 BEGIN SELECT RAISE(ABORT, 'injected receipt rollback'); END;",
        )?;
        Ok(())
    })?;
    let failed = observed
        .commit(fixture.store(), Some(&stop), || clock(now))
        .unwrap_err();
    assert!(failed.to_string().contains("injected receipt rollback"));
    let competing = Connection::open(fixture.state.path().join("activity.sqlite3"))?;
    competing.execute_batch("BEGIN IMMEDIATE")?;
    assert_eq!(
        observed
            .commit(fixture.store(), Some(&stop), || panic!("busy transaction"))
            .unwrap_err()
            .downcast_ref::<rusqlite::Error>()
            .and_then(rusqlite::Error::sqlite_error_code),
        Some(rusqlite::ErrorCode::DatabaseBusy)
    );
    competing.execute_batch("ROLLBACK")?;
    drop(competing);
    for slot in [slots.disk, slots.vars] {
        assert!(
            std::fs::OpenOptions::new()
                .write(true)
                .open(slot_path(&ws.root.path, slot).unwrap().as_str())
                .is_err()
        );
    }
    let lease = open_relative(&ws.root.leaf, GENERATION_LEASE, Node::File, true, false)?;
    assert!(lease.try_lock().is_err());
    drop(lease);

    fixture.store().transact(|db| {
        assert_release(db, generation, None, BEFORE, 2)?;
        assert_eq!(receipt(db, id)?.0, Status::Pending.code());
        db.execute_batch("DROP TRIGGER fail_test_stop;")?;
        Ok(())
    })?;
    // A worker that loses the acknowledgement may have only its held proof
    // and original receipt stamp. Reopen the store before replaying them.
    let _lost = observed.commit(fixture.store(), Some(&stop), || clock(now))?;
    fixture.reopen();
    assert_eq!(
        observed.commit(fixture.store(), Some(&stop), || panic!(
            "terminal stop clock"
        ))?,
        Some(Status::Succeeded)
    );
    fixture.store().transact(|db| {
        assert_release(db, generation, Some("clean"), AFTER, 1)?;
        assert_eq!(receipt(db, id)?.3, Some(true));
        Ok(())
    })
}

#[test]
fn test_release_rejects_stale_epoch_and_conflicting_input() -> Result<()> {
    let fixture = Fixture::new();
    let (ws, slots, ids) = clone_pair(&fixture)?;
    let mut held = proof(&ws, slots, ids, StopDisposition::DirtyRelease);
    let observed = observe(&held);
    let (id, stop) = admit_stop(fixture.store(), Instant::now())?;
    let next_epoch = [97; 16];
    let generation = ws.binding().generation;
    fixture.store().transact(|db| {
        store::interrupt_pending(db, VM, 101, false)?;
        store::snapshot::invalidate_release(db, VM, next_epoch)
    })?;
    assert!(
        observed
            .commit(fixture.store(), None, || clock(Instant::now()))
            .is_err()
    );
    let target = Target {
        vm: VM,
        generation,
        epoch: next_epoch,
    };
    let next = |held: &ReleasedGeneration<'_>| -> Result<()> {
        let new = held
            .observe_test_release(target, Instant::now() + Duration::from_secs(10), &|| false)
            .unwrap();
        assert!(
            new.commit(fixture.store(), Some(&stop), || panic!("wrong receipt"))
                .is_err()
        );
        fixture.store().transact(|db| {
            assert_release(db, generation, None, BEFORE, 2)?;
            assert_eq!(receipt(db, id)?.0, Status::Interrupted.code());
            Ok(())
        })?;
        assert_eq!(
            new.commit(fixture.store(), None, || clock(Instant::now()))?,
            None
        );
        Ok(())
    };
    next(&held)?;
    held.stop_disposition = Some(StopDisposition::CleanRelease);
    let conflict = held
        .observe_test_release(target, Instant::now() + Duration::from_secs(10), &|| false)
        .unwrap();
    assert!(
        conflict
            .commit(fixture.store(), None, || panic!("conflicting release"))
            .is_err()
    );
    fixture
        .store()
        .transact(|db| assert_release(db, generation, Some("dirty"), AFTER, 3))
}

#[test]
fn starting_stop_with_clean_on_time_release_settles_stopped_without_recovery() -> Result<()> {
    for (disposition, expected, state, confirmed) in [
        (
            StopDisposition::CleanRelease,
            Status::Succeeded,
            1,
            Some(true),
        ),
        (
            StopDisposition::DirtyRelease,
            Status::Interrupted,
            3,
            Some(false),
        ),
    ] {
        let fixture = Fixture::new();
        let (ws, slots, ids) = clone_pair(&fixture)?;
        // The start epoch is installed but the VM never reached running.
        fixture
            .store()
            .transact(|db| Ok(db.execute("UPDATE vm_instances SET state=1 WHERE id=?1", [VM])?))?;
        let held = proof(&ws, slots, ids, disposition);
        let observed = observe(&held);
        let now = Instant::now();
        let (id, stop) = admit_stop(fixture.store(), now + Duration::from_secs(10))?;
        assert_eq!(
            observed.commit(fixture.store(), Some(&stop), || clock(now))?,
            Some(expected)
        );
        let clean = disposition == StopDisposition::CleanRelease;
        fixture.store().transact(|db| {
            let committed = if clean { "clean" } else { "dirty" };
            assert_release(db, slots.disk.generation, Some(committed), AFTER, state)?;
            assert_eq!(receipt(db, id)?.0, expected.code());
            assert_eq!(receipt(db, id)?.3, confirmed);
            Ok(())
        })?;
    }
    assert_admission_closed();
    Ok(())
}

#[test]
fn live_stop_release_commits_without_a_stop_receipt() -> Result<()> {
    let fixture = Fixture::new();
    let (workspace, slots, ids) = clone_pair(&fixture)?;
    let proof = proof(&workspace, slots, ids, StopDisposition::DirtyRelease);
    let release = observe(&proof);
    release.try_commit_stop(fixture.store(), None, || clock(Instant::now()))?;
    fixture
        .store()
        .transact(|db| assert_release(db, slots.disk.generation, Some("dirty"), AFTER, 3))?;
    assert_admission_closed();
    Ok(())
}
