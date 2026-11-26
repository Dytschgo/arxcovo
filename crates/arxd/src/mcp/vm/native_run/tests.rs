use super::*;
use crate::{
    store::vm::{
        self as store,
        native::{self, release::{Observed, Target}},
    },
    vm::{paths::FileIdentity, test_issuer::Installation},
};
use rusqlite::{OptionalExtension, params};
use std::sync::atomic::{AtomicBool, AtomicUsize};
use super::super::native_run::LiveLaunch;

fn assert_admission_closed(h: &Harness) {
    let candidate = h.supervisor().candidate();
    assert!(candidate.dispatch(runtime::NativeMutation::Boot, |_| ()).is_err());
    assert!(matches!(candidate.capability(), runtime::NativeCapability::Unavailable(_)));
}

struct Starting {
    owner: Arc<native_run::Run>,
    worker: PendingOperation,
    registration: owned_worker::Registration<Arc<native_run::Run>>,
    observed: Observed,
}

type BoundClone = (Id, crate::vm::PairSlots, [crate::vm::ArtifactEvidence; 2], native::Scope);

/// A stopped TEST clone whose clean released pair is bound to a native
/// workspace. Every later request goes through the closed dispatcher.
async fn bound_clone(h: &Harness, client: &Client) -> BoundClone {
    let build = call(client, "vm_template_build", json!({
        "recipe_id":hex::encode(Controller::windows_recipe_id()), "request_id":key(500)
    })).await;
    assert_eq!(build["ok"], true, "{build}");
    h.hold_build.notify_one();
    let template = terminal(client, &build).await["template_id"].clone();
    let create = call(client, "vm_create", json!({
        "template_id":template, "request_id":key(501)
    })).await;
    let cloned = terminal(client, &create).await;
    assert_eq!(cloned["state"], "succeeded", "{cloned}");
    let vm = parse_id(cloned["vm_id"].as_str().unwrap()).unwrap();
    let supervisor = h.supervisor();
    let controller = &supervisor.controller;
    let store = controller.native_store();
    let (slots, pins, scope) = store.transact(|db| {
        let (slots, pins) = store::snapshot::active(db, vm, false)?.unwrap().pair;
        let generation = slots.disk.generation;
        let scope = native::Scope {
            generation,
            production: FileIdentity { volume: pins[0].identity.volume, file: [32; 16] },
            workspace: FileIdentity { volume: pins[0].identity.volume, file: pins[0].directory },
            lease: FileIdentity { volume: pins[0].identity.volume, file: [33; 16] },
        };
        db.execute("UPDATE vm_generations SET active=0 WHERE id=?1", [generation])?;
        db.execute("INSERT INTO vm_native_generations
             (generation,host,origin,protocol,volume,production,state,directory,lease)
            SELECT ?1,id,'clone',1,?2,?3,'bound',?4,?5 FROM vm_state_host",
            params![generation, scope.production.volume.to_be_bytes(), scope.production.file,
                scope.workspace.file, scope.lease.file])?;
        for (slot, pin) in [slots.disk, slots.vars].into_iter().zip(pins) {
            db.execute("INSERT INTO vm_native_slots
                (generation,role,object_key,state,file,bytes,content_digest)
                VALUES (?1,?2,?3,'verified',?4,?5,?6)",
                params![generation, slot.role, slot.key, pin.identity.file,
                    pin.pin.size, pin.pin.sha256])?;
        }
        db.execute("UPDATE vm_generations SET active=1 WHERE id=?1", [generation])?;
        Ok((slots, pins, scope))
    }).unwrap();
    // The fixture clone handler is now out of the request path. Every
    // subsequent request goes through the closed foreground dispatcher.
    supervisor.gate.lock().unwrap().fixture = None;
    assert_admission_closed(h);
    (vm, slots, pins, scope)
}

async fn reserved(h: &Harness, client: &Client) -> Starting {
    let (vm, slots, pins, scope) = bound_clone(h, client).await;
    let supervisor = h.supervisor();
    let controller = &supervisor.controller;
    let before = Instant::now();
    let request = Request {
        principal: "alpha", vm, request_id: RequestId::parse(&key(502)).unwrap(),
        operation: Operation::Start, disk: DiskClaim::default(),
    };
    let (Admission::Admitted(worker), Some(_)) = controller
        .admit_native_start(&Installation::fixture(), &request, HOST, RESOURCES).unwrap()
    else { panic!("scoped start refused") };
    assert!(worker.deadline() >= before + Duration::from_secs(360));
    let target = Target { vm, generation: scope.generation, epoch: worker.admitted().unwrap().operation };
    // Start admission invalidates the previous release and installs its own
    // operation as the unresolved writer epoch of the exact generation.
    assert_eq!(unresolved_epoch(h, vm), Some((target.generation, target.epoch)));
    assert_eq!(controller.active_release_epoch(vm, false).unwrap(), None);
    // The same transaction records the exact never-created writer epoch.
    assert_eq!(writer_epoch(h, target.generation), (target.epoch, false));
    assert!(controller.native_run_current("alpha", target, false).unwrap());
    assert!(!controller.native_run_current("alpha", target, true).unwrap());
    let owner = Arc::new(native_run::Run::new("alpha", target));
    let registration = supervisor.native_runs.register(vm, owner.clone()).unwrap();
    let observed = Observed { target, scope, slots, pins, clean: true };
    Starting { owner, worker, registration, observed }
}

fn start_request(vm: Id, number: u64) -> Request<'static> {
    Request {
        principal: "alpha", vm, request_id: RequestId::parse(&key(number)).unwrap(),
        operation: Operation::Start, disk: DiskClaim::default(),
    }
}

fn operation_count(h: &Harness) -> i64 {
    h.supervisor().controller.native_store().transact(|db| {
        Ok(db.query_row("SELECT count(*) FROM vm_operations", [], |r| r.get(0))?)
    }).unwrap()
}

