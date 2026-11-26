use super::{super::*, *};
use crate::vm::{
    reaper::{ArtifactCustody, CleanupKind, CleanupWork},
    runtime::{Quiesced, ReaperRuntime, StopDisposition},
};
use std::sync::mpsc;
use tokio::{runtime::Builder, sync::oneshot};

const BOUND: Duration = Duration::from_secs(5);

fn open(root: &Path) -> Arc<Supervisor> {
    Supervisor::new(
        root,
        VmPolicy::default(),
        ForegroundClock {
            utc: Some(Arc::new(super::super::tests::UTC.into())),
        },
        Some(super::super::tests::RECIPE),
        NativeRefusal::QualificationMissing,
        None,
    )
    .unwrap()
}

fn runtime() -> io::Result<Runtime> {
    Builder::new_current_thread().enable_all().build()
}

async fn pre_entry_failures<T: Send + 'static>(value: impl Fn() -> T) {
    let mut active_after_drain = Vec::new();
    for initialize in [
        (|| Err(io::Error::other("injected pre-entry error"))) as fn() -> io::Result<Runtime>,
        || panic!("injected pre-entry panic"),
    ] {
        let root = crate::service::application::tests::private_temp();
        let supervisor = open(root.path());
        let registry = Registry::default();
        let subject = [1; 16];
        let entered = Arc::new(AtomicBool::new(false));
        {
            let entered = entered.clone();
            let mut gate = supervisor.lock().unwrap();
            let registration = registry.register(subject, value()).unwrap();
            gate.enqueue_test_worker(&supervisor, registration, initialize, move |_, _| {
                entered.store(true, Ordering::SeqCst);
                Ok(())
            })
            .unwrap();
        }
        supervisor.close();
        assert_eq!(
            supervisor
                .drain_workers(BOUND)
                .await
                .err()
                .unwrap()
                .to_string(),
            "TEST worker completion unconfirmed"
        );
        assert!(!entered.load(Ordering::SeqCst));
        active_after_drain.push(
            supervisor
                .observe_test_worker(&registry, subject, |entry| entry.is_some())
                .unwrap(),
        );
    }
    assert_eq!(
        active_after_drain,
        [false, false],
        "pre-entry error and panic"
    );
}

#[tokio::test]
async fn build_registration_drops_on_pre_entry_error_and_panic() {
    pre_entry_failures(|| {}).await;
}

#[tokio::test]
async fn clone_registration_drops_on_pre_entry_error_and_panic() {
    pre_entry_failures(|| {}).await;
}

#[tokio::test]
async fn boot_run_entry_drops_on_pre_entry_error_and_panic() {
    pre_entry_failures(|| tokio::sync::mpsc::channel::<()>(1).0).await;
}

#[tokio::test]
async fn copy_registration_drops_on_pre_entry_error_and_panic() {
    pre_entry_failures(Shutdown::default).await;
}

#[test]
fn late_boot_registration_does_not_remove_successor() {
    let runs = Registry::default();
    let vm = [2; 16];
    let predecessor = runs.register(vm, 1).unwrap();
    assert_eq!(runs.register(vm, 2).err().unwrap().0, "busy");
    assert_eq!(runs.with_entry(vm, |run| run.copied()).unwrap(), Some(1));
    // Model a retired run whose task still holds its old registration.
    runs.0.lock().unwrap().remove(&vm).unwrap();
    let successor = runs.register(vm, 2).unwrap();
    drop(predecessor);
    assert_eq!(runs.with_entry(vm, |run| run.copied()).unwrap(), Some(2));
    drop(successor);
    assert_eq!(runs.with_entry(vm, |run| run.copied()).unwrap(), None);
}

fn serialized_executor() -> Runtime {
    Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(1)
        .build()
        .unwrap()
}

