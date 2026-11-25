use super::*;

#[test]
fn completed_operation_cannot_reuse_its_worker_to_mint_an_effect() {
    let fixture = Fixture::new();
    let spec = BuildRequest {
        principal: OWNER,
        key: key(NOW, 27),
        recipe: "e03a3012c18d20b8691490b580ab41e9",
        digest: [9; 32],
        retained: disk(),
        transient: disk(),
    };
    let BuildAdmission::Fresh(slots, worker, receipt) = fixture
        .ctl
        .admit_build(spec, host(), resources(), |_, _| unreachable!())
        .unwrap()
        .unwrap()
    else {
        panic!("fresh reservation required");
    };
    let slot = slots.pair.disk;
    let mut binding = runtime::NativeEffectBinding {
        principal: OWNER,
        request: spec.key,
        operation: receipt.id,
        action: 11,
        generation: slot.generation,
    };
    assert!(
        fixture
            .ctl
            .validate_native_effect(&worker, &binding, &[slot])
            .unwrap()
    );
    binding.principal = "bob";
    assert!(
        !fixture
            .ctl
            .validate_native_effect(&worker, &binding, &[slot])
            .unwrap()
    );
    binding.principal = OWNER;
    assert!(
        !fixture
            .ctl
            .validate_native_effect(&worker, &binding, &[slots.verification.disk])
            .unwrap()
    );

    // Keep the generation and its reservation intact to isolate the receipt
    // gate. A terminal receipt never gives its old worker new effect authority.
    fixture
        .ctl
        .reaper_test_store()
        .transact(|db| {
            store::finish(
                db,
                worker.activity_id(),
                Status::Interrupted,
                NOW,
                Some(false),
            )
        })
        .unwrap();
    assert!(
        !fixture
            .ctl
            .validate_native_effect(&worker, &binding, &[slot])
            .unwrap()
    );
    assert!(matches!(
        fixture.ctl.admit_build(spec, host(), resources(), |_, _| unreachable!()).unwrap().unwrap(),
        BuildAdmission::Existing(existing) if existing.id == receipt.id
    ));
}

#[test]
fn native_utc_high_water_survives_a_store_reopen() {
    let fixture = Fixture::new();
    fixture.clock.set(NOW + 2_000, 2);
    assert!(fixture.ctl.advance_native_clock().unwrap().is_some());
    let fixture = fixture.reopen();
    fixture.clock.set(NOW + 1_000, 1);
    assert!(fixture.ctl.advance_native_clock().unwrap().is_none());
    fixture.clock.set(NOW + 3_000, 1);
    assert!(fixture.ctl.advance_native_clock().unwrap().is_some());
}

#[test]
fn native_monitor_queries_refuse_store_contention() {
    use std::sync::mpsc;
    let fixture = Fixture::new();
    let controller = &fixture.ctl;
    std::thread::scope(|scope| {
        let held = controller
            .reaper_test_store()
            .try_live_connection()
            .unwrap();
        let (send, receive) = mpsc::channel();
        scope.spawn(move || {
            let clock = controller.advance_native_clock();
            let live = controller.native_vm_unexpired(OWNER, [0; 16]);
            let _ = send.send(clock.is_err() && live.is_err());
        });
        let result = receive.recv_timeout(std::time::Duration::from_secs(2));
        drop(held);
        assert_eq!(result, Ok(true));
    });
}

fn native_admit(
    f: &Fixture,
    vm: Id,
    number: u8,
    principal: &str,
    operation: Operation<'_>,
    epoch: Option<Id>,
) -> Admission {
    f.ctl
        .admit_native_operation(
            Request {
                principal,
                request_id: key(f.clock.now().utc_ms, number),
                vm,
                disk: DiskClaim::default(),
                operation,
            },
            host(),
            resources(),
            epoch,
        )
        .unwrap()
}

#[test]
fn linked_clone_has_a_bounded_file_verification_window() {
    let f = Fixture::new();
    let (_, _, clone) = f.reserve_clone(f.build(41, true), 42);
    assert_eq!(
        clone.deadline() - f.clock.now().monotonic,
        runtime::SNAPSHOT_COPY_MAXIMUM
    );
}

