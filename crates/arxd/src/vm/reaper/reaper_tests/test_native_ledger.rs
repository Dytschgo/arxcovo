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
