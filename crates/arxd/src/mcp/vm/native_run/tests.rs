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

fn retry_high_water(h: &Harness) -> i64 {
    h.supervisor().controller.native_store().transact(|db| {
        Ok(db.query_row("SELECT high_water_ms FROM vm_retry_clock", [], |r| r.get(0))?)
    }).unwrap()
}

fn operation_exists(h: &Harness, operation: Id) -> bool {
    h.supervisor().controller.native_store().transact(|db| {
        Ok(db.query_row(
            "SELECT count(*) FROM vm_operations WHERE operation_id=?1",
            [operation], |r| r.get::<_, i64>(0),
        )? == 1)
    }).unwrap()
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
    // parked worker owns the slot.
    drop(held);
    let (resume, parked) = std::sync::mpsc::channel::<()>();
    let started = supervisor.gate.lock().unwrap().start_native_run(
        supervisor, &Installation::fixture(), start_request(vm, 505), HOST, RESOURCES,
        move |_, _, _, _| { let _ = parked.recv(); Ok(()) },
    ).unwrap();
    assert!(unresolved_epoch(&h, vm).is_some());
    assert!(supervisor.native_runs.with_entry(vm, |owner| owner.is_some()).unwrap());

    // Insert an unrelated expired terminal operation and advance fake UTC
    // within the replay window. Occupied-slot replay and refusals must make
    // zero durable writes (no retry-clock advance and no expired-row pruning).
    let expired_op: Id = [91; 16];
    let expired_key: Id = [92; 16];
    let unrelated_vm: Id = [93; 16];
    supervisor.controller.native_store().transact(|db| {
        db.execute(
            "INSERT INTO activity(principal,vm,action,started_ms,finished_ms,terminal_order,status,actor)
             VALUES('beta',?1,3,?2,?2,(SELECT coalesce(max(terminal_order),0)+1 FROM activity),1,'agent')",
            params![unrelated_vm, UTC - 10_000],
        )?;
        let activity = db.last_insert_rowid();
        db.execute(
            "INSERT INTO vm_operations(principal,request_key,operation_id,vm,action,activity_id,
                valid_through_ms,status,binding_kind,subject_id)
             VALUES('beta',?1,?2,?3,3,?4,?5,1,'vm',?3)",
            params![expired_key, expired_op, unrelated_vm, activity, UTC - 1],
        )?;
        Ok(())
    }).unwrap();
    let operations = operation_count(&h);
    let high_water_before = retry_high_water(&h);
    h.now.store(UTC + 5_000, Ordering::SeqCst);

    // Exact replay returns the old receipt with zero durable writes.
    let replay = supervisor.gate.lock().unwrap().start_native_run(
        supervisor, &Installation::fixture(), start_request(vm, 505), HOST, RESOURCES,
        |_, _, _, _| panic!("a replay reached a worker"),
    ).unwrap();
    assert_eq!(replay, started);
    assert_eq!(retry_high_water(&h), high_water_before, "occupied-slot replay advanced vm_retry_clock");
    assert_eq!(operation_count(&h), operations);
    assert!(operation_exists(&h, expired_op), "occupied-slot replay pruned an expired operation");

    // Key collision (reusing the vm_create key 501) and stale key are refused
    // without advancing the retry clock or pruning unrelated rows.
    let collision = supervisor.gate.lock().unwrap().start_native_run(
        supervisor, &Installation::fixture(), start_request(vm, 501), HOST, RESOURCES,
        |_, _, _, _| panic!("a colliding key reached a worker"),
    );
    assert_eq!(collision.unwrap_err().0, "key_collision");
    assert_eq!(retry_high_water(&h), high_water_before);
    assert!(operation_exists(&h, expired_op));

    let stale_req = Request {
        principal: "alpha",
        vm,
        request_id: RequestId::parse(&format!("{:016x}{:016x}", 1_u64, 599_u64)).unwrap(),
        operation: Operation::Start,
        disk: DiskClaim::default(),
    };
    let stale = supervisor.gate.lock().unwrap().start_native_run(
        supervisor, &Installation::fixture(), stale_req, HOST, RESOURCES,
        |_, _, _, _| panic!("a stale key reached a worker"),
    );
    assert_eq!(stale.unwrap_err().0, "stale_key");
    assert_eq!(retry_high_water(&h), high_water_before);
    assert!(operation_exists(&h, expired_op));

    // Fresh valid key returns Busy while keeping both high_water_ms and the
    // unrelated expired terminal operation row intact.
    let fresh = supervisor.gate.lock().unwrap().start_native_run(
        supervisor, &Installation::fixture(), start_request(vm, 506), HOST, RESOURCES,
        |_, _, _, _| panic!("a refused start reached its worker"),
    );
    assert_eq!(fresh.unwrap_err().0, "busy");
    assert_eq!(retry_high_water(&h), high_water_before, "occupied-slot busy advanced vm_retry_clock");
    assert_eq!(operation_count(&h), operations);
    assert!(operation_exists(&h, expired_op), "occupied-slot busy pruned an expired operation");

    resume.send(()).unwrap();
    assert_admission_closed(&h);
    drop(client);
    let _ = h.shutdown().await;
}

fn cleanup_id(h: &Harness, vm: Id) -> Option<i64> {
    h.supervisor()
        .controller
        .native_store()
        .transact(|db| {
            Ok(db.query_row(
                "SELECT cleanup_id FROM vm_instances WHERE id=?1",
                [vm],
                |r| r.get(0),
            )?)
        })
        .unwrap()
}

fn operation_status(h: &Harness, operation: Id) -> i64 {
    h.supervisor()
        .controller
        .native_store()
        .transact(|db| {
            Ok(db.query_row(
                "SELECT status FROM vm_operations WHERE operation_id=?1",
                [operation],
                |r| r.get(0),
            )?)
        })
        .unwrap()
}

#[tokio::test]
async fn pre_entry_runtime_init_error_fences_epoch_and_releases_registration() {
    let mut h = windows_harness().await;
    let client = h.client(0).await;
    let (vm, _, _, scope) = bound_clone(&h, &client).await;
    let supervisor = h.supervisor().clone();
    let entered = Arc::new(AtomicBool::new(false));
    let worker_entered = entered.clone();
    let started = supervisor
        .gate
        .lock()
        .unwrap()
        .start_native_run_with(
            &supervisor,
            &Installation::fixture(),
            start_request(vm, 510),
            HOST,
            RESOURCES,
            || Err(std::io::Error::other("injected pre-entry error")),
            || {},
            move |_, _, _, _| {
                worker_entered.store(true, Ordering::SeqCst);
                Ok(())
            },
        )
        .unwrap();
    let epoch = parse_id(started["operation_id"].as_str().unwrap()).unwrap();
    eventually("the run slot to be released", || {
        supervisor
            .native_runs
            .with_entry(vm, |owner| owner.is_none())
            .unwrap_or(false)
    })
    .await;
    assert!(!entered.load(Ordering::SeqCst), "work closure ran despite pre-entry init failure");
    assert_eq!(
        vm_state(&h, vm),
        3,
        "pre-entry runtime init error stranded the VM without fencing to recovery"
    );
    assert!(
        cleanup_id(&h, vm).is_some(),
        "pre-entry runtime init error left no recovery cleanup claim"
    );
    assert_eq!(
        operation_status(&h, epoch),
        Status::Interrupted.code(),
        "pre-entry runtime init error left the start operation pending"
    );
    assert_eq!(unresolved_epoch(&h, vm), Some((scope.generation, epoch)));
    assert_admission_closed(&h);
    drop((supervisor, client));
    let _ = h.shutdown().await;
}

