use super::*;
use crate::{
    store::vm::{
        self as store,
        native::{self, release::{Observed, Target}},
    },
    vm::{paths::FileIdentity, test_issuer::Installation},
};
use rusqlite::{OptionalExtension, params};

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