fn writer_epoch(h: &Harness, generation: Id) -> (Id, bool) {
    h.supervisor().controller.native_store().transact(|db| {
        Ok(db.query_row("SELECT epoch,boot IS NOT NULL FROM vm_native_writer WHERE generation=?1",
            [generation], |r| Ok((r.get(0)?, r.get(1)?)))?)
    }).unwrap()
}

fn unresolved_epoch(h: &Harness, vm: Id) -> Option<(Id, Id)> {
    h.supervisor().controller.native_store().transact(|db| {
        Ok(db.query_row(
            "SELECT g.id,r.epoch FROM vm_generations g
             JOIN vm_snapshot_release r ON r.generation=g.id
             WHERE g.vm_id=?1 AND g.active=1 AND r.disposition IS NULL",
            [vm], |row| Ok((row.get(0)?, row.get(1)?)),
        ).optional()?)
    }).unwrap()
}

fn record_checked_creation(h: &Harness, observed: &Observed) {
    let origin = native::recovery::Origin {
        boot: [34; 32],
        database: FileIdentity { volume: observed.scope.production.volume, file: [35; 16] },
        lock: FileIdentity { volume: observed.scope.production.volume, file: [36; 16] },
    };
    h.supervisor().controller.native_store().transact(|db| {
        native::recovery::record_job(db, observed.scope, observed.target.epoch, origin)
    }).unwrap();
}

async fn windows_harness() -> Harness {
    Harness::start_configured(false, false, true, |fixture| fixture.windows_recipe = true).await
}

#[tokio::test]
async fn live_windows_witness_is_withdrawn_before_teardown_and_never_resurrected() {
    let mut h = windows_harness().await;
    let client = h.client(0).await;
    let other = h.client(1).await;
    let Starting { owner, worker, registration, observed } = reserved(&h, &client).await;
    let target = observed.target;
    let args = json!({"vm_id":hex::encode(target.vm)});
    assert_eq!(call(&client, "vm_get", args.clone()).await["ready"], false);
    assert!(!owner.commit_ready(h.supervisor(), &worker, || Some([42; 16]), || true).await);
    record_checked_creation(&h, &observed);
    assert!(!owner.commit_ready(h.supervisor(), &worker, || Some(target.epoch), || true).await);
    assert!(!owner.commit_ready(h.supervisor(), &worker, || None, || true).await);
    assert!(owner.commit_ready(h.supervisor(), &worker, || Some([42; 16]), || true).await);

    let status = call(&client, "vm_get", args.clone()).await;
    assert_eq!(status["ready"], true);
    assert_eq!(status["native_boot_witness"], json!({
        "source":"guest_ready_handshake","vm_id":args["vm_id"],
        "generation":hex::encode(target.generation),"operation_id":hex::encode(target.epoch),
        "witness_id":hex::encode([42; 16]),
    }));
    let foreign = call(&other, "vm_get", args.clone()).await;
    assert_eq!(foreign["ok"], false);
    assert!(foreign.get("native_boot_witness").is_none());

    // Drive the stalled teardown and the authenticated read on one task.
    let (entered, release) = (Notify::new(), Notify::new());
    let settled = std::cell::Cell::new(false);
    {
        let teardown = owner.settle_live(
            h.supervisor(),
            async || {
                entered.notify_one();
                release.notified().await;
                Some(())
            },
            async |()| settled.set(true),
        );
        tokio::pin!(teardown);
        tokio::select! {
            () = &mut teardown => panic!("stalled teardown finished before its release"),
            () = async {
                entered.notified().await;
                let stopped = call(&client, "vm_get", args.clone()).await;
                assert_eq!(stopped["ready"], false);
                assert!(stopped.get("native_boot_witness").is_none());
            } => {}
        }
        assert!(!settled.get());
        release.notify_one();
        teardown.await;
    }
    assert!(settled.get());
    assert!(!owner.witnessed());

    // A new foreground owner registry over the same ledger has no witness.
    drop(registration);
    let reopened = call(&client, "vm_get", args).await;
    assert_eq!(reopened["ready"], false);
    assert!(reopened.get("native_boot_witness").is_none());
    assert_admission_closed(&h);
    drop((client, other));
    // The synthetic fixture has no real Job to release during drain.
    let _ = h.shutdown().await;
}

#[tokio::test]
async fn exact_starting_stop_replays_and_signals_under_closed_admission() {
    let mut h = windows_harness().await;
    let client = h.client(0).await;
    let other = h.client(1).await;
    let Starting { owner, worker, registration: _registration, observed } = reserved(&h, &client).await;
    let args = json!({"vm_id":hex::encode(observed.target.vm), "request_id":key(503)});
    let supervisor = h.supervisor();
    assert!(!owner.stop_signalled());
    assert_eq!(call(&other, "vm_stop", args.clone()).await["ok"], false);
    assert!(!owner.stop_signalled());
    supervisor.close();
    let stop = call(&client, "vm_stop", args.clone()).await;
    assert_eq!(stop["ok"], true, "{stop}");
    assert!(owner.stop_signalled());
    assert!(owner.retry_signalled().await);
    // A replay returns the same receipt and nudges the retrying owner.
    assert_eq!(call(&client, "vm_stop", args).await, stop);
    assert!(owner.retry_signalled().await);
    assert!(!owner.commit_ready(supervisor, &worker, || Some([42; 16]), || true).await);
    let ctl = &supervisor.controller;
    ctl.native_store().transact(|db| {
        let state: i64 = db.query_row("SELECT status FROM vm_operations WHERE operation_id=?1",
            [observed.target.epoch], |r| r.get(0))?;
        assert_eq!(state, Status::Interrupted.code());
        Ok(())
    }).unwrap();
    assert_admission_closed(&h);
    drop((client, other));
    let _ = h.shutdown().await;
}