#[tokio::test]
async fn enqueue_refusal_with_transient_fence_failure_establishes_recovery_claim_promptly() {
    let mut h = windows_harness().await;
    let client = h.client(0).await;
    let (vm, _, _, scope) = bound_clone(&h, &client).await;
    let supervisor = h.supervisor().clone();
    let fault_supervisor = supervisor.clone();
    let refused = supervisor.gate.lock().unwrap().start_native_run_with(
        &supervisor,
        &Installation::fixture(),
        start_request(vm, 511),
        HOST,
        RESOURCES,
        || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
        },
        move || {
            fault_supervisor
                .controller
                .native_store()
                .transact(|db| {
                    db.execute_batch(
                        "CREATE TRIGGER fail_pre_entry_fence BEFORE UPDATE OF state ON vm_instances
                         WHEN NEW.state=3
                         BEGIN SELECT RAISE(ABORT,'injected transient fence failure'); END;",
                    )?;
                    Ok(())
                })
                .unwrap();
            fault_supervisor.fence();
        },
        |_, _, _, _| panic!("a refused start reached its worker"),
    );
    assert_eq!(refused.unwrap_err().0, "shutdown");
    assert!(
        supervisor
            .native_runs
            .with_entry(vm, |owner| owner.is_none())
            .unwrap(),
        "enqueue refusal leaked its run registration"
    );
    let Some((generation, epoch)) = unresolved_epoch(&h, vm) else {
        panic!("expected committed unresolved start epoch after post-admit refusal");
    };
    assert_eq!(generation, scope.generation);
    assert_eq!(vm_state(&h, vm), 1, "immediate fence trigger did not block the first fence attempt");
    assert_eq!(cleanup_id(&h, vm), None);
    assert_eq!(operation_status(&h, epoch), Status::Pending.code());

    // Clear the transient fence write failure; the prompt bounded retry must
    // establish the recovery fence and claim without waiting for restart/expiry.
    supervisor
        .controller
        .native_store()
        .transact(|db| {
            db.execute_batch("DROP TRIGGER fail_pre_entry_fence")?;
            Ok(())
        })
        .unwrap();
    let fenced = tokio::time::timeout(Duration::from_secs(2), async {
        while vm_state(&h, vm) != 3 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    assert!(
        fenced.is_ok(),
        "bounded recovery retry failed to fence the epoch after transient write failure cleared"
    );
    assert_eq!(vm_state(&h, vm), 3);
    assert!(cleanup_id(&h, vm).is_some(), "recovery retry left no cleanup claim");
    assert_eq!(
        operation_status(&h, epoch),
        Status::Interrupted.code(),
        "recovery retry left the start operation pending"
    );
    assert_eq!(unresolved_epoch(&h, vm), Some((scope.generation, epoch)));
    assert!(
        supervisor
            .native_runs
            .with_entry(vm, |owner| owner.is_none())
            .unwrap()
    );
    assert_admission_closed(&h);
    drop((supervisor, client));
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

struct ReleaseBarrierOnDrop(Arc<native_run::BlockingBarrier>);

impl Drop for ReleaseBarrierOnDrop {
    fn drop(&mut self) {
        self.0.release();
    }
}

#[tokio::test]
async fn retry_fence_serialized_with_concurrent_stop_admission_preserves_on_time_stop() {
    let mut h = windows_harness().await;
    let client = h.client(0).await;
    let Starting {
        owner,
        worker,
        registration: _registration,
        observed,
    } = reserved(&h, &client).await;
    record_checked_creation(&h, &observed);
    assert!(
        owner
            .commit_ready(h.supervisor(), &worker, || Some([42; 16]), || true)
            .await
    );
    let vm = observed.target.vm;
    let ctl = &h.supervisor().controller;
    let barrier = native_run::BlockingBarrier::new();
    let _release_guard = ReleaseBarrierOnDrop(barrier.clone());
    owner.arm_stop_handoff_barrier(barrier.clone());
    let after_retry_fence = owner.arm_after_retry_fence();

    let attempts = std::cell::Cell::new(0);
    let stop = {
        let stop_fut = call(
            &client,
            "vm_stop",
            json!({"vm_id": hex::encode(vm), "request_id": key(512)}),
        );
        let settle_fut = owner.settle_with(
            h.supervisor(),
            || {
                let attempt = attempts.get() + 1;
                attempts.set(attempt);
                if attempt == 1 {
                    anyhow::bail!("injected transient release observation failure");
                }
                Ok(())
            },
            |_, receipt| {
                assert!(receipt.is_some(), "the owner did not receive the concurrently admitted stop");
                ctl.native_store().try_native_transaction(|db| {
                    native::release::commit(db, &observed, receipt, || ctl.native_clock())
                        .map(|_| ())
                })
            },
        );
        tokio::pin!(stop_fut);
        tokio::pin!(settle_fut);
        // Pause stop admission under supervisor.gate right after SQLite records
        // the pending stop receipt and before owner.stop(stop) installs it.
        tokio::select! {
            res = &mut stop_fut => panic!("vm_stop completed before barrier release: {res}"),
            () = barrier.wait_entered() => {}
        }
        // Drive the owner through one transient failure and past wait_retry's
        // conditional fence check while stop admission still holds the gate.
        tokio::select! {
            () = &mut settle_fut => panic!("settle_with completed before barrier release"),
            res = &mut stop_fut => panic!("vm_stop completed before barrier release: {res}"),
            res = after_retry_fence => res.unwrap(),
        }
        // Release stop admission so it installs StopReceipt, wakes the retrying
        // owner, and returns its pending receipt; then allow the clean release
        // to commit before the stop deadline.
        barrier.release();
        let (stop, ()) = tokio::join!(stop_fut, settle_fut);
        stop
    };

    assert_eq!(stop["ok"], true, "{stop}");
    assert_eq!(attempts.get(), 2);
    assert_eq!(
        vm_state(&h, vm),
        1,
        "retry fencing moved the VM to recovery instead of preserving the on-time stop"
    );
    assert_eq!(unresolved_epoch(&h, vm), None);
    assert_eq!(
        ctl.active_release_epoch(vm, false).unwrap(),
        Some((observed.target.generation, observed.target.epoch))
    );
    let settled = terminal(&client, &stop).await;
    assert_eq!(
        settled["state"], "succeeded",
        "retry fencing interrupted the concurrently admitted on-time stop: {settled}"
    );
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

    /// The line fixture has no script channel.
    async fn run_script(
        &mut self,
        _authority: crate::vm::runtime::ScriptAuthority,
        _script: String,
        _timeout_s: u32,
        _deadline: Instant,
        _cancel: &Shutdown,
        _refresh: impl FnMut() -> Result<Instant, NativeRefusal>,
    ) -> Result<(arx_guest::ScriptStatus, Option<u32>), NativeRefusal> {
        Err(NativeRefusal::GuestEndpointUnverified)
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
            let revoked = AtomicBool::new(false);
            let authorize = FixtureAuthorizer {
                controller: &supervisor.controller,
                revoked: &revoked,
            };
            runtime.block_on(owner.live(supervisor, &start.worker, &mut launch, &authorize, || false,
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

// ---- 7c 14: scripts on the owned channel, cancelled by stop ----

/// Script bytes that must never reach the ledger, a receipt or a read.
const CANARY: &str = "canary-7c14-0f3a";
const BOOT_MS: u64 = 3_600_000;
const SELECTION_SEAL: crate::vm::qualification_record::Hash = [0xa1; 32];
const SELECTION_SCOPE: crate::vm::qualification_record::Hash = [0xb2; 32];
const SELECTION_OWNER: &str = "S-1-12-1-1-2-3-4";

/// A held TEST selection over the harness clock. The production issuer
/// body mints and consumes every script permit against the real ledger.
struct FixtureSelection<'a> {
    origin: Instant,
    revoked: &'a AtomicBool,
}

impl crate::vm::test_issuer::SelectedSource for FixtureSelection<'_> {
    fn seal_sha256(&self) -> crate::vm::qualification_record::Hash {
        SELECTION_SEAL
    }

    fn class(&self) -> crate::vm::qualification_record::Class {
        crate::vm::qualification_record::Class::Test
    }

    fn scope(&self) -> crate::vm::qualification_record::Hash {
        SELECTION_SCOPE
    }

    fn recipes(&self) -> crate::vm::qualification_record::TestRecipes {
        use crate::vm::qualification_record::RecipeIdentity;
        crate::vm::qualification_record::TestRecipes {
            ubuntu: RecipeIdentity { recipe_id: [0x11; 16], recipe_sha256: [1; 32] },
            windows: RecipeIdentity {
                recipe_id: Controller::windows_recipe_id(),
                recipe_sha256: [0x22; 32],
            },
        }
    }

    fn owner_matches(&self, configured: &str) -> bool {
        configured == SELECTION_OWNER
    }

    fn sample(
        &self,
        advance: &dyn Fn() -> Result<ClockReading, crate::vm::selection_epoch::Invalidation>,
    ) -> Result<crate::vm::selection_epoch::Sample, crate::vm::selection_epoch::Invalidation> {
        if self.revoked.load(Ordering::SeqCst) {
            return Err(crate::vm::selection_epoch::Invalidation::Confirmed(
                NativeRefusal::QualificationMissing,
            ));
        }
        let captured = Instant::now();
        let reading = advance()?;
        let elapsed = reading.monotonic.saturating_duration_since(self.origin).as_millis();
        Ok(crate::vm::selection_epoch::Sample {
            captured,
            now: crate::vm::qualification_record::Instant2 {
                utc: u64::try_from(reading.utc_ms / 1_000).unwrap(),
                mono_ms: BOOT_MS + u64::try_from(elapsed).unwrap(),
            },
            boot_marker: [7; 32],
            reading,
        })
    }
}

fn selection_config() -> crate::config::TestQualificationSelection {
    let profile = json!({
        "enabled": true, "owner_sid": SELECTION_OWNER, "supervisor_sid": SELECTION_OWNER,
        "bundle": "E:/pinned bundle", "volume": "D:/", "state": "D:/test_vms/state",
        "disk_ceiling_bytes": 10_737_418_240_u64, "manifest": "qemu-11.1.3-windows-x64",
        "execution_profile": "current_user",
        "test_qualification": {
            "enrollment": "D:/private/enrollment.json", "dossier": "D:/private/dossier",
            "sha256": hex::encode(SELECTION_SEAL), "scope": hex::encode(SELECTION_SCOPE),
        },
    });
    crate::config::VmRuntimeProfile::parse(&serde_json::to_vec(&profile).unwrap())
        .unwrap()
        .test_qualification()
        .unwrap()
}

/// Opens a fresh selection epoch per dispatch, because the harness UTC
/// clock stands still, and delegates to the production issuer authorizer.
struct FixtureAuthorizer<'a> {
    controller: &'a Controller,
    revoked: &'a AtomicBool,
}

impl native_run::ScriptAuthorizer for FixtureAuthorizer<'_> {
    fn authorize(
        &self,
        worker: &PendingOperation,
        target: crate::vm::test_issuer::NativeTarget,
        script: &str,
        timeout_s: u32,
        cancel: &Shutdown,
    ) -> Result<crate::vm::runtime::ScriptAuthority, NativeRefusal> {
        let issuer = crate::vm::test_issuer::TestIssuer::open(
            &Installation::fixture(),
            self.controller,
            FixtureSelection { origin: Instant::now(), revoked: self.revoked },
            &selection_config(),
        )?;
        issuer.authorize(worker, target, script, timeout_s, cancel)
    }
}

/// What the duplex guest service saw and did.
#[derive(Default)]
struct GuestControl {
    scripts: Mutex<Vec<(String, u32)>>,
    challenges: Mutex<Vec<String>>,
    /// A script ending in " hold" parks here until released.
    held: TeardownBarrier,
    replied: AtomicBool,
    /// While set, each periodic ping parks until released or cancelled.
    hold_pings: AtomicBool,
    ping_hold: TeardownBarrier,
}

impl GuestControl {
    fn received(&self) -> Vec<(String, u32)> {
        self.scripts.lock().unwrap().clone()
    }
}

/// The real guest wire protocol over a duplex pipe. The script's last word
/// picks the reply: `exit 7`, `timeout`, `hold`, `drop` (close the channel),
/// `garbage` (a malformed frame), anything else exits 0.
async fn script_guest(mut stream: DuplexStream, control: Arc<GuestControl>) {
    use arx_guest::{Request, Response, ScriptExecution, ScriptStatus};
    while let Ok(length) = stream.read_u32().await {
        if length as usize > arx_guest::MAX_FRAME {
            return;
        }
        let mut payload = vec![0; length as usize + 4];
        payload[..4].copy_from_slice(&length.to_be_bytes());
        if stream.read_exact(&mut payload[4..]).await.is_err() {
            return;
        }
        let Ok(Some(request)) = arx_guest::read_frame::<Request>(&mut payload.as_slice()) else {
            return;
        };
        let response = match request {
            Request::Hello { version, challenge } => {
                control.challenges.lock().unwrap().push(challenge.clone());
                Response::HelloAck { version, challenge, script_execution: ScriptExecution::Available }
            }
            Request::Ping { id } => Response::Pong { id },
            Request::RunScript { id, script, timeout_s } => {
                control.scripts.lock().unwrap().push((script.clone(), timeout_s));
                let (status, exit_code) = match script.rsplit(' ').next() {
                    Some("hold") => {
                        control.held.mark_entered();
                        control.held.wait_release().await;
                        (ScriptStatus::Exited, Some(0))
                    }
                    Some("7") => (ScriptStatus::Exited, Some(7)),
                    Some("timeout") => (ScriptStatus::TimedOut, None),
                    Some("drop") => return,
                    Some("garbage") => {
                        let junk = br#"{"not":"a guest response"}"#;
                        let mut frame = u32::try_from(junk.len()).unwrap().to_be_bytes().to_vec();
                        frame.extend_from_slice(junk);
                        let _ = stream.write_all(&frame).await;
                        std::future::pending::<()>().await;
                        return;
                    }
                    _ => (ScriptStatus::Exited, Some(0)),
                };
                control.replied.store(true, Ordering::SeqCst);
                Response::ScriptResult { id, status, exit_code }
            }
        };
        let mut frame = Vec::new();
        if arx_guest::write_frame(&mut frame, &response).is_err()
            || stream.write_all(&frame).await.is_err()
        {
            return;
        }
    }
}

/// Starts the guest port the real readiness handshake waits for.
async fn script_qmp(stream: DuplexStream) {
    let [major, minor, micro] = crate::vm::qemu_args::QEMU_VERSION
        .split('.')
        .map(|part| part.parse::<u32>().unwrap())
        .collect::<Vec<_>>()[..]
    else {
        unreachable!("pinned QEMU version is major.minor.micro");
    };
    let mut peer = BufReader::new(stream);
    let greeting = json!({"QMP":{"version":{"qemu":{"major":major,"minor":minor,"micro":micro},"package":""},"capabilities":[]}});
    if peer.get_mut().write_all(format!("{greeting}\n").as_bytes()).await.is_err() {
        return;
    }
    let mut line = String::new();
    while peer.read_line(&mut line).await.is_ok_and(|read| read > 0) {
        let Ok(command) = serde_json::from_str::<Value>(&line) else { return };
        line.clear();
        let result = match command["execute"].as_str() {
            Some("query-status") => json!({"status":"prelaunch","running":false}),
            Some(_) => json!({}),
            None => return,
        };
        if send(&mut peer, json!({"return":result,"id":command["id"]})).await.is_err() {
            return;
        }
        if command["execute"] == "cont" {
            let open = json!({"event":"VSERPORT_CHANGE","data":{"id":crate::vm::qemu_args::GUEST_PORT_ID,"open":true}});
            if send(&mut peer, open).await.is_err() {
                return;
            }
        }
    }
}

/// A known fixture child whose guest channel passed the real readiness
/// handshake. Its script exchange observes neither cancellation nor the
/// worker deadline, as if blocked in pipe I/O, so only the owner's own
/// bounds can end it early.
/// After the first verified ping, periodic pings check liveness only, so
/// the test's authenticated reads never contend with a monitor sample.
struct ScriptLaunch {
    alive: Arc<AtomicBool>,
    pings: Arc<AtomicUsize>,
    channel: GuestChannel<DuplexStream>,
    _qmp: Monitor,
    control: Arc<GuestControl>,
    teardown: Arc<TeardownBarrier>,
    proof: Option<Observed>,
}

impl LiveLaunch for ScriptLaunch {
    type Proof = Observed;

    async fn boot(
        &mut self,
        _cancel: &Shutdown,
        _boot_by: Instant,
        mut refresh: impl FnMut() -> Result<Instant, NativeRefusal>,
    ) -> Result<(), NativeRefusal> {
        refresh().map(|_| ())
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
        cancel: &Shutdown,
        refresh: impl FnMut() -> Result<Instant, NativeRefusal>,
    ) -> Result<(), NativeRefusal> {
        let refused = NativeRefusal::GuestEndpointUnverified;
        if self.control.hold_pings.load(Ordering::SeqCst) {
            self.control.ping_hold.mark_entered();
            tokio::select! {
                _ = cancel.cancelled() => return Err(refused),
                () = self.control.ping_hold.wait_release() => {}
            }
        }
        if self.pings.load(Ordering::SeqCst) == 0 {
            self.channel.ping(cancel).await.map_err(|_| refused)?;
            self.check_live(refresh)?;
        } else if !self.alive.load(Ordering::SeqCst) {
            return Err(NativeRefusal::JobUnverified);
        }
        self.pings.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn run_script(
        &mut self,
        _authority: crate::vm::runtime::ScriptAuthority,
        script: String,
        timeout_s: u32,
        _deadline: Instant,
        _cancel: &Shutdown,
        _refresh: impl FnMut() -> Result<Instant, NativeRefusal>,
    ) -> Result<(arx_guest::ScriptStatus, Option<u32>), NativeRefusal> {
        self.channel
            .run_script(&Shutdown::default(), script, timeout_s)
            .await
            .map_err(|_| NativeRefusal::GuestEndpointUnverified)
    }

    async fn stop(&mut self) -> Option<Observed> {
        self.teardown.mark_entered();
        self.teardown.wait_release().await;
        self.proof.take()
    }
}

/// One installed run serving scripts through the real start adapter,
/// worker and owned loop, with real authenticated MCP requests.
struct ScriptRun {
    vm: Id,
    epoch: Id,
    scope: native::Scope,
    owner: Arc<native_run::Run>,
    control: Arc<GuestControl>,
    alive: Arc<AtomicBool>,
    revoked: Arc<AtomicBool>,
    teardown: Arc<TeardownBarrier>,
    finished: Arc<AtomicBool>,
    witness: Value,
    args: Value,
    _cleanup: ComposedCleanup,
}

impl Drop for ScriptRun {
    /// Frees any held reply or ping before the composed cleanup aborts the
    /// peers, releases teardown and, on a panic, fences the supervisor.
    fn drop(&mut self) {
        self.control.held.release();
        self.control.ping_hold.release();
    }
}

impl ScriptRun {
    fn script(&self, number: u64, script: &str, timeout_s: u32) -> Value {
        json!({"vm_id":self.args["vm_id"], "request_id":key(number),
            "script":format!("{CANARY} {script}"), "timeout_s":timeout_s})
    }

    fn stop(&self, number: u64) -> Value {
        json!({"vm_id":self.args["vm_id"], "request_id":key(number)})
    }

    async fn settled(&self, at_least: usize) {
        eventually("the script receipt to settle", || self.owner.scripts_settled() >= at_least).await;
    }

    async fn teardown_entered(&self) {
        let entered = tokio::time::timeout(Duration::from_secs(15), self.teardown.wait_entered()).await;
        assert!(entered.is_ok(), "teardown never started");
    }

    async fn released(&self, h: &Harness, supervisor: &Supervisor) {
        self.teardown.release();
        eventually("the worker to finish", || self.finished.load(Ordering::SeqCst)).await;
        eventually("the run slot to be released", || {
            supervisor.native_runs.with_entry(self.vm, |owner| owner.is_none()).unwrap_or(false)
        }).await;
        assert_eq!(unresolved_epoch(h, self.vm), None);
        assert_eq!(
            supervisor.controller.active_release_epoch(self.vm, false).unwrap(),
            Some((self.scope.generation, self.epoch))
        );
    }
}

async fn script_run(h: &Harness, client: &Client, number: u64) -> ScriptRun {
    let (vm, slots, pins, scope) = bound_clone(h, client).await;
    let supervisor = h.supervisor().clone();
    assert!(supervisor.native_scripts.set(native_run::NativeScripts {
        installation: Installation::fixture(),
        host: Arc::new(|| HOST),
        resources: RESOURCES,
    }).is_ok());
    let control = Arc::new(GuestControl::default());
    let (host, peer) = tokio::io::duplex(4096);
    let (serial, guest) = tokio::io::duplex(8192);
    let qmp_peer = tokio::spawn(script_qmp(peer));
    let guest_peer = tokio::spawn(script_guest(guest, control.clone()));
    let alive = Arc::new(AtomicBool::new(true));
    let teardown = Arc::new(TeardownBarrier::default());
    let cleanup = ComposedCleanup {
        start_gate: None,
        alive: alive.clone(),
        teardown: teardown.clone(),
        supervisor: supervisor.clone(),
        peers: [qmp_peer.abort_handle(), guest_peer.abort_handle()],
    };
    let ready = Shutdown::default();
    let (read, write) = split(host);
    let mut qmp = Qmp::handshake(read, write, &ready).await.unwrap();
    qmp.release_startup_hold(&ready, || true).await.unwrap();
    let channel = GuestChannel::ready(serial, &mut qmp, &ready).await.unwrap();
    let pings = Arc::new(AtomicUsize::new(0));
    let launch = ScriptLaunch {
        alive: alive.clone(),
        pings: pings.clone(),
        channel,
        _qmp: qmp,
        control: control.clone(),
        teardown: teardown.clone(),
        proof: None,
    };
    let finished = Arc::new(AtomicBool::new(false));
    let worker_finished = finished.clone();
    let revoked = Arc::new(AtomicBool::new(false));
    let selection_revoked = revoked.clone();
    let started = supervisor.gate.lock().unwrap().start_native_run(
        &supervisor, &Installation::fixture(), start_request(vm, number), HOST, RESOURCES,
        move |supervisor, runtime, owner, start| {
            let target = owner.target();
            supervisor.controller.native_store().transact(|db| {
                native::recovery::record_job(db, scope, target.epoch, fixture_origin(scope))
            })?;
            let mut launch = launch;
            launch.proof = Some(Observed { target, scope, slots, pins, clean: true });
            let authorize = FixtureAuthorizer {
                controller: &supervisor.controller,
                revoked: &selection_revoked,
            };
            let revoked = || selection_revoked.load(Ordering::SeqCst);
            runtime.block_on(owner.live(supervisor, &start.worker, &mut launch, &authorize, revoked,
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
    // Ready after the validated handshake, the start commit and one verified
    // ping. The wait reads only atomics while the worker samples the store.
    eventually("a verified ping", || {
        assert!(!teardown.has_entered() && !finished.load(Ordering::SeqCst),
            "the worker entered teardown before its first ping");
        pings.load(Ordering::SeqCst) >= 1
    }).await;
    let owner = supervisor.native_runs.with_entry(vm, |owner| owner.cloned()).unwrap().unwrap();
    let args = json!({"vm_id":hex::encode(vm)});
    let status = call(client, "vm_get", args.clone()).await;
    assert_eq!(status["ready"], true, "{status}");
    assert_eq!(status["native_boot_witness"]["operation_id"], hex::encode(epoch));
    ScriptRun {
        vm, epoch, scope, owner, control, alive, revoked, teardown, finished,
        witness: status["native_boot_witness"].clone(), args, _cleanup: cleanup,
    }
}

async fn operation(client: &Client, receipt: &Value) -> Value {
    call(client, "vm_operation", json!({"operation_id":receipt["operation_id"]})).await
}

/// Every stored text or blob value, so a raw script, nonce or witness can
/// be ruled out of the whole ledger.
fn ledger_contains(h: &Harness, needle: &[u8]) -> bool {
    h.supervisor().controller.native_store().transact(|db| {
        let tables: Vec<String> = db
            .prepare("SELECT name FROM sqlite_master WHERE type='table'")?
            .query_map([], |row| row.get(0))?
            .collect::<Result<_, _>>()?;
        for table in tables {
            let mut statement = db.prepare(&format!("SELECT * FROM \"{table}\""))?;
            let columns = statement.column_count();
            let mut rows = statement.query([])?;
            while let Some(row) = rows.next()? {
                for column in 0..columns {
                    let bytes = match row.get_ref(column)? {
                        rusqlite::types::ValueRef::Text(bytes)
                        | rusqlite::types::ValueRef::Blob(bytes) => bytes,
                        _ => continue,
                    };
                    if bytes.windows(needle.len()).any(|window| window == needle) {
                        return Ok(true);
                    }
                }
            }
        }
        Ok(false)
    }).unwrap()
}

async fn finish_script_test(mut h: Harness, clients: Vec<Client>, run: ScriptRun) {
    drop(run);
    assert_admission_closed(&h);
    drop(clients);
    let _ = h.shutdown().await;
}

#[tokio::test]
async fn owned_scripts_keep_typed_results_and_the_witness_without_leaking_bytes() {
    let h = windows_harness().await;
    let client = h.client(0).await;
    let run = script_run(&h, &client, 520).await;
    let cases = [
        ("exit 0", 30, "succeeded", json!(0), "exited"),
        ("exit 7", 45, "failed", json!(7), "exited"),
        ("timeout", 2, "failed", Value::Null, "timed_out"),
    ];
    let mut receipts = Vec::new();
    for (number, (script, timeout, state, code, status)) in (530..).zip(cases.clone()) {
        let args = run.script(number, script, timeout);
        let before = run.owner.scripts_settled();
        let receipt = call(&client, "vm_run_script", args.clone()).await;
        assert_eq!(receipt["ok"], true, "{receipt}");
        run.settled(before + 1).await;
        let settled = operation(&client, &receipt).await;
        assert_eq!(settled["action"], "run_script", "{settled}");
        assert_eq!(settled["state"], state, "{settled}");
        assert_eq!(settled["script_status"], status, "{settled}");
        assert_eq!(settled.get("exit_code").cloned().unwrap_or(Value::Null), code, "{settled}");
        assert!(settled.get("reason").is_none() || state == "failed", "{settled}");
        // The channel stayed verified, so the same witness stays published.
        let current = call(&client, "vm_get", run.args.clone()).await;
        assert_eq!(current["ready"], true, "{current}");
        assert_eq!(current["native_boot_witness"], run.witness);
        for read in [&receipt, &settled, &current] {
            assert!(!read.to_string().contains(CANARY), "script bytes leaked: {read}");
        }
        receipts.push((args, receipt));
    }
    // Each exchange carried exactly the admitted bytes and timeout, once.
    let expected: Vec<_> = cases.iter()
        .map(|(script, timeout, ..)| (format!("{CANARY} {script}"), *timeout))
        .collect();
    assert_eq!(run.control.received(), expected);
    // A same-request replay returns the original receipt with no exchange.
    let settled = run.owner.scripts_settled();
    for (args, receipt) in &receipts {
        assert_eq!(call(&client, "vm_run_script", args.clone()).await, *receipt);
    }
    assert_eq!(run.control.received().len(), 3);
    assert_eq!(run.owner.scripts_settled(), settled);
    // No raw script, challenge nonce or witness reaches the ledger.
    let witness = parse_id(run.witness["witness_id"].as_str().unwrap()).unwrap();
    let challenges = run.control.challenges.lock().unwrap().clone();
    assert!(!challenges.is_empty());
    assert!(!ledger_contains(&h, CANARY.as_bytes()));
    assert!(!ledger_contains(&h, &witness));
    assert!(!ledger_contains(&h, hex::encode(witness).as_bytes()));
    for challenge in challenges {
        assert!(!ledger_contains(&h, challenge.as_bytes()));
    }
    finish_script_test(h, vec![client], run).await;
}

#[tokio::test]
async fn owner_stop_interrupts_a_paused_script_before_its_timeout_and_stops_cleanly() {
    // Without a reply only the interrupt bound ends the exchange; a reply
    // that lands after the stop cannot replace the receipt the stop settled.
    for reply_after_stop in [false, true] {
        let h = windows_harness().await;
        let client = h.client(0).await;
        let other = h.client(1).await;
        let run = script_run(&h, &client, 540).await;
        let supervisor = h.supervisor().clone();
        let args = run.script(541, "hold", 60);
        let receipt = call(&client, "vm_run_script", args.clone()).await;
        assert_eq!(receipt["ok"], true, "{receipt}");
        let held = tokio::time::timeout(Duration::from_secs(20), run.control.held.wait_entered()).await;
        assert!(held.is_ok(), "the guest never received the script");
        // In flight: the witness stays, a replay returns the same receipt and
        // a competing script or foreign stop is refused, all without a
        // second exchange.
        assert_eq!(call(&client, "vm_get", run.args.clone()).await["ready"], true);
        assert_eq!(call(&client, "vm_run_script", args.clone()).await, receipt);
        let competing = call(&client, "vm_run_script", run.script(542, "exit 0", 30)).await;
        assert_eq!(competing["ok"], false, "{competing}");
        assert_eq!(call(&other, "vm_stop", run.stop(543)).await["ok"], false);
        assert_eq!(call(&client, "vm_get", run.args.clone()).await["ready"], true);

        let stopped_at = Instant::now();
        let stop = call(&client, "vm_stop", run.stop(544)).await;
        assert_eq!(stop["ok"], true, "{stop}");
        // Withdrawn at stop admission, while the exchange is still blocked.
        let stopping = call(&client, "vm_get", run.args.clone()).await;
        assert_eq!(stopping["ready"], false, "{stopping}");
        assert!(stopping.get("native_boot_witness").is_none());
        assert_eq!(call(&client, "vm_stop", run.stop(545)).await["ok"], false);
        assert_eq!(call(&client, "vm_stop", run.stop(544)).await, stop);
        if reply_after_stop {
            run.control.held.release();
        }
        let entered = tokio::time::timeout_at(
            (stopped_at + Duration::from_secs(3)).into(),
            run.teardown.wait_entered(),
        ).await;
        assert!(entered.is_ok(), "teardown exceeded the 2 s interrupt bound with 1 s tolerance");
        assert_eq!(run.control.replied.load(Ordering::SeqCst), reply_after_stop);
        // While teardown is blocked: still unready, and the in-flight
        // script settled with the stop as unknown, with no guest result.
        let blocked = call(&client, "vm_get", run.args.clone()).await;
        assert_eq!(blocked["ready"], false, "{blocked}");
        assert!(blocked.get("native_boot_witness").is_none());
        let script = operation(&client, &receipt).await;
        assert_eq!(script["state"], "failed", "{script}");
        assert_eq!(script["reason"], "execution_unknown", "{script}");
        assert!(script.get("script_status").is_none(), "{script}");
        assert!(!run.finished.load(Ordering::SeqCst));
        run.released(&h, &supervisor).await;
        let settled = terminal(&client, &stop).await;
        assert_eq!(settled["state"], "succeeded", "{settled}");
        assert_eq!(vm_state(&h, run.vm), 1, "the owner stop did not stop cleanly");
        assert_eq!(run.control.received().len(), 1);
        drop(supervisor);
        finish_script_test(h, vec![client, other], run).await;
    }
}

#[tokio::test]
async fn stop_or_loss_settles_a_queued_script_that_never_reached_the_guest() {
    for (number, by_stop) in [(550, true), (555, false)] {
        let h = windows_harness().await;
        let client = h.client(0).await;
        let run = script_run(&h, &client, number).await;
        let supervisor = h.supervisor().clone();
        run.control.hold_pings.store(true, Ordering::SeqCst);
        let parked = tokio::time::timeout(Duration::from_secs(20), run.control.ping_hold.wait_entered()).await;
        assert!(parked.is_ok(), "the run never parked in a ping");
        // The loop is busy in its ping, so the admitted script stays queued.
        let receipt = call(&client, "vm_run_script", run.script(number + 1, "exit 0", 30)).await;
        assert_eq!(receipt["ok"], true, "{receipt}");
        let stop = if by_stop {
            let stop = call(&client, "vm_stop", run.stop(number + 2)).await;
            assert_eq!(stop["ok"], true, "{stop}");
            Some(stop)
        } else {
            // The child is lost; the parked ping returns and finds it gone.
            run.alive.store(false, Ordering::SeqCst);
            run.control.ping_hold.release();
            None
        };
        run.teardown_entered().await;
        let script = operation(&client, &receipt).await;
        assert_eq!(script["state"], "failed", "stop={by_stop}: {script}");
        assert_eq!(script["reason"], "cancelled", "stop={by_stop}: {script}");
        assert!(run.control.received().is_empty(), "a queued script reached the guest");
        assert_eq!(call(&client, "vm_get", run.args.clone()).await["ready"], false);
        run.released(&h, &supervisor).await;
        if let Some(stop) = stop {
            assert_eq!(terminal(&client, &stop).await["state"], "succeeded");
            assert_eq!(vm_state(&h, run.vm), 1);
        } else {
            assert_eq!(vm_state(&h, run.vm), 3);
        }
        drop(supervisor);
        finish_script_test(h, vec![client], run).await;
    }
}

#[tokio::test]
async fn a_script_result_after_revocation_loses_to_the_terminal_cutoff() {
    let h = windows_harness().await;
    let client = h.client(0).await;
    let run = script_run(&h, &client, 620).await;
    let supervisor = h.supervisor().clone();
    let receipt = call(&client, "vm_run_script", run.script(621, "hold", 30)).await;
    assert_eq!(receipt["ok"], true, "{receipt}");
    let held = tokio::time::timeout(Duration::from_secs(20), run.control.held.wait_entered()).await;
    assert!(held.is_ok(), "the guest never received the script");
    // The selection is revoked after the exchange's last sample; the guest's
    // successful reply then arrives and must not count.
    run.revoked.store(true, Ordering::SeqCst);
    run.control.held.release();
    run.teardown_entered().await;
    assert!(run.control.replied.load(Ordering::SeqCst));
    let script = operation(&client, &receipt).await;
    assert_eq!(script["state"], "interrupted", "{script}");
    assert_eq!(script["reason"], "execution_unknown", "{script}");
    assert!(script.get("script_status").is_none(), "{script}");
    assert_eq!(call(&client, "vm_get", run.args.clone()).await["ready"], false);
    run.released(&h, &supervisor).await;
    assert_eq!(vm_state(&h, run.vm), 3);
    drop(supervisor);
    finish_script_test(h, vec![client], run).await;
}

#[tokio::test]
async fn transport_loss_during_a_script_withdraws_before_teardown_and_reaches_recovery() {
    for (number, fault) in [(560, "drop"), (570, "garbage")] {
        let h = windows_harness().await;
        let client = h.client(0).await;
        let run = script_run(&h, &client, number).await;
        let supervisor = h.supervisor().clone();
        let receipt = call(&client, "vm_run_script", run.script(number + 1, fault, 30)).await;
        assert_eq!(receipt["ok"], true, "{receipt}");
        run.teardown_entered().await;
        let lost = call(&client, "vm_get", run.args.clone()).await;
        assert_eq!(lost["ready"], false, "{fault}: {lost}");
        assert!(lost.get("native_boot_witness").is_none());
        let script = operation(&client, &receipt).await;
        assert_eq!(script["state"], "interrupted", "{fault}: {script}");
        assert_eq!(script["reason"], "execution_unknown", "{fault}: {script}");
        assert!(script.get("script_status").is_none(), "{script}");
        assert_eq!(vm_state(&h, run.vm), 3, "{fault}: a lost channel must enter recovery");
        assert_eq!(unresolved_epoch(&h, run.vm), Some((run.scope.generation, run.epoch)));
        assert!(!run.finished.load(Ordering::SeqCst));
        run.released(&h, &supervisor).await;
        assert_eq!(run.control.received().len(), 1);
        drop(supervisor);
        finish_script_test(h, vec![client], run).await;
    }
}

const FAIL_SCRIPT_RECEIPT: &str = "CREATE TRIGGER fail_script BEFORE UPDATE OF status ON vm_operations
    WHEN NEW.action=2 BEGIN SELECT RAISE(ABORT,'fixture receipt failure'); END";

fn sql(h: &Harness, batch: &str) {
    h.supervisor().controller.native_store().transact(|db| { db.execute_batch(batch)?; Ok(()) }).unwrap();
}

#[tokio::test]
async fn failed_script_receipt_keeps_its_typed_result_until_the_ledger_settles_it() {
    let h = windows_harness().await;
    let client = h.client(0).await;
    let run = script_run(&h, &client, 580).await;
    sql(&h, FAIL_SCRIPT_RECEIPT);
    let receipt = call(&client, "vm_run_script", run.script(581, "exit 7", 30)).await;
    assert_eq!(receipt["ok"], true, "{receipt}");
    run.settled(1).await;
    // The first settlement failed; the receipt is kept, not dropped, and the
    // still-verified channel keeps its witness.
    assert_eq!(operation(&client, &receipt).await["state"], "pending");
    assert_eq!(call(&client, "vm_get", run.args.clone()).await["ready"], true);
    run.settled(2).await;
    assert_eq!(operation(&client, &receipt).await["state"], "pending");
    sql(&h, "DROP TRIGGER fail_script");
    let settled = await_operation(&client, &receipt, Duration::from_secs(20)).await;
    assert_eq!(settled["state"], "failed", "{settled}");
    assert_eq!(settled["script_status"], "exited", "{settled}");
    assert_eq!(settled["exit_code"], 7, "{settled}");
    // Retrying the receipt never repeats the guest exchange.
    assert_eq!(run.control.received().len(), 1);
    assert_eq!(call(&client, "vm_get", run.args.clone()).await["ready"], true);
    finish_script_test(h, vec![client], run).await;
}

#[tokio::test]
async fn a_failed_receipt_retry_past_its_deadline_starts_teardown() {
    let h = windows_harness().await;
    let client = h.client(0).await;
    let run = script_run(&h, &client, 582).await;
    let supervisor = h.supervisor().clone();
    sql(&h, FAIL_SCRIPT_RECEIPT);
    let receipt = call(&client, "vm_run_script", run.script(583, "exit 7", 1)).await;
    assert_eq!(receipt["ok"], true, "{receipt}");
    let after_admission = tokio::time::Instant::now();
    run.settled(1).await;
    assert_eq!(operation(&client, &receipt).await["state"], "pending");
    assert!(run.owner.witnessed());

    // Keep the already-started exchange parked through the cutoff. On
    // resuming, the retry itself must enter teardown before another exchange.
    run.control.hold_pings.store(true, Ordering::SeqCst);
    tokio::time::timeout(Duration::from_secs(3), run.control.ping_hold.wait_entered()).await.unwrap();
    tokio::time::sleep_until(after_admission + Duration::from_secs(6)).await;
    sql(&h, "DROP TRIGGER fail_script");
    run.control.ping_hold.release();
    let entered = tokio::time::timeout(Duration::from_secs(2), run.teardown.wait_entered()).await;
    assert!(entered.is_ok(), "the deadline retry did not start teardown");
    assert!(!run.owner.witnessed());
    let script = operation(&client, &receipt).await;
    assert_eq!(script["state"], "interrupted", "{script}");
    assert_eq!(script["reason"], "execution_unknown", "{script}");
    assert!(script.get("exit_code").is_none(), "{script}");
    run.released(&h, &supervisor).await;
    assert_eq!(vm_state(&h, run.vm), 3);
    assert_eq!(run.control.received().len(), 1);
    drop(supervisor);
    finish_script_test(h, vec![client], run).await;
}

#[tokio::test]
async fn a_held_store_connection_cannot_delay_script_loss_teardown() {
    let h = windows_harness().await;
    let client = h.client(0).await;
    let run = script_run(&h, &client, 584).await;
    let supervisor = h.supervisor().clone();
    let receipt = call(&client, "vm_run_script", run.script(585, "hold", 60)).await;
    assert_eq!(receipt["ok"], true, "{receipt}");
    tokio::time::timeout(Duration::from_secs(3), run.control.held.wait_entered()).await.unwrap();
    let (held, entered) = tokio::sync::oneshot::channel();
    let (release, wait) = std::sync::mpsc::channel::<()>();
    let holder = supervisor.clone();
    let holding = tokio::task::spawn_blocking(move || {
        holder.controller.native_store().transact(|_| {
            held.send(()).unwrap();
            let _ = wait.recv_timeout(Duration::from_secs(10));
            Ok(())
        }).unwrap();
    });
    entered.await.unwrap();
    // The owner's final monitor sample refuses while the connection mutex
    // is held. Teardown must start before this other thread releases it.
    run.control.held.release();
    let started = tokio::time::timeout(Duration::from_secs(2), run.teardown.wait_entered()).await;
    drop(release);
    holding.await.unwrap();
    assert!(started.is_ok(), "the held store mutex delayed teardown");
    assert!(!run.owner.witnessed());
    assert_eq!(operation(&client, &receipt).await["state"], "pending");
    run.released(&h, &supervisor).await;
    let script = operation(&client, &receipt).await;
    assert_eq!(script["state"], "interrupted", "{script}");
    assert_eq!(script["reason"], "execution_unknown", "{script}");
    assert_eq!(vm_state(&h, run.vm), 3);
    drop(supervisor);
    finish_script_test(h, vec![client], run).await;
}

#[tokio::test]
async fn a_script_receipt_write_fault_preserves_the_owner_stop_and_starts_teardown() {
    let h = windows_harness().await;
    let client = h.client(0).await;
    let run = script_run(&h, &client, 586).await;
    let supervisor = h.supervisor().clone();
    let receipt = call(&client, "vm_run_script", run.script(587, "hold", 60)).await;
    assert_eq!(receipt["ok"], true, "{receipt}");
    tokio::time::timeout(Duration::from_secs(3), run.control.held.wait_entered()).await.unwrap();
    sql(&h, FAIL_SCRIPT_RECEIPT);
    let args = run.stop(588);
    let stop = call(&client, "vm_stop", args.clone()).await;
    assert_eq!(stop["ok"], true, "{stop}");
    let entered = tokio::time::timeout(Duration::from_secs(3), run.teardown.wait_entered()).await;
    assert!(entered.is_ok(), "the receipt write fault suppressed owner cancellation");
    assert!(!run.owner.witnessed());
    assert_eq!(call(&client, "vm_stop", args).await, stop);
    let competing = call(&client, "vm_stop", run.stop(589)).await;
    assert_eq!(competing["ok"], false, "{competing}");
    let stop = operation(&client, &stop).await;
    assert_eq!(stop["state"], "interrupted", "{stop}");
    assert_eq!(stop["reason"], "cleanup_unconfirmed", "{stop}");
    assert_eq!(operation(&client, &receipt).await["state"], "pending");
    assert_eq!(unresolved_epoch(&h, run.vm), Some((run.scope.generation, run.epoch)));

    sql(&h, "DROP TRIGGER fail_script");
    run.released(&h, &supervisor).await;
    let script = operation(&client, &receipt).await;
    assert_eq!(script["state"], "failed", "{script}");
    assert_eq!(script["reason"], "execution_unknown", "{script}");
    assert_eq!(vm_state(&h, run.vm), 3);
    drop(supervisor);
    finish_script_test(h, vec![client], run).await;
}

#[tokio::test]
async fn pending_cap_counts_the_script_replaced_by_an_authenticated_stop() {
    for (number, in_flight) in [(645, true), (650, false)] {
        let h = windows_harness().await;
        let client = h.client(0).await;
        let run = script_run(&h, &client, number).await;
        let supervisor = h.supervisor().clone();
        let script = if in_flight {
            let script = call(&client, "vm_run_script", run.script(number + 1, "hold", 60)).await;
            assert_eq!(script["ok"], true, "{script}");
            tokio::time::timeout(Duration::from_secs(3), run.control.held.wait_entered()).await.unwrap();
            Some(script)
        } else {
            None
        };
        supervisor.controller.native_store().transact(|db| {
            for _ in 0..store::MAX_CALLER_PENDING - i64::from(in_flight) {
                db.execute(
                    "INSERT INTO activity(principal,actor,vm,action,started_ms)
                     VALUES('capfixture','agent',randomblob(16),2,0)",
                    [],
                )?;
                db.execute(
                    "INSERT INTO vm_operations(principal,request_key,vm,action,activity_id,valid_through_ms)
                     SELECT principal,randomblob(16),vm,action,id,0 FROM activity WHERE id=last_insert_rowid()",
                    [],
                )?;
            }
            assert_eq!(store::count(db, "SELECT count(*) FROM vm_operations WHERE status=0", [])?,
                store::MAX_CALLER_PENDING);
            Ok(())
        }).unwrap();
        let args = run.stop(number + 2);
        let stop = call(&client, "vm_stop", args.clone()).await;
        if let Some(script) = script {
            assert_eq!(stop["ok"], true, "{stop}");
            assert_eq!(operation(&client, &script).await["reason"], "execution_unknown");
        } else {
            assert_eq!(stop["reason"], "pending_full", "{stop}");
            assert!(run.owner.witnessed());
        }
        sql(&h, "DELETE FROM vm_operations WHERE principal='capfixture';
                 DELETE FROM activity WHERE principal='capfixture'");
        let stop = if in_flight { stop } else { call(&client, "vm_stop", args).await };
        assert_eq!(stop["ok"], true, "{stop}");
        run.teardown_entered().await;
        run.released(&h, &supervisor).await;
        assert_eq!(terminal(&client, &stop).await["state"], "succeeded");
        drop(supervisor);
        finish_script_test(h, vec![client], run).await;
    }
}

#[tokio::test]
async fn revocation_of_a_queued_script_withdraws_on_authorization_refusal() {
    let h = windows_harness().await;
    let client = h.client(0).await;
    let run = script_run(&h, &client, 655).await;
    let supervisor = h.supervisor().clone();
    run.control.hold_pings.store(true, Ordering::SeqCst);
    tokio::time::timeout(Duration::from_secs(3), run.control.ping_hold.wait_entered()).await.unwrap();
    let receipt = call(&client, "vm_run_script", run.script(656, "exit 0", 60)).await;
    assert_eq!(receipt["ok"], true, "{receipt}");
    run.revoked.store(true, Ordering::SeqCst);
    run.control.ping_hold.release();
    let entered = tokio::time::timeout(Duration::from_secs(2), run.teardown.wait_entered()).await;
    assert!(entered.is_ok(), "refused authorization left the revoked run serving");
    assert!(!run.owner.witnessed());
    assert!(run.control.received().is_empty());
    let script = operation(&client, &receipt).await;
    assert_eq!(script["state"], "failed", "{script}");
    assert_eq!(script["reason"], "cancelled", "{script}");
    run.released(&h, &supervisor).await;
    drop(supervisor);
    finish_script_test(h, vec![client], run).await;
}

#[tokio::test]
async fn failed_loss_receipt_cannot_strand_the_withdrawn_teardown() {
    let h = windows_harness().await;
    let client = h.client(0).await;
    let run = script_run(&h, &client, 590).await;
    let supervisor = h.supervisor().clone();
    sql(&h, FAIL_SCRIPT_RECEIPT);
    let receipt = call(&client, "vm_run_script", run.script(591, "drop", 30)).await;
    assert_eq!(receipt["ok"], true, "{receipt}");
    run.teardown_entered().await;
    // The loss receipt could not settle, so the ledger still reads running:
    // only the owner's withdrawal at loss detection keeps the witness off.
    assert_eq!(vm_state(&h, run.vm), 2);
    let lost = call(&client, "vm_get", run.args.clone()).await;
    assert_eq!(lost["ready"], false, "{lost}");
    assert!(lost.get("native_boot_witness").is_none());
    assert_eq!(operation(&client, &receipt).await["state"], "pending");
    assert!(!run.finished.load(Ordering::SeqCst));
    // The release keeps retrying behind the failing ledger, then settles both
    // the release and the pending receipt once the ledger recovers.
    run.teardown.release();
    sql(&h, "DROP TRIGGER fail_script");
    run.released(&h, &supervisor).await;
    let script = operation(&client, &receipt).await;
    assert_eq!(script["state"], "interrupted", "{script}");
    assert_eq!(vm_state(&h, run.vm), 3);
    assert_eq!(run.control.received().len(), 1);
    drop(supervisor);
    finish_script_test(h, vec![client], run).await;
}

#[tokio::test]
async fn dispatch_refuses_mismatched_or_stale_script_bindings_without_an_exchange() {
    let h = windows_harness().await;
    let client = h.client(0).await;
    let run = script_run(&h, &client, 600).await;
    let supervisor = h.supervisor().clone();
    let controller = &supervisor.controller;
    let admit = |number: u64, script: &str, timeout: u32| {
        let script = format!("{CANARY} {script}");
        let request = Request {
            principal: "alpha", vm: run.vm, request_id: RequestId::parse(&key(number)).unwrap(),
            operation: Operation::Script { script: &script, timeout_seconds: timeout.into() },
            disk: DiskClaim::default(),
        };
        let Admission::Admitted(worker) = controller
            .admit_native_script(&Installation::fixture(), &request, run.owner.target(), HOST, RESOURCES)
            .unwrap()
        else { panic!("script admission refused") };
        let receipt = json!({"ok":true,
            "operation_id":hex::encode(worker.admitted().unwrap().operation)});
        (worker, receipt)
    };
    let dispatch = async |worker, script: &str, timeout_s| {
        let before = run.owner.scripts_settled();
        let job = native_run::ScriptJob { worker, script: format!("{CANARY} {script}"), timeout_s };
        assert!(run.owner.enqueue(job).is_ok());
        run.settled(before + 1).await;
    };
    // Changed bytes or timeout at dispatch are refused before the guest.
    for (number, sent, timeout) in [(601, "exit 7", 30), (602, "exit 0", 31)] {
        let (worker, receipt) = admit(number, "exit 0", 30);
        dispatch(worker, sent, timeout).await;
        let refused = operation(&client, &receipt).await;
        assert_eq!(refused["state"], "failed", "{refused}");
        assert_eq!(refused["reason"], "cancelled", "{refused}");
        assert!(run.control.received().is_empty(), "a mismatched script reached the guest");
        assert_eq!(call(&client, "vm_get", run.args.clone()).await["ready"], true);
    }
    // Positive control on the same direct path.
    let (worker, receipt) = admit(603, "exit 0", 30);
    dispatch(worker, "exit 0", 30).await;
    assert_eq!(operation(&client, &receipt).await["state"], "succeeded");
    assert_eq!(run.control.received().len(), 1);
    // A writer epoch replaced after admission is stale at dispatch.
    let (worker, receipt) = admit(604, "exit 0", 30);
    controller.native_store()
        .transact(|db| store::snapshot::invalidate_release(db, run.vm, [44; 16]))
        .unwrap();
    dispatch(worker, "exit 0", 30).await;
    let stale = operation(&client, &receipt).await;
    assert_eq!(stale["state"], "failed", "{stale}");
    assert_eq!(stale["reason"], "cancelled", "{stale}");
    assert_eq!(run.control.received().len(), 1);
    drop(supervisor);
    finish_script_test(h, vec![client], run).await;
}

#[tokio::test]
async fn a_script_admitted_as_its_run_is_lost_is_settled_not_stranded() {
    let h = windows_harness().await;
    let client = h.client(0).await;
    let run = script_run(&h, &client, 610).await;
    let supervisor = h.supervisor().clone();
    let script = format!("{CANARY} exit 0");
    let request = Request {
        principal: "alpha", vm: run.vm, request_id: RequestId::parse(&key(611)).unwrap(),
        operation: Operation::Script { script: &script, timeout_seconds: 30 },
        disk: DiskClaim::default(),
    };
    let Admission::Admitted(worker) = supervisor.controller
        .admit_native_script(&Installation::fixture(), &request, run.owner.target(), HOST, RESOURCES)
        .unwrap()
    else { panic!("script admission refused") };
    let receipt = json!({"ok":true, "operation_id":hex::encode(worker.admitted().unwrap().operation)});
    // The route admits and hands off under the foreground gate, which a stop
    // also takes; only a loss can end the run in between.
    run.alive.store(false, Ordering::SeqCst);
    run.teardown_entered().await;
    let job = native_run::ScriptJob { worker, script, timeout_s: 30 };
    let Err(job) = run.owner.enqueue(job) else { panic!("an ended run queued a script") };
    // The route's hand-off failure path settles the receipt it admitted.
    run.owner.settle_script(&supervisor, job.worker, native_run::Settlement::Cancelled);
    let settled = operation(&client, &receipt).await;
    assert_eq!(settled["state"], "failed", "{settled}");
    assert_eq!(settled["reason"], "cancelled", "{settled}");
    assert!(run.control.received().is_empty());
    run.released(&h, &supervisor).await;
    assert_eq!(vm_state(&h, run.vm), 3);
    drop(supervisor);
    finish_script_test(h, vec![client], run).await;
}

#[tokio::test]
async fn a_script_past_its_admitted_deadline_withdraws_and_tears_down() {
    // A 1 s script is admitted with a 1 s + 5 s worker deadline while the
    // run is busy in a ping, so its exchange starts late. Either its typed
    // reply would land after that deadline, or its authorization expires in
    // the queue. Both receipts can only settle as unconfirmed, so the run
    // must withdraw and tear down at the deadline instead of serving on.
    for (number, held_past_deadline) in [(630, false), (635, true)] {
        let h = windows_harness().await;
        let client = h.client(0).await;
        let run = script_run(&h, &client, number).await;
        let supervisor = h.supervisor().clone();
        run.control.hold_pings.store(true, Ordering::SeqCst);
        let parked = tokio::time::timeout(Duration::from_secs(20), run.control.ping_hold.wait_entered()).await;
        assert!(parked.is_ok(), "the run never parked in a ping");
        let receipt = call(&client, "vm_run_script", run.script(number + 1, "hold", 1)).await;
        assert_eq!(receipt["ok"], true, "{receipt}");
        // The worker deadline is at most 6 s after this instant.
        let admitted = tokio::time::Instant::now();
        let deadline = admitted + Duration::from_secs(6);
        if held_past_deadline {
            tokio::time::sleep_until(deadline + Duration::from_secs(1)).await;
            assert!(!run.teardown.has_entered());
            run.control.ping_hold.release();
        } else {
            // Dispatch at +4 s: the exchange's own 6 s bound would run to
            // +10 s, past the worker deadline at +6 s.
            tokio::time::sleep_until(admitted + Duration::from_secs(4)).await;
            run.control.ping_hold.release();
            let held = tokio::time::timeout(Duration::from_secs(1), run.control.held.wait_entered()).await;
            assert!(held.is_ok(), "the guest never received the script");
        }
        // Teardown starts at the deadline, well before the exchange's own
        // bound at +10 s, without any read prompting it.
        let entered = tokio::time::timeout_at(
            admitted + Duration::from_secs(9),
            run.teardown.wait_entered(),
        ).await;
        assert!(entered.is_ok(), "held={held_past_deadline}: the run served on past the deadline");
        assert!(!run.control.replied.load(Ordering::SeqCst));
        let blocked = call(&client, "vm_get", run.args.clone()).await;
        assert_eq!(blocked["ready"], false, "{blocked}");
        assert!(blocked.get("native_boot_witness").is_none());
        let script = operation(&client, &receipt).await;
        assert_eq!(script["state"], "interrupted", "held={held_past_deadline}: {script}");
        assert!(script.get("script_status").is_none(), "{script}");
        let reason = if held_past_deadline { "cancelled" } else { "execution_unknown" };
        assert_eq!(script["reason"], reason, "{script}");
        assert_eq!(run.control.received().len(), usize::from(!held_past_deadline));
        run.released(&h, &supervisor).await;
        assert_eq!(vm_state(&h, run.vm), 3);
        drop(supervisor);
        finish_script_test(h, vec![client], run).await;
    }
}