#[test]
fn slow_valid_native_start_still_has_a_publishable_witness_epoch() {
    let f = Fixture::new();
    let (vm, slots) = f.clone(f.build(11, true), 12, true);
    f.ctl
        .extend_lease(OWNER, vm, key(NOW, 13), 600)
        .unwrap()
        .unwrap();
    let Admission::Admitted(worker) = native_admit(&f, vm, 14, OWNER, Operation::Start, None)
    else {
        panic!("start refused")
    };
    let epoch = f
        .ctl
        .foreground_operation_ref(OWNER, worker.activity_id())
        .unwrap();
    f.clock.set(NOW + 150_000, 150);
    assert!(worker.deadline() > f.clock.now().monotonic);
    assert_eq!(
        f.ctl.finish(&worker, Completion::Succeeded).unwrap(),
        Status::Succeeded
    );
    assert_eq!(
        f.ctl.native_running_epoch(OWNER, vm).unwrap(),
        Some((slots.disk.generation, epoch))
    );
}

#[test]
fn boot_interrupt_stop_requires_the_current_owner_and_epoch_and_replays() {
    let f = Fixture::new();
    let (vm, _) = f.clone(f.build(21, true), 22, true);
    let start = f.worker(vm, 23, Operation::Start);
    let epoch = f
        .ctl
        .foreground_operation_ref(OWNER, start.activity_id())
        .unwrap();
    for (owner, binding) in [("bob", epoch), (OWNER, [0; 16])] {
        assert!(matches!(
            native_admit(&f, vm, 24, owner, Operation::Stop, Some(binding)),
            Admission::Refused(_)
        ));
    }
    let Admission::Admitted(stop) = native_admit(&f, vm, 25, OWNER, Operation::Stop, Some(epoch))
    else {
        panic!("stop refused")
    };
    assert!(matches!(
        native_admit(&f, vm, 25, OWNER, Operation::Stop, Some(epoch)),
        Admission::Existing(_)
    ));
    assert!(matches!(
        native_admit(&f, vm, 26, OWNER, Operation::Stop, Some(epoch)),
        Admission::Refused(_)
    ));
    assert_eq!(
        f.ctl
            .finish_native_start(&start, epoch, Completion::Succeeded)
            .unwrap(),
        Status::Interrupted
    );
    assert_eq!(
        f.count(
            "SELECT count(*) FROM activity WHERE vm IS NOT NULL AND action IN (3,4) AND status=0"
        ),
        1
    );
    f.ctl.finish_native_stop(&stop, vm, epoch).unwrap();
    assert_eq!(
        f.count("SELECT count(*) FROM activity WHERE action=4 AND cleanup_confirmed=0"),
        1
    );
}

#[test]
fn lost_channel_fences_only_its_epoch_and_recovery_waits_for_held_release() {
    let f = Fixture::new();
    let (vm, _) = f.clone(f.build(31, true), 32, true);
    let active = f.ctl.native_start_pair(OWNER, vm).unwrap().unwrap();
    let start = f.worker(vm, 33, Operation::Start);
    let epoch = f
        .ctl
        .foreground_operation_ref(OWNER, start.activity_id())
        .unwrap();
    f.ctl.finish(&start, Completion::Succeeded).unwrap();
    assert!(f.ctl.native_quarantine(vm, [0; 16]).is_err());
    f.ctl.native_quarantine(vm, epoch).unwrap();
    f.ctl.native_quarantine(vm, epoch).unwrap();
    assert_eq!(
        f.count("SELECT count(*) FROM vm_instances WHERE state=3 AND cleanup_id IS NOT NULL"),
        1
    );
    let claim = f.ctl.recovery_intents().unwrap().pop().unwrap();
    let cleanup = f
        .ctl
        .reaper_test_store()
        .transact(|db| store::cleanup_work(db, claim.into()))
        .unwrap()
        .unwrap();
    assert!(!f.ctl.native_cleanup_safe(&cleanup).unwrap());
    assert_eq!(
        f.count("SELECT count(*) FROM vm_disk_claims WHERE persistent>0"),
        1
    );
    let Admission::Admitted(stop) = native_admit(&f, vm, 34, OWNER, Operation::Stop, Some(epoch))
    else {
        panic!("recovery stop refused")
    };
    f.ctl
        .native_record_release(
            vm,
            epoch,
            active.pair,
            runtime::StopDisposition::DirtyRelease,
        )
        .unwrap();
    f.ctl.finish_native_stop(&stop, vm, epoch).unwrap();
    assert!(f.ctl.native_cleanup_safe(&cleanup).unwrap());
}