#[tokio::test]
async fn unsuccessful_start_commit_and_expired_or_replaced_epochs_cannot_project() {
    let mut h = windows_harness().await;
    let client = h.client(0).await;
    let Starting { owner, worker, registration: _registration, observed } = reserved(&h, &client).await;
    record_checked_creation(&h, &observed);
    let target = observed.target;
    let args = json!({"vm_id":hex::encode(target.vm)});
    let ctl = &h.supervisor().controller;
    let trigger = "CREATE TRIGGER fail_start BEFORE UPDATE OF status ON vm_operations
        WHEN NEW.action=3 AND NEW.status=1 BEGIN SELECT RAISE(ABORT,'fixture commit failure'); END";
    ctl.native_store().transact(|db| { db.execute_batch(trigger)?; Ok(()) }).unwrap();
    assert!(!owner.commit_ready(h.supervisor(), &worker, || Some([42; 16]), || true).await);
    assert!(ctl.native_run_current("alpha", target, false).unwrap());
    let failed = call(&client, "vm_get", args.clone()).await;
    assert_eq!(failed["ready"], false);
    assert!(failed.get("native_boot_witness").is_none());
    ctl.native_store().transact(|db| { db.execute_batch("DROP TRIGGER fail_start")?; Ok(()) }).unwrap();

    assert!(owner.commit_ready(h.supervisor(), &worker, || Some([43; 16]), || true).await);
    let expires = call(&client, "vm_get", args.clone()).await["expires_at_ms"].as_i64().unwrap();
    h.now.store(expires, Ordering::SeqCst);
    let expired = call(&client, "vm_get", args.clone()).await;
    assert_eq!(expired["ready"], false);
    assert!(expired.get("native_boot_witness").is_none());
    assert!(owner.stop_signalled());
    // Moving the clock backward cannot resurrect an already withdrawn proof.
    h.now.store(expires - 1, Ordering::SeqCst);
    ctl.native_store().transact(|db| store::snapshot::invalidate_release(db, target.vm, [44; 16])).unwrap();
    let replaced = call(&client, "vm_get", args).await;
    assert_eq!(replaced["ready"], false);
    assert!(replaced.get("native_boot_witness").is_none());
    assert_admission_closed(&h);
    drop(client);
    let _ = h.shutdown().await;
}

#[tokio::test]
async fn idle_stop_and_stale_start_epoch_each_clear_ready() {
    for replace in [false, true] {
        let mut h = windows_harness().await;
        let client = h.client(0).await;
        let Starting { owner, worker, registration: _registration, observed } =
            reserved(&h, &client).await;
        record_checked_creation(&h, &observed);
        assert!(owner.commit_ready(h.supervisor(), &worker, || Some([42; 16]), || true).await);
        let target = observed.target;
        let args = json!({"vm_id":hex::encode(target.vm)});
        assert_eq!(call(&client, "vm_get", args.clone()).await["ready"], true);
        if replace {
            h.supervisor().controller.native_store().transact(|db| {
                store::snapshot::invalidate_release(db, target.vm, [44; 16])
            }).unwrap();
        } else {
            let args = json!({"vm_id":args["vm_id"], "request_id":key(503)});
            let stop = call(&client, "vm_stop", args.clone()).await;
            assert_eq!(stop["ok"], true, "{stop}");
            assert_eq!(call(&client, "vm_stop", args).await, stop);
        }
        let stopping = call(&client, "vm_get", args).await;
        assert_eq!(stopping["ready"], false);
        assert!(stopping.get("native_boot_witness").is_none());
        assert!(owner.stop_signalled());
        assert_admission_closed(&h);
        drop(client);
        let _ = h.shutdown().await;
    }
}

fn vm_state(h: &Harness, vm: Id) -> i64 {
    h.supervisor().controller.native_store().transact(|db| {
        Ok(db.query_row("SELECT state FROM vm_instances WHERE id=?1", [vm], |r| r.get(0))?)
    }).unwrap()
}

#[tokio::test]
async fn ready_commit_waits_out_a_concurrent_dispatch_on_the_gate() {
    let mut h = windows_harness().await;
    let client = h.client(0).await;
    let Starting { owner, worker, registration: _registration, observed } = reserved(&h, &client).await;
    record_checked_creation(&h, &observed);
    // A concurrent dispatch, such as a vm_get poll, holds the gate at the
    // moment the validated boot is ready to commit.
    let supervisor = h.supervisor().clone();
    let (locked, held) = std::sync::mpsc::channel();
    let holder = std::thread::spawn(move || {
        let _gate = supervisor.gate.lock().unwrap();
        locked.send(()).unwrap();
        std::thread::sleep(Duration::from_millis(200));
    });
    held.recv().unwrap();
    let waiting = Instant::now();
    assert!(owner.commit_ready(h.supervisor(), &worker, || Some([42; 16]), || true).await);
    assert!(waiting.elapsed() >= Duration::from_millis(100));
    holder.join().unwrap();
    let args = json!({"vm_id":hex::encode(observed.target.vm)});
    assert_eq!(call(&client, "vm_get", args).await["ready"], true);
    assert_admission_closed(&h);
    drop(client);
    let _ = h.shutdown().await;
}