#[test]
fn abort_before_worker_entry_releases_queued_registration() {
    serialized_executor().block_on(async {
        let root = crate::service::application::tests::private_temp();
        let supervisor = open(root.path());
        let registry = Registry::default();
        let subject = [3; 16];
        let (entered, busy) = oneshot::channel();
        let (release, parked) = mpsc::channel();
        let blocker = tokio::task::spawn_blocking(move || {
            entered.send(()).unwrap();
            parked.recv_timeout(BOUND).unwrap();
        });
        tokio::time::timeout(BOUND, busy).await.unwrap().unwrap();
        let reached = Arc::new(AtomicBool::new(false));
        {
            let reached = reached.clone();
            let mut gate = supervisor.lock().unwrap();
            let registration = registry.register(subject, ()).unwrap();
            let queued = gate
                .enqueue_test_worker(&supervisor, registration, runtime, move |_, _| {
                    reached.store(true, Ordering::SeqCst);
                    Ok(())
                })
                .unwrap();
            assert!(
                registry
                    .with_entry(subject, |entry| entry.is_some())
                    .unwrap()
            );
            queued.abort();
        }
        release.send(()).unwrap();
        blocker.await.unwrap();
        supervisor.close();
        assert!(supervisor.drain_workers(BOUND).await.is_err());
        assert!(!reached.load(Ordering::SeqCst));
        assert!(
            !registry
                .with_entry(subject, |entry| entry.is_some())
                .unwrap()
        );
    });
}

#[tokio::test]
async fn disconnected_request_leaves_worker_owned_until_cooperative_drain() {
    let root = crate::service::application::tests::private_temp();
    let supervisor = open(root.path());
    let registry = Registry::default();
    let connection = supervisor.connection("alpha".into());
    let subject = [4; 16];
    let (entered, running) = oneshot::channel();
    {
        let mut gate = supervisor.lock().unwrap();
        let registration = registry.register(subject, ()).unwrap();
        gate.enqueue_test_worker(
            &supervisor,
            registration,
            runtime,
            move |supervisor, executor| {
                entered.send(()).unwrap();
                executor.block_on(async {
                    tokio::time::timeout(BOUND, supervisor.cancel.cancelled()).await
                })?;
                Ok(())
            },
        )
        .unwrap();
    }
    drop(connection);
    tokio::time::timeout(BOUND, running).await.unwrap().unwrap();
    assert!(!supervisor.cancel.is_cancelled());
    assert!(
        supervisor
            .observe_test_worker(&registry, subject, |entry| entry.is_some())
            .unwrap()
    );
    supervisor.close();
    assert!(supervisor.drain_workers(BOUND).await.is_ok());
    assert!(
        !registry
            .with_entry(subject, |entry| entry.is_some())
            .unwrap()
    );
}

fn build_request(recipe: &str) -> BuildRequest<'_> {
    use super::super::tests::{RECIPE, RETAINED, TRANSIENT, key};
    BuildRequest {
        principal: "alpha",
        key: RequestId::parse(&key(1)).unwrap(),
        recipe,
        digest: RECIPE.1,
        retained: RETAINED,
        transient: TRANSIENT,
    }
}

fn reserve_build(supervisor: &Supervisor) -> (BuildSlots, PendingOperation, OperationRef) {
    use super::super::tests::{HOST, RECIPE, RESOURCES};
    let BuildAdmission::Fresh(slots, worker, receipt) = supervisor
        .controller
        .admit_build(
            build_request(&hex::encode(RECIPE.0)),
            HOST,
            RESOURCES,
            |_, _| None,
        )
        .unwrap()
        .unwrap()
    else {
        panic!("expected fresh build");
    };
    (*slots, worker, receipt)
}

fn build_state(db: &rusqlite::Connection, subject: Id) -> String {
    db.query_row(
        "SELECT state FROM vm_build_intents WHERE id=?",
        [subject],
        |row| row.get(0),
    )
    .unwrap()
}