#[test]
fn native_expiry_requires_release_for_the_exact_active_generation() {
    let fixture = Fixture::new();
    let golden = fixture.build(50, true);
    let (vm, _) = fixture.clone(golden, 51, true);
    assert!(fixture.ctl.native_vm_unexpired(OWNER, vm).unwrap());
    assert!(!fixture.ctl.native_vm_unexpired("bob", vm).unwrap());
    fixture.clock.set(NOW + 61_000, 61);
    assert!(!fixture.ctl.native_vm_unexpired(OWNER, vm).unwrap());
    let claim = fixture.ctl.claim_expired().unwrap().pop().unwrap();
    let work = fixture
        .ctl
        .reaper_test_store()
        .transact(|db| store::cleanup_work(db, claim.into()))
        .unwrap()
        .unwrap();
    assert_eq!(work.kind.vm(), Some(vm));
    assert!(fixture.ctl.native_cleanup_safe(&work).unwrap());
    fixture
        .ctl
        .reaper_test_store()
        .transact(|db| store::snapshot::invalidate_release(db, vm, [3; 16]))
        .unwrap();
    assert!(!fixture.ctl.native_cleanup_safe(&work).unwrap());
    assert_eq!(
        fixture.count("SELECT count(*) FROM vm_disk_claims WHERE persistent>0"),
        1
    );
}

#[test]
fn foreground_test_stop_precheck_admits_boot_and_recovery_for_the_exact_epoch() {
    use crate::vm::controller::{ForegroundSubject, Precheck};
    let f = Fixture::new();
    let (vm, _) = f.clone(f.build(61, true), 62, true);
    let start = f.worker(vm, 63, Operation::Start);
    let epoch = f
        .ctl
        .foreground_operation_ref(OWNER, start.activity_id())
        .unwrap();
    let check = |owner: &str, subject| f.ctl.foreground_precheck(owner, subject).unwrap();
    let assert_states = || {
        assert_eq!(
            check(OWNER, ForegroundSubject::Stop(vm)),
            Err(Precheck::Refused(Refusal::Busy))
        );
        assert_eq!(check(OWNER, ForegroundSubject::TestStop(vm, epoch)), Ok(()));
        assert_eq!(
            check(OWNER, ForegroundSubject::TestStop(vm, [0; 16])),
            Err(Precheck::Refused(Refusal::Busy))
        );
        assert_eq!(
            check("bob", ForegroundSubject::TestStop(vm, epoch)),
            Err(Precheck::Refused(Refusal::NotFound))
        );
    };
    // The boot is still pending.
    assert_states();
    // A lost channel fenced the generation in recovery.
    f.ctl.finish(&start, Completion::Succeeded).unwrap();
    f.ctl.native_quarantine(vm, epoch).unwrap();
    assert_states();
    // Both prechecks agree with the epoch-bound stop admission.
    assert!(matches!(
        native_admit(&f, vm, 64, OWNER, Operation::Stop, Some(epoch)),
        Admission::Admitted(_)
    ));
}

#[tokio::test(start_paused = true)]
async fn unlaunched_start_releases_its_pair_or_stays_fenced_until_a_retry() {
    use runtime::test_issuer::settle_unlaunched;
    use std::cell::Cell;
    let f = Fixture::new();
    let (vm, _) = f.clone(f.build(71, true), 72, true);
    let (pair, evidence) = f.ctl.native_start_pair(OWNER, vm).unwrap().unwrap().pair;
    let admit_start = |number| {
        let Admission::Admitted(start) =
            native_admit(&f, vm, number, OWNER, Operation::Start, None)
        else {
            panic!("start refused")
        };
        let epoch = f
            .ctl
            .foreground_operation_ref(OWNER, start.activity_id())
            .unwrap();
        (start, epoch)
    };

    // The first observation is refused: the epoch is fenced and charged, and
    // the woken retry records the stopped pair for recovery.
    let (start, epoch) = admit_start(73);
    let wake = tokio::sync::Notify::new();
    let attempts = Cell::new(0);
    settle_unlaunched(
        || {
            f.ctl
                .finish_native_start(&start, epoch, Completion::CleanupUnconfirmed)?;
            Ok(())
        },
        &wake,
        || {
            attempts.set(attempts.get() + 1);
            (attempts.get() > 1).then_some(evidence)
        },
        |files| {
            assert_eq!(
                f.count("SELECT count(*) FROM vm_instances WHERE state=3"),
                1
            );
            f.ctl.native_record_release(
                vm,
                epoch,
                (pair, *files),
                runtime::StopDisposition::CleanRelease,
            )
        },
        || {
            f.ctl.native_quarantine(vm, epoch)?;
            wake.notify_one();
            Ok(())
        },
    )
    .await
    .unwrap();
    assert_eq!(attempts.get(), 2);
    assert_eq!(
        f.count("SELECT count(*) FROM vm_disk_claims WHERE persistent>0"),
        1
    );
    let claim = f.ctl.recovery_intents().unwrap().pop().unwrap();
    let cleanup = f
        .ctl
        .reaper_test_store()
        .transact(|db| store::cleanup_work(db, claim.into()))
        .unwrap()
        .unwrap();
    assert!(f.ctl.native_cleanup_safe(&cleanup).unwrap());

    // Without a recorded release the recovery that the unconfirmed start
    // opened is stranded. A proof on the first observation unblocks it.
    let f = Fixture::new();
    let (vm, _) = f.clone(f.build(74, true), 75, true);
    let (pair, evidence) = f.ctl.native_start_pair(OWNER, vm).unwrap().unwrap().pair;
    let Admission::Admitted(start) = native_admit(&f, vm, 76, OWNER, Operation::Start, None) else {
        panic!("start refused")
    };
    let epoch = f
        .ctl
        .foreground_operation_ref(OWNER, start.activity_id())
        .unwrap();
    settle_unlaunched(
        || {
            f.ctl
                .finish_native_start(&start, epoch, Completion::CleanupUnconfirmed)?;
            Ok(())
        },
        &tokio::sync::Notify::new(),
        || {
            let claim = f.ctl.recovery_intents().unwrap().pop().unwrap();
            let cleanup = f
                .ctl
                .reaper_test_store()
                .transact(|db| store::cleanup_work(db, claim.into()))
                .unwrap()
                .unwrap();
            assert!(!f.ctl.native_cleanup_safe(&cleanup).unwrap());
            Some(evidence)
        },
        |files| {
            f.ctl.native_record_release(
                vm,
                epoch,
                (pair, *files),
                runtime::StopDisposition::CleanRelease,
            )
        },
        || panic!("a proved release must not fence"),
    )
    .await
    .unwrap();
    let claim = f.ctl.recovery_intents().unwrap().pop().unwrap();
    let cleanup = f
        .ctl
        .reaper_test_store()
        .transact(|db| store::cleanup_work(db, claim.into()))
        .unwrap()
        .unwrap();
    assert!(f.ctl.native_cleanup_safe(&cleanup).unwrap());
}