#[tokio::test]
async fn ready_commit_resamples_a_child_lost_while_the_gate_was_busy() {
    let mut h = windows_harness().await;
    let client = h.client(0).await;
    let Starting { owner, worker, registration: _registration, observed } = reserved(&h, &client).await;
    record_checked_creation(&h, &observed);
    // The final sample passes, then the child exits (or owner custody fails)
    // while a concurrent dispatch holds the gate. Loss lands before release.
    let supervisor = h.supervisor().clone();
    let alive = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let (locked, held) = std::sync::mpsc::channel();
    let holder = {
        let (supervisor, alive) = (supervisor.clone(), alive.clone());
        std::thread::spawn(move || {
            let _gate = supervisor.gate.lock().unwrap();
            locked.send(()).unwrap();
            std::thread::sleep(Duration::from_millis(100));
            alive.store(false, Ordering::SeqCst);
        })
    };
    held.recv().unwrap();
    let samples = std::cell::Cell::new(0);
    let waiting = Instant::now();
    let committed = owner.commit_ready(&supervisor, &worker, || Some([42; 16]), || {
        samples.set(samples.get() + 1);
        alive.load(Ordering::SeqCst)
    }).await;
    let waited = waiting.elapsed();
    holder.join().unwrap();
    assert!(!committed, "a witness was committed for an exited child");
    assert!(samples.get() >= 2, "the final observation was not refreshed after the gate wait");
    assert!(waited < Duration::from_secs(5), "a lost child delayed teardown");
    assert!(!owner.witnessed());
    let target = observed.target;
    supervisor.controller.native_store().transact(|db| {
        let state: i64 = db.query_row("SELECT status FROM vm_operations WHERE operation_id=?1",
            [target.epoch], |r| r.get(0))?;
        assert_eq!(state, Status::Pending.code(), "the start succeeded for an exited child");
        Ok(())
    }).unwrap();
    let status = call(&client, "vm_get", json!({"vm_id":hex::encode(target.vm)})).await;
    assert_eq!(status["ready"], false);
    assert!(status.get("native_boot_witness").is_none());
    assert_admission_closed(&h);
    drop(client);
    let _ = h.shutdown().await;
}

#[tokio::test]
async fn start_behind_a_held_registration_is_busy_without_a_durable_write() {
    let mut h = windows_harness().await;
    let client = h.client(0).await;
    let (vm, _, _, scope) = bound_clone(&h, &client).await;
    let supervisor = h.supervisor();
    // A previous owner committed its clean release and still holds its
    // registration until its worker returns.
    let previous = Target { vm, generation: scope.generation, epoch: [61; 16] };
    let held = supervisor.native_runs
        .register(vm, Arc::new(native_run::Run::new("alpha", previous))).unwrap();
    let released = supervisor.controller.active_release_epoch(vm, false).unwrap();
    assert!(released.is_some());
    let operations = operation_count(&h);
    let busy = supervisor.gate.lock().unwrap().start_native_run(
        supervisor, &Installation::fixture(), start_request(vm, 505), HOST, RESOURCES,
        |_, _, _, _| panic!("a refused start reached its worker"),
    );
    assert_eq!(busy.unwrap_err().0, "busy");
    // No fresh pending operation or writer epoch was committed.
    assert_eq!(operation_count(&h), operations);
    assert_eq!(supervisor.controller.active_release_epoch(vm, false).unwrap(), released);
    assert_eq!(unresolved_epoch(&h, vm), None);

    // Once the previous owner returns, the same fresh request starts and its
    // parked worker owns the slot. A replay of it writes nothing new.
    drop(held);
    let (resume, parked) = std::sync::mpsc::channel::<()>();
    let started = supervisor.gate.lock().unwrap().start_native_run(
        supervisor, &Installation::fixture(), start_request(vm, 505), HOST, RESOURCES,
        move |_, _, _, _| { let _ = parked.recv(); Ok(()) },
    ).unwrap();
    let operations = operation_count(&h);
    assert!(unresolved_epoch(&h, vm).is_some());
    assert!(supervisor.native_runs.with_entry(vm, |owner| owner.is_some()).unwrap());
    let replay = supervisor.gate.lock().unwrap().start_native_run(
        supervisor, &Installation::fixture(), start_request(vm, 505), HOST, RESOURCES,
        |_, _, _, _| panic!("a replay reached a worker"),
    ).unwrap();
    assert_eq!(replay, started);
    assert_eq!(operation_count(&h), operations);
    let fresh = supervisor.gate.lock().unwrap().start_native_run(
        supervisor, &Installation::fixture(), start_request(vm, 506), HOST, RESOURCES,
        |_, _, _, _| panic!("a refused start reached its worker"),
    );
    assert_eq!(fresh.unwrap_err().0, "busy");
    assert_eq!(operation_count(&h), operations);
    resume.send(()).unwrap();
    assert_admission_closed(&h);
    drop(client);
    let _ = h.shutdown().await;
}

#[tokio::test]
async fn reaper_wakes_a_live_owner_and_recovers_only_without_one() {
    let mut h = windows_harness().await;
    let client = h.client(0).await;
    let Starting { owner, registration, observed, .. } = reserved(&h, &client).await;
    let vm = observed.target.vm;
    let supervisor = h.supervisor();
    let mut recovered = 0;
    assert!(!supervisor.reap_native_run(vm, &mut || { recovered += 1; true }));
    assert_eq!(recovered, 0, "the reaper competed with the live owner");
    assert!(owner.retry_signalled().await, "the reaper did not wake the owner's retry");
    // A busy gate is unconfirmed custody for one sweep turn.
    {
        let _gate = supervisor.gate.lock().unwrap();
        assert!(!supervisor.reap_native_run(vm, &mut || { recovered += 1; true }));
    }
    assert_eq!(recovered, 0);
    drop(registration);
    // Without a worker, orphan recovery runs under a fence that excludes a
    // new run owner for the same VM.
    assert!(supervisor.reap_native_run(vm, &mut || {
        recovered += 1;
        supervisor.native_runs.register(vm, owner.clone()).is_err()
    }));
    assert_eq!(recovered, 1);
    let again = supervisor.native_runs.register(vm, owner.clone()).unwrap();
    drop(again);
    assert_admission_closed(&h);
    drop(client);
    let _ = h.shutdown().await;
}