#[tokio::test]
async fn failed_terminal_write_releases_worker_and_preserves_durable_build() {
    use super::super::tests::{HOST, RECIPE, RESOURCES};
    let root = crate::service::application::tests::private_temp();
    let supervisor = open(root.path());
    let registry = Registry::default();
    let database = root.path().join("activity.sqlite3");
    let db = rusqlite::Connection::open(&database).unwrap();
    db.execute_batch(
        "CREATE TRIGGER reject_terminal BEFORE UPDATE OF status ON activity
         WHEN OLD.status=0 AND NEW.status<>0
         BEGIN SELECT RAISE(ABORT, 'injected terminal write failure'); END;",
    )
    .unwrap();
    let (terminal_result, terminal_attempt) = oneshot::channel();
    let (slots, receipt) = {
        let mut gate = supervisor.lock().unwrap();
        let (slots, worker, receipt) = reserve_build(&supervisor);
        let registration = registry.register(slots.id, ()).unwrap();
        let entries = Registry(registry.0.clone());
        gate.enqueue_test_worker(&supervisor, registration, runtime, move |supervisor, _| {
            assert!(
                entries
                    .with_entry(slots.id, |entry| entry.is_some())
                    .unwrap()
            );
            let result = supervisor.controller.fail_build(&worker, slots, false);
            assert!(
                entries
                    .with_entry(slots.id, |entry| entry.is_some())
                    .unwrap()
            );
            terminal_result
                .send(result.as_ref().unwrap_err().to_string())
                .unwrap();
            result
        })
        .unwrap();
        (slots, receipt)
    };
    supervisor.close();
    assert_eq!(
        supervisor
            .drain_workers(BOUND)
            .await
            .err()
            .unwrap()
            .to_string(),
        "TEST worker completion unconfirmed"
    );
    assert!(
        !supervisor
            .observe_test_worker(&registry, slots.id, |entry| entry.is_some())
            .unwrap()
    );
    let error = terminal_attempt.await.unwrap();
    assert!(error.contains("injected terminal write failure"), "{error}");
    assert_eq!(build_state(&db, slots.id), "pending");
    let BuildAdmission::Existing(replay) = supervisor
        .controller
        .admit_build(
            build_request(&hex::encode(RECIPE.0)),
            HOST,
            RESOURCES,
            |_, _| None,
        )
        .unwrap()
        .unwrap()
    else {
        panic!("the same key must replay the original receipt");
    };
    assert_eq!(replay, receipt);
    db.execute_batch("DROP TRIGGER reject_terminal").unwrap();
    drop(supervisor);
    let _reopened = open(root.path());
    // Missing artifact identities require quarantine, which remains visible
    // to the ordinary sweep as the exact same build cleanup kind.
    assert_eq!(build_state(&db, slots.id), "quarantined");
    assert_eq!(
        crate::store::vm::cleanup_kinds(&db).unwrap(),
        [CleanupKind::Build(slots.id)]
    );
}

struct SweepProbe<'a> {
    supervisor: &'a Supervisor,
    registry: &'a Registry<()>,
    entered: mpsc::Sender<Id>,
    observed: mpsc::Sender<Option<bool>>,
}

impl ReaperRuntime for SweepProbe<'_> {
    fn quiesce(&self, work: &CleanupWork, _: Instant) -> Quiesced {
        self.entered.send(work.kind.subject()).unwrap();
        let live = self
            .supervisor
            .observe_test_worker(self.registry, work.kind.subject(), |entry| entry.is_some())
            .ok();
        self.observed.send(live).unwrap();
        Quiesced::Unconfirmed
    }

    fn graceful_stop(&self, _: &CleanupWork, _: Instant) -> StopDisposition {
        panic!("unconfirmed release cannot stop a worker")
    }

    fn force_reap(&self, _: &CleanupWork, _: Instant) -> StopDisposition {
        panic!("unconfirmed release cannot force-reap a worker")
    }
}

impl ArtifactCustody for SweepProbe<'_> {
    fn reconcile(&self, _: &CleanupWork, _: Instant) -> bool {
        panic!("missing release proof cannot authorize disposal")
    }
}

#[test]
fn sweep_defers_during_rejected_handoff_and_keeps_unfinished_intent() {
    let root = crate::service::application::tests::private_temp();
    let supervisor = open(root.path());
    let registry = Registry::default();
    let mut gate = supervisor.lock().unwrap();
    let (slots, worker, _) = reserve_build(&supervisor);
    // Make the real sweep visit this intent while admission holds the gate.
    supervisor
        .controller
        .fail_build(&worker, slots, false)
        .unwrap();
    let (entered, sweep_entered) = mpsc::channel();
    let (observed, sweep_observed) = mpsc::channel();
    std::thread::scope(|threads| {
        threads.spawn(|| {
            let probe = SweepProbe {
                supervisor: &supervisor,
                registry: &registry,
                entered,
                observed,
            };
            assert!(supervisor.controller.sweep_with(&probe, &probe).unwrap());
        });
        assert_eq!(sweep_entered.recv_timeout(BOUND).unwrap(), slots.id);
        let during_handoff = sweep_observed.recv_timeout(BOUND).unwrap();
        let registration = registry.register(slots.id, ()).unwrap();
        supervisor.fence();
        let rejection = gate.enqueue_test_worker(&supervisor, registration, runtime, |_, _| {
            panic!("rejected work cannot run")
        });
        drop(gate);
        assert_eq!(rejection.err().unwrap().0, SHUTDOWN.0);
        assert_eq!(during_handoff, None, "sweep must defer during the handoff");
        assert!(
            !supervisor
                .observe_test_worker(&registry, slots.id, |entry| entry.is_some())
                .unwrap()
        );
    });
    let db = rusqlite::Connection::open(root.path().join("activity.sqlite3")).unwrap();
    assert_eq!(build_state(&db, slots.id), "quarantined");
    assert_eq!(
        crate::store::vm::cleanup_kinds(&db).unwrap(),
        [CleanupKind::Build(slots.id)]
    );
}