#[test]
fn failed_clone_staging_and_build_reach_native_disposal_after_their_workers() {
    let f = Fixture::new();
    let golden = f.build(81, true);
    let (vm, slots, worker) = f.reserve_clone(golden, 82);
    f.clock.set(NOW + 61_000, 1);
    f.sweep(&Runtime::new(|_| Quiesced::Unconfirmed));
    let staging = || {
        let store = f.ctl.reaper_test_store();
        let activity_id = store
            .transact(|db| {
                Ok(db.query_row(
                    "SELECT cleanup_id FROM vm_instances WHERE id=?1 AND cleanup_route='staging'",
                    [vm],
                    |row| row.get(0),
                )?)
            })
            .unwrap();
        store
            .transact(|db| store::cleanup_work(db, CleanupKind::Staging { vm, activity_id }))
            .unwrap()
            .unwrap()
    };
    // The claim interrupted the clone receipt. The native reaper's in-memory
    // job registry keeps it unconfirmed until the worker returns.
    f.clock.set(NOW, 0);
    f.ctl.finish_linked_clone(&worker, slots, None).unwrap();
    let work = staging();
    assert!(work.accounted);
    assert!(f.ctl.native_cleanup_safe(&work).unwrap());

    // That staging row correctly keeps fresh admission closed, so the failed
    // build runs on its own ledger.
    let f = Fixture::new();
    let spec = BuildRequest {
        principal: OWNER,
        key: key(NOW, 83),
        recipe: "e03a3012c18d20b8691490b580ab41e9",
        digest: [83; 32],
        retained: disk(),
        transient: disk(),
    };
    let BuildAdmission::Fresh(build, worker, _) = f
        .ctl
        .admit_build(spec, host(), resources(), |_, _| unreachable!())
        .unwrap()
        .unwrap()
    else {
        panic!("fresh build required");
    };
    let cleanup = || {
        f.ctl
            .reaper_test_store()
            .transact(|db| store::cleanup_work(db, CleanupKind::Build(build.id)))
            .unwrap()
    };
    if let Some(pending) = cleanup() {
        assert!(!f.ctl.native_cleanup_safe(&pending).unwrap());
    }
    f.ctl
        .fail_build_with(&worker, *build, false, Some(FailureReason::InstallFailed))
        .unwrap();
    let work = cleanup().unwrap();
    assert!(work.accounted);
    assert_eq!(
        work.artifacts
            .iter()
            .map(|asset| asset.role.as_str())
            .collect::<Vec<_>>(),
        ["disk", "seed", "vars", "vars", "verification_overlay"]
    );
    assert!(f.ctl.native_cleanup_safe(&work).unwrap());
}