#[tokio::test]
async fn release_settles_without_a_receipt_and_keeps_its_proof_across_store_failure() {
    let mut h = windows_harness().await;
    let client = h.client(0).await;
    let Starting { owner, registration: _registration, observed, .. } = reserved(&h, &client).await;
    let vm = observed.target.vm;
    // Without a stop receipt the exact writer is fenced and the release still
    // commits. A failed commit keeps the same held proof for the next attempt.
    let (observations, commits) = (std::cell::Cell::new(0), std::cell::RefCell::new(Vec::new()));
    let proof = "held proof";
    owner.settle_with(
        h.supervisor(),
        || { observations.set(observations.get() + 1); Ok(proof) },
        |held, stop| {
            assert!(std::ptr::eq(*held, proof));
            commits.borrow_mut().push(stop.is_some());
            if commits.borrow().len() == 1 {
                anyhow::bail!("database is locked");
            }
            Ok(())
        },
    ).await;
    assert_eq!(observations.get(), 2);
    assert_eq!(*commits.borrow(), [false, false]);
    assert_eq!(vm_state(&h, vm), 3, "an unreceipted release must fence its writer");
    assert_admission_closed(&h);
    drop(client);
    let _ = h.shutdown().await;
}

#[tokio::test]
async fn release_after_owner_stop_carries_the_receipt_without_fencing() {
    let mut h = windows_harness().await;
    let client = h.client(0).await;
    let Starting { owner, registration: _registration, observed, .. } = reserved(&h, &client).await;
    let vm = observed.target.vm;
    let args = json!({"vm_id":hex::encode(vm), "request_id":key(504)});
    assert_eq!(call(&client, "vm_stop", args).await["ok"], true);
    let commits = std::cell::RefCell::new(Vec::new());
    owner.settle_with(h.supervisor(), || Ok(()), |_, stop| {
        commits.borrow_mut().push(stop.is_some());
        Ok(())
    }).await;
    assert_eq!(*commits.borrow(), [true]);
    assert_eq!(vm_state(&h, vm), 1, "a receipted stop must not enter recovery");
    assert_admission_closed(&h);
    drop(client);
    let _ = h.shutdown().await;
}

#[tokio::test]
async fn retryable_release_rollback_keeps_the_on_time_stop_and_its_receipt() {
    let mut h = windows_harness().await;
    let client = h.client(0).await;
    let Starting { owner, worker, registration: _registration, observed } = reserved(&h, &client).await;
    record_checked_creation(&h, &observed);
    assert!(owner.commit_ready(h.supervisor(), &worker, || Some([42; 16]), || true).await);
    let vm = observed.target.vm;
    let stop = call(&client, "vm_stop", json!({"vm_id":hex::encode(vm), "request_id":key(507)})).await;
    assert_eq!(stop["ok"], true, "{stop}");
    let ctl = &h.supervisor().controller;
    // A one-shot SQLite rollback of the first release attempt. The failure
    // clears before the owner retries with the same clean proof.
    let trigger = "CREATE TRIGGER fail_release BEFORE UPDATE OF disposition ON vm_snapshot_release
        BEGIN SELECT RAISE(ABORT,'fixture release rollback'); END";
    ctl.native_store().transact(|db| { db.execute_batch(trigger)?; Ok(()) }).unwrap();
    let attempts = std::cell::Cell::new(0);
    owner.settle_with(h.supervisor(), || Ok(()), |_, receipt| {
        attempts.set(attempts.get() + 1);
        assert!(receipt.is_some(), "the owner lost its stop receipt");
        let committed = ctl.native_store().try_native_transaction(|db| {
            native::release::commit(db, &observed, receipt, || ctl.native_clock()).map(|_| ())
        });
        match attempts.get() {
            1 => {
                assert!(committed.is_err(), "the fixture rollback did not fire");
                assert_eq!(vm_state(&h, vm), 2, "the rollback changed the VM state");
                ctl.native_store().transact(|db| {
                    db.execute_batch("DROP TRIGGER fail_release")?;
                    Ok(())
                }).unwrap();
            }
            2 => assert!(committed.is_ok(), "{committed:?}"),
            _ => panic!("the clean release kept failing: {committed:?}"),
        }
        committed
    }).await;
    assert_eq!(attempts.get(), 2);
    // The second attempt released on time: stopped, clean and the original
    // stop receipt succeeded instead of being interrupted by a fence.
    assert_eq!(vm_state(&h, vm), 1, "the retry interrupted the on-time stop");
    assert_eq!(unresolved_epoch(&h, vm), None);
    assert_eq!(ctl.active_release_epoch(vm, false).unwrap(), Some((observed.target.generation, observed.target.epoch)));
    let settled = terminal(&client, &stop).await;
    assert_eq!(settled["state"], "succeeded", "{settled}");
    assert_admission_closed(&h);
    drop(client);
    let _ = h.shutdown().await;
}

#[tokio::test]
async fn teardown_retry_keeps_one_launch_and_is_woken_by_the_reaper() {
    let mut h = windows_harness().await;
    let client = h.client(0).await;
    let Starting { owner, worker, registration: _registration, observed } = reserved(&h, &client).await;
    record_checked_creation(&h, &observed);
    assert!(owner.commit_ready(h.supervisor(), &worker, || Some([42; 16]), || true).await);
    let vm = observed.target.vm;
    let supervisor = h.supervisor();
    let (attempts, settled) = (std::cell::Cell::new(0), std::cell::Cell::new(None));
    let launch = "one owned launch";
    let started = Instant::now();
    owner.settle_live(
        supervisor,
        async || {
            assert!(!owner.witnessed(), "teardown started before the witness was withdrawn");
            attempts.set(attempts.get() + 1);
            if attempts.get() == 1 {
                // An incomplete release: the periodic reaper wakes the owner
                // instead of competing for the same generation.
                assert!(!supervisor.reap_native_run(vm, &mut || panic!("reaper competed")));
                return None;
            }
            Some(launch)
        },
        async |proof| settled.set(Some(proof)),
    ).await;
    assert_eq!(attempts.get(), 2);
    assert!(std::ptr::eq(settled.get().unwrap(), launch));
    assert!(started.elapsed() < Duration::from_secs(1), "retry waited instead of waking");
    let args = json!({"vm_id":hex::encode(vm)});
    let status = call(&client, "vm_get", args).await;
    assert_eq!(status["ready"], false);
    assert!(status.get("native_boot_witness").is_none());
    assert_admission_closed(&h);
    drop(client);
    let _ = h.shutdown().await;
}