#[test]
fn reaped_worker_failure_closes_queue_and_remains_visible_to_drain() {
    serialized_executor().block_on(async {
        let root = crate::service::application::tests::private_temp();
        let supervisor = open(root.path());
        let registry = Registry::default();
        {
            let mut gate = supervisor.lock().unwrap();
            let registration = registry.register([5; 16], ()).unwrap();
            gate.enqueue_test_worker(&supervisor, registration, runtime, |_, _| {
                anyhow::bail!("injected worker failure");
            })
            .unwrap();
        }
        // The only blocking thread has published the first worker's join
        // result by the time this next item reaches it.
        tokio::task::spawn_blocking(|| {}).await.unwrap();
        let subject = [6; 16];
        {
            let mut gate = supervisor.lock().unwrap();
            let registration = registry.register(subject, ()).unwrap();
            let result = gate.enqueue_test_worker(&supervisor, registration, runtime, |_, _| {
                panic!("fenced worker cannot run")
            });
            assert_eq!(result.unwrap_err().0, SHUTDOWN.0);
        }
        assert!(supervisor.cancel.is_cancelled());
        assert!(
            !registry
                .with_entry(subject, |entry| entry.is_some())
                .unwrap()
        );
        supervisor.close();
        assert_eq!(
            supervisor
                .drain_workers(BOUND)
                .await
                .err()
                .unwrap()
                .to_string(),
            "TEST worker completion unconfirmed"
        );
    });
}

#[test]
fn production_test_admission_cannot_enqueue_a_worker() {
    use super::super::tests::{RECIPE, key, request};
    let root = crate::service::application::tests::private_temp();
    let supervisor = open(root.path());
    let connection = supervisor.connection("alpha".into());
    let result = connection
        .inventory()
        .dispatch(
            request(
                "vm_template_build",
                json!({"recipe_id":hex::encode(RECIPE.0), "request_id":key(1)}),
            ),
            || false,
        )
        .unwrap_err();
    assert_eq!(result.0, "qualification_missing");
    let gate = supervisor.lock().unwrap();
    assert_eq!(supervisor.capability(&gate)["available"], false);
    assert!(gate.test_workers.tasks.is_empty());
    let db = rusqlite::Connection::open(root.path().join("activity.sqlite3")).unwrap();
    let intents: i64 = db
        .query_row("SELECT COUNT(*) FROM vm_build_intents", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(intents, 0);

    drop(gate);
    // Native release is fixture-only, including when a stop is requested
    // for an instance that has no admitted native worker.
    let result = connection.inventory().dispatch(
        request(
            "vm_stop",
            json!({"vm_id":"7358295852574b59a1146cab94dd5451","request_id":key(2)}),
        ),
        || false,
    );
    assert!(result.is_err());
    assert!(supervisor.lock().unwrap().test_workers.tasks.is_empty());
    let releases: i64 = db
        .query_row("SELECT count(*) FROM vm_snapshot_release", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(releases, 0);
}

#[test]
fn orphan_fence_prevents_reacquisition_and_never_overrides_the_current_owner() {
    let root = crate::service::application::tests::private_temp();
    let supervisor = open(root.path());
    let registry = Registry::default();
    let subject = [0x46; 16];
    let owner = registry.register(subject, 1).unwrap();
    assert!(
        !supervisor.with_test_orphan_fence(&registry, subject, &mut || {
            panic!("orphan recovery bypassed the existing owner")
        })
    );
    drop(owner);
    assert!(
        supervisor.with_test_orphan_fence(&registry, subject, &mut || {
            assert!(
                supervisor.gate.try_lock().is_ok(),
                "proof held the dispatch gate"
            );
            assert!(
                registry.register(subject, 2).is_err(),
                "worker reacquired the epoch"
            );
            true
        })
    );
    assert!(registry.register(subject, 3).is_ok());
}