#[tokio::test]
async fn reopened_store_has_no_witness_and_reaches_orphan_recovery() {
    let mut h = windows_harness().await;
    let client = h.client(0).await;
    let Starting { owner, worker, registration, observed } = reserved(&h, &client).await;
    record_checked_creation(&h, &observed);
    assert!(owner.commit_ready(h.supervisor(), &worker, || Some([42; 16]), || true).await);
    let target = observed.target;
    let args = json!({"vm_id":hex::encode(target.vm)});
    assert_eq!(call(&client, "vm_get", args.clone()).await["ready"], true);
    // The process ends without settling its release.
    drop((registration, owner, worker, client));
    let _ = h.shutdown().await;
    let mut h = h.reopen(|fixture| fixture.windows_recipe = true).await;
    let client = h.client(0).await;
    let status = call(&client, "vm_get", args).await;
    assert_eq!(status["ready"], false, "{status}");
    assert!(status.get("native_boot_witness").is_none());
    let supervisor = h.supervisor();
    assert!(supervisor.native_runs.with_entry(target.vm, |owner| owner.is_none()).unwrap());
    // The unresolved writer epoch stays durable for offline recovery.
    assert_eq!(unresolved_epoch(&h, target.vm), Some((target.generation, target.epoch)));
    assert!(!supervisor.controller.native_run_current("alpha", target, true).unwrap());
    let mut recovered = false;
    assert!(supervisor.reap_native_run(target.vm, &mut || { recovered = true; true }));
    assert!(recovered);
    assert_admission_closed(&h);
    drop(client);
    let _ = h.shutdown().await;
}

/// The synthetic Job origin of a fixture child's checked creation edge.
fn fixture_origin(scope: native::Scope) -> native::recovery::Origin {
    native::recovery::Origin {
        boot: [34; 32],
        database: FileIdentity { volume: scope.production.volume, file: [35; 16] },
        lock: FileIdentity { volume: scope.production.volume, file: [36; 16] },
    }
}

/// Holds a fixture teardown until the test has observed the projection.
/// Both signals are latched, so neither side can lose the other's wakeup.
#[derive(Default)]
struct TeardownBarrier {
    entered: AtomicBool,
    entered_signal: Notify,
    released: AtomicBool,
    release_signal: Notify,
}

impl TeardownBarrier {
    fn mark_entered(&self) {
        self.entered.store(true, Ordering::SeqCst);
        self.entered_signal.notify_waiters();
    }

    fn release(&self) {
        self.released.store(true, Ordering::SeqCst);
        self.release_signal.notify_waiters();
    }

    fn has_entered(&self) -> bool {
        self.entered.load(Ordering::SeqCst)
    }

    async fn wait_entered(&self) {
        Self::wait(&self.entered_signal, &self.entered).await;
    }

    async fn wait_release(&self) {
        Self::wait(&self.release_signal, &self.released).await;
    }

    async fn wait(signal: &Notify, latched: &AtomicBool) {
        let notified = signal.notified();
        tokio::pin!(notified);
        let _ = notified.as_mut().enable();
        if !latched.load(Ordering::SeqCst) {
            notified.await;
        }
    }
}

/// Unblocks the parked worker, the teardown barrier and both duplex peers on
/// every exit, including a panic, so a failed run can never hang its drop.
struct ComposedCleanup {
    start_gate: Option<std::sync::mpsc::Sender<()>>,
    alive: Arc<AtomicBool>,
    teardown: Arc<TeardownBarrier>,
    supervisor: Arc<Supervisor>,
    peers: [tokio::task::AbortHandle; 2],
}

impl ComposedCleanup {
    fn release_worker(&mut self) {
        if let Some(gate) = self.start_gate.take() {
            let _ = gate.send(());
        }
    }
}

impl Drop for ComposedCleanup {
    fn drop(&mut self) {
        drop(self.start_gate.take());
        self.alive.store(false, Ordering::SeqCst);
        for peer in &self.peers {
            peer.abort();
        }
        self.teardown.release();
        if std::thread::panicking() {
            // Wakes the ping loop and any bounded retry at once.
            self.supervisor.fence();
        }
    }
}

/// A known fixture child: a liveness flag stands in for the process and its
/// Job, and duplex QMP and guest peers answer like the real endpoints.
struct FixtureLaunch {
    alive: Arc<AtomicBool>,
    booted: Arc<AtomicBool>,
    pings: Arc<AtomicUsize>,
    qmp: BufReader<DuplexStream>,
    guest: BufReader<DuplexStream>,
    teardown: Arc<TeardownBarrier>,
    proof: Option<Observed>,
}

const QMP_CONT: &str = r#"{"execute":"cont"}"#;
const QMP_RETURN: &str = r#"{"return":{}}"#;

async fn fixture_exchange(
    stream: &mut BufReader<DuplexStream>,
    send: Option<&str>,
    expect: &str,
) -> Result<(), NativeRefusal> {
    let refused = NativeRefusal::GuestEndpointUnverified;
    if let Some(line) = send {
        stream.write_all(format!("{line}\n").as_bytes()).await.map_err(|_| refused)?;
    }
    let mut line = String::new();
    match stream.read_line(&mut line).await {
        Ok(read) if read > 0 && line.trim() == expect => Ok(()),
        _ => Err(refused),
    }
}

impl LiveLaunch for FixtureLaunch {
    type Proof = Observed;

    async fn boot(
        &mut self,
        cancel: &Shutdown,
        _boot_by: Instant,
        mut refresh: impl FnMut() -> Result<Instant, NativeRefusal>,
    ) -> Result<(), NativeRefusal> {
        refresh()?;
        fixture_exchange(&mut self.qmp, Some(QMP_CONT), QMP_RETURN).await?;
        fixture_exchange(&mut self.guest, None, "READY").await?;
        if cancel.is_cancelled() {
            return Err(NativeRefusal::GuestEndpointUnverified);
        }
        refresh()?;
        self.booted.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn check_live(
        &self,
        refresh: impl FnOnce() -> Result<Instant, NativeRefusal>,
    ) -> Result<(), NativeRefusal> {
        if !self.alive.load(Ordering::SeqCst) {
            return Err(NativeRefusal::JobUnverified);
        }
        refresh().map(|_| ())
    }

    async fn ping(
        &mut self,
        _cancel: &Shutdown,
        refresh: impl FnMut() -> Result<Instant, NativeRefusal>,
    ) -> Result<(), NativeRefusal> {
        fixture_exchange(&mut self.guest, Some("PING"), "PONG").await?;
        self.check_live(refresh)?;
        self.pings.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn stop(&mut self) -> Option<Observed> {
        self.teardown.mark_entered();
        self.teardown.wait_release().await;
        self.proof.take()
    }
}

async fn qmp_peer(stream: DuplexStream) {
    let mut stream = BufReader::new(stream);
    let mut line = String::new();
    while stream.read_line(&mut line).await.is_ok_and(|read| read > 0) {
        if line.trim() == QMP_CONT
            && stream.write_all(format!("{QMP_RETURN}\n").as_bytes()).await.is_err()
        {
            return;
        }
        line.clear();
    }
}

/// Reports ready, answers exactly one ping, then holds the channel silent
/// until it is aborted. A second ping parks without sampling the store, so
/// the test's own reads cannot contend with the live monitor.
async fn guest_peer(stream: DuplexStream) {
    let mut stream = BufReader::new(stream);
    if stream.write_all(b"READY\n").await.is_err() {
        return;
    }
    let mut line = String::new();
    if stream.read_line(&mut line).await.is_ok_and(|read| read > 0)
        && line.trim() == "PING"
        && stream.write_all(b"PONG\n").await.is_ok()
    {
        std::future::pending::<()>().await;
    }
}

async fn eventually(what: &str, mut done: impl FnMut() -> bool) {
    let waited = tokio::time::timeout(Duration::from_secs(20), async {
        while !done() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }).await;
    assert!(waited.is_ok(), "timed out waiting for {what}");
}

#[tokio::test]
async fn composed_start_replays_publishes_after_ping_and_withdraws_on_loss() {
    let mut h = windows_harness().await;
    let client = h.client(0).await;
    let (vm, slots, pins, scope) = bound_clone(&h, &client).await;
    let supervisor = h.supervisor().clone();
    let (launch_qmp, peer_qmp) = tokio::io::duplex(1024);
    let (launch_guest, peer_guest) = tokio::io::duplex(1024);
    let qmp = tokio::spawn(qmp_peer(peer_qmp));
    let guest = tokio::spawn(guest_peer(peer_guest));
    let alive = Arc::new(AtomicBool::new(true));
    let (booted, pings) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicUsize::new(0)));
    let teardown = Arc::new(TeardownBarrier::default());
    let (release_worker, worker_gate) = std::sync::mpsc::channel::<()>();
    let mut cleanup = ComposedCleanup {
        start_gate: Some(release_worker),
        alive: alive.clone(),
        teardown: teardown.clone(),
        supervisor: supervisor.clone(),
        peers: [qmp.abort_handle(), guest.abort_handle()],
    };
    let launch = FixtureLaunch {
        alive: alive.clone(),
        booted: booted.clone(),
        pings: pings.clone(),
        qmp: BufReader::new(launch_qmp),
        guest: BufReader::new(launch_guest),
        teardown: teardown.clone(),
        proof: None,
    };
    let finished = Arc::new(AtomicBool::new(false));
    let worker_finished = finished.clone();
    let started = supervisor.gate.lock().unwrap().start_native_run(
        &supervisor, &Installation::fixture(), start_request(vm, 508), HOST, RESOURCES,
        move |supervisor, runtime, owner, start| {
            // The worker owns its slot but waits until the replay is checked,
            // so the test's store reads cannot race the live monitor.
            if worker_gate.recv().is_err() {
                return Ok(());
            }
            let target = owner.target();
            // The fixture child crosses the checked creation edge once.
            supervisor.controller.native_store().transact(|db| {
                native::recovery::record_job(db, scope, target.epoch, fixture_origin(scope))
            })?;
            let mut launch = launch;
            launch.proof = Some(Observed { target, scope, slots, pins, clean: true });
            runtime.block_on(owner.live(supervisor, &start.worker, &mut launch, || false,
                async |proof: Observed| {
                    owner.settle_with(supervisor, || Ok(()), |_, receipt| {
                        supervisor.controller.native_store().try_native_transaction(|db| {
                            native::release::commit(db, &proof, receipt,
                                || supervisor.controller.native_clock()).map(|_| ())
                        })
                    }).await;
                }));
            worker_finished.store(true, Ordering::SeqCst);
            Ok(())
        },
    ).unwrap();
    let epoch = parse_id(started["operation_id"].as_str().unwrap()).unwrap();
    let operations = operation_count(&h);
    // One replay of the exact request while its worker owns the slot.
    let replay = supervisor.gate.lock().unwrap().start_native_run(
        &supervisor, &Installation::fixture(), start_request(vm, 508), HOST, RESOURCES,
        |_, _, _, _| panic!("a replay reached a worker"),
    ).unwrap();
    assert_eq!(replay, started);
    assert_eq!(operation_count(&h), operations);
    cleanup.release_worker();

    // Ready after the validated handshake and one ping. The wait reads only
    // atomics while boot, commit and the ping sample the store.
    eventually("a successful ping", || {
        if pings.load(Ordering::SeqCst) >= 1 {
            return true;
        }
        assert!(
            !teardown.has_entered() && !finished.load(Ordering::SeqCst),
            "the worker entered teardown before its first ping (booted={})",
            booted.load(Ordering::SeqCst),
        );
        false
    }).await;
    assert!(booted.load(Ordering::SeqCst));
    assert!(!teardown.has_entered());
    // The guest now stays silent, so authenticated reads cannot race a ping.
    let args = json!({"vm_id":hex::encode(vm)});
    let status = call(&client, "vm_get", args.clone()).await;
    assert_eq!(status["state"], "running", "{status}");
    assert_eq!(status["ready"], true, "{status}");
    let witness = &status["native_boot_witness"];
    assert_eq!(witness["source"], "guest_ready_handshake");
    assert_eq!(witness["vm_id"], hex::encode(vm));
    assert_eq!(witness["generation"], hex::encode(scope.generation));
    assert_eq!(witness["operation_id"], hex::encode(epoch));
    let witness_id = parse_id(witness["witness_id"].as_str().unwrap()).unwrap();
    assert_ne!(witness_id, epoch);
    assert_ne!(witness_id, [0; 16]);
    let start = call(&client, "vm_operation", json!({"operation_id":hex::encode(epoch)})).await;
    assert_eq!(start["state"], "succeeded", "{start}");

    // The child is lost while teardown is blocked: the read stays unready.
    alive.store(false, Ordering::SeqCst);
    guest.abort();
    let entered = tokio::time::timeout(Duration::from_secs(20), teardown.wait_entered()).await;
    assert!(entered.is_ok(), "teardown never started after the child was lost");
    let lost = call(&client, "vm_get", args.clone()).await;
    assert_eq!(lost["ready"], false, "{lost}");
    assert!(lost.get("native_boot_witness").is_none());
    assert!(!finished.load(Ordering::SeqCst));
    assert_eq!(unresolved_epoch(&h, vm), Some((scope.generation, epoch)));
    teardown.release();

    // The real release commits on the owner's proof, and the slot is freed.
    eventually("the worker to finish", || finished.load(Ordering::SeqCst)).await;
    eventually("the run slot to be released", || {
        supervisor.native_runs.with_entry(vm, |owner| owner.is_none()).unwrap_or(false)
    }).await;
    assert_eq!(unresolved_epoch(&h, vm), None);
    assert_eq!(
        supervisor.controller.active_release_epoch(vm, false).unwrap(),
        Some((scope.generation, epoch))
    );
    assert_eq!(vm_state(&h, vm), 3, "an unreceipted release hands the VM to recovery");
    let released = call(&client, "vm_get", args).await;
    assert_eq!(released["ready"], false);
    assert!(released.get("native_boot_witness").is_none());
    drop(cleanup);
    assert_admission_closed(&h);
    drop((supervisor, client));
    let _ = h.shutdown().await;
}

#[tokio::test]
async fn composed_start_restarted_before_creation_reaches_the_real_recovery_commit() {
    let mut h = windows_harness().await;
    let client = h.client(0).await;
    let (vm, slots, pins, scope) = bound_clone(&h, &client).await;
    let supervisor = h.supervisor().clone();
    let (resume, parked) = std::sync::mpsc::channel::<()>();
    let started = supervisor.gate.lock().unwrap().start_native_run(
        &supervisor, &Installation::fixture(), start_request(vm, 509), HOST, RESOURCES,
        move |_, _, _, _| {
            // The process ends after admission, before the creation edge.
            let _ = parked.recv();
            Ok(())
        },
    ).unwrap();
    let epoch = parse_id(started["operation_id"].as_str().unwrap()).unwrap();
    let target = Target { vm, generation: scope.generation, epoch };
    assert_eq!(writer_epoch(&h, scope.generation), (epoch, false));
    resume.send(()).unwrap();
    eventually("the run slot to be released", || {
        supervisor.native_runs.with_entry(vm, |owner| owner.is_none()).unwrap_or(false)
    }).await;
    drop((supervisor, client));
    let _ = h.shutdown().await;

    let mut h = h.reopen(|fixture| fixture.windows_recipe = true).await;
    let client = h.client(0).await;
    let reopened = call(&client, "vm_get", json!({"vm_id":hex::encode(vm)})).await;
    assert_eq!(reopened["ready"], false, "{reopened}");
    assert!(reopened.get("native_boot_witness").is_none());
    let supervisor = h.supervisor();
    assert_eq!(vm_state(&h, vm), 3, "startup recovery did not claim the VM");
    assert_eq!(unresolved_epoch(&h, vm), Some((scope.generation, epoch)));
    let store = supervisor.controller.native_store();
    let cleanup: i64 = store.transact(|db| {
        Ok(db.query_row("SELECT cleanup_id FROM vm_instances WHERE id=?1", [vm], |r| r.get(0))?)
    }).unwrap();
    let kind = crate::vm::reaper::CleanupKind::Recovery { vm, activity_id: cleanup };
    let origin = fixture_origin(scope);
    let observed = Observed { target, scope, slots, pins, clean: true };
    let mut created = None;
    assert!(supervisor.reap_native_run(vm, &mut || {
        store.try_native_transaction(|db| {
            let expected = native::recovery::orphan(db, kind, origin)?
                .ok_or_else(|| anyhow::anyhow!("no exact orphan"))?;
            created = Some(expected.created);
            native::recovery::commit(db, kind, origin, expected, &observed,
                || supervisor.controller.native_clock())
        }).is_ok()
    }));
    assert_eq!(created, Some(false));
    assert_eq!(unresolved_epoch(&h, vm), None);
    assert_eq!(
        supervisor.controller.active_release_epoch(vm, false).unwrap(),
        Some((scope.generation, epoch))
    );
    assert_admission_closed(&h);
    drop(client);
    let _ = h.shutdown().await;
}
