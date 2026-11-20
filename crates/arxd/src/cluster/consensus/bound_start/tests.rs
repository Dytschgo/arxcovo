use super::*;
use crate::cluster::{
    bound_open::{BoundFailure, BoundIntent, BoundStores},
    enrollment::{Command, FoundMembers, NodeId},
    join_receipt::{Origin, tests::fixture},
    startup_plan::{Mode, tests::Layout},
    state_store::{Entry, LogPos},
};
fn intent(l: &Layout, mode: &str) -> StartIntent {
    let p = l.plan(mode);
    StartIntent {
        mode: p.mode,
        cluster: [1; 16],
        node: NodeId::new(1).unwrap(),
        peer: p.peer.parse().unwrap(),
        enrollment: p.enrollment.parse().unwrap(),
    }
}
fn init() -> Init {
    Init {
        cluster: [1; 16],
        first_free: NodeId::new(2).unwrap(),
    }
}
fn lock(l: &Layout, name: &str, held: bool) {
    let f = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(l.root.path().join(format!("state/{name}.lock")))
        .unwrap();
    assert_eq!(f.try_lock().is_err(), held, "{name}");
}
fn dispose(job: &Job) {
    job.request_close();
    assert!(job.begin_close());
    job.finish_close(None, true, false);
    assert_eq!(job.state().status, Status::Clean);
}
#[test]
fn fresh_transfer_preserves_connections_and_original_locks() {
    let l = Layout::new();
    let job = Job::anchor();
    job.prepare(
        l.plan("create-founder"),
        &intent(&l, "create-founder"),
        None,
    );
    assert_eq!(job.state().status, Status::Prepared);
    let guards = crate::cluster::startup_plan::tests::guards(job.state().owner.as_ref().unwrap());
    let before = job.state().stores.as_ref().unwrap().pointers();
    job.transfer().unwrap();
    let pointers = {
        let s = job.state();
        let (log, sm) = s.adapters.as_ref().unwrap();
        (
            log.0.lock().unwrap().bound_pointer(),
            sm.0.lock().unwrap().bound_pointer(),
        )
    };
    assert_eq!(before, pointers);
    assert_ne!(pointers.0, 0);
    assert_ne!(pointers.1, 0);
    assert_ne!(pointers.0, pointers.1);
    assert!(job.transfer().is_err());
    assert_eq!(
        guards,
        crate::cluster::startup_plan::tests::guards(job.state().owner.as_ref().unwrap())
    );
    for name in ["cluster-owner", "cluster-state", "cluster-log"] {
        lock(&l, name, true);
    }
    dispose(job);
    for name in ["cluster-owner", "cluster-state", "cluster-log"] {
        lock(&l, name, false);
    }
}
#[test]
fn untouched_and_authenticated_recovery_transfer() {
    for recovered in [false, true] {
        let l = Layout::new();
        l.write("state/cluster-owner.lock", b"");
        let dir = l.root.path().join("state");
        drop(StateStore::open(&dir, init()).unwrap());
        drop(VoteLogStore::open(&dir, init().cluster).unwrap());
        let expected = if recovered {
            let (record, root, e) = fixture(
                1,
                Origin::Found {
                    term: 1,
                    leader: 1,
                    index: 0,
                },
            );
            l.write("credentials/identity.json", &record);
            l.write("credentials/ca.der", &root);
            l.write("credentials/sealed-key", b"opaque");
            let entry = Entry::Enrollment(Command::Found {
                cluster: [1; 16],
                members: FoundMembers::new([(e.node, e.key)]).unwrap(),
                proposed_at_ms: 1,
            });
            let p = e.origin.position();
            StateStore::open(&dir, init())
                .unwrap()
                .apply(p, &entry)
                .unwrap();
            VoteLogStore::open(&dir, init().cluster)
                .unwrap()
                .append(&[(p, entry)])
                .unwrap();
            Some(e)
        } else {
            None
        };
        let mode = if recovered {
            "recover-founder"
        } else {
            "create-founder"
        };
        let job = Job::anchor();
        job.prepare(l.plan(mode), &intent(&l, mode), expected.as_ref());
        assert_eq!(job.state().status, Status::Prepared);
        job.transfer().unwrap();
        dispose(job);
    }
}
#[test]
fn terminal_inspect_and_partial_transfer_refuse() {
    let l = Layout::new();
    let mut owner = Owner::acquire(l.plan("create-founder")).unwrap();
    let mut stores = BoundStores::acquire(&mut owner, BoundIntent::Fresh(init())).into_owned();
    assert!(stores.take_pair().is_ok());
    assert!(stores.take_pair().is_err());
    drop(stores);
    drop(owner);
    let mut owner = Owner::acquire(l.plan("create-founder")).unwrap();
    let mut inspected =
        BoundStores::acquire(&mut owner, BoundIntent::Inspect([1; 16])).into_owned();
    assert!(matches!(
        inspected.take_pair(),
        Err(BoundFailure::Unavailable)
    ));
    drop(inspected);
    let mut retry =
        BoundStores::acquire(&mut owner, BoundIntent::UntouchedGenesis(init())).into_owned();
    assert!(matches!(retry.take_pair(), Err(BoundFailure::Unavailable)));
    drop(retry);
    drop(owner);
    // A real second lock refusal retains the first writable store.
    let l = Layout::new();
    l.write("state/cluster-owner.lock", b"");
    drop(StateStore::open(&l.root.path().join("state"), init()).unwrap());
    drop(VoteLogStore::open(&l.root.path().join("state"), init().cluster).unwrap());
    let second = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(l.root.path().join("state/cluster-log.lock"))
        .unwrap();
    second.try_lock().unwrap();
    let mut owner = Owner::acquire(l.plan("create-founder")).unwrap();
    let mut partial =
        BoundStores::acquire(&mut owner, BoundIntent::UntouchedGenesis(init())).into_owned();
    assert!(partial.take_pair().is_err());
    lock(&l, "cluster-state", true);
    drop(partial);
    lock(&l, "cluster-state", false);
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn detached_observer_retains_paused_real_preparation() {
    let l = Layout::new();
    let job = Job::anchor();
    let entered = Arc::new(std::sync::Barrier::new(2));
    let release = Arc::new(std::sync::Barrier::new(2));
    *job.gate.lock().unwrap() = Some((entered.clone(), release.clone()));
    let observer = job.observe();
    let plan = l.plan("create-founder");
    let i = intent(&l, "create-founder");
    let worker = tokio::task::spawn_blocking(move || job.prepare(plan, &i, None));
    entered.wait();
    drop(observer);
    for name in ["cluster-owner", "cluster-state", "cluster-log"] {
        lock(&l, name, true);
    }
    release.wait();
    worker.await.unwrap();
    assert_eq!(job.observe().status(), Status::Prepared);
    assert_eq!(
        job.with_retained_owner::<()>(|_| panic!("pre-Engine loan")),
        Err(Refusal::Ownership)
    );
    job.transfer().unwrap();
    drop(job.observe()); // stored completion is independent of notification
    dispose(job);
}
#[test]
fn single_close_dispatch_and_sticky_consumed_node_uncertainty() {
    let job = Job::anchor();
    job.set(Status::EngineMayExist);
    drop(job.request_close());
    drop(job.request_close());
    assert!(job.begin_close());
    assert!(!job.begin_close());
    job.finish_close(None, false, false); // consumed/lost node is not join proof
    job.finish_close(None, true, false); // synthetic absent success cannot clear it
    assert_eq!(job.state().status, Status::Unconfirmed);
    assert!(!job.begin_close());
    let l = Layout::new();
    let job = Job::anchor();
    job.prepare(
        l.plan("create-founder"),
        &intent(&l, "create-founder"),
        None,
    );
    job.set(Status::EngineMayExist);
    assert!(job.begin_close());
    job.finish_close(None, true, false);
    assert_eq!(job.state().status, Status::Unconfirmed);
    lock(&l, "cluster-owner", true);
    // Test-only deliberate cleanup after assertions: no Engine ever executed.
    let mut s = job.state();
    s.stores = None;
    s.owner = None;
}
#[test]
fn incompatible_full_log_position_refuses_before_transfer() {
    let l = Layout::new();
    l.write("state/cluster-owner.lock", b"");
    let dir = l.root.path().join("state");
    let a = LogPos {
        term: 1,
        leader: 1,
        index: 0,
    };
    StateStore::open(&dir, init())
        .unwrap()
        .apply(a, &Entry::Blank)
        .unwrap();
    VoteLogStore::open(&dir, [1; 16])
        .unwrap()
        .append(&[(LogPos { term: 2, ..a }, Entry::Blank)])
        .unwrap();
    let job = Job::anchor();
    job.prepare(
        l.plan("create-founder"),
        &intent(&l, "create-founder"),
        None,
    );
    assert_eq!(job.state().status, Status::Refused);
    assert!(job.transfer().is_err());
    assert_eq!(intent(&l, "create-founder").mode, Mode::CreateFounder);
    dispose(job);
}

#[test]
fn cross_process_lock_probe() {
    let Some(path) = std::env::var_os("ARX_STEP5A_PROBE") else {
        return;
    };
    let f = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .unwrap();
    let held = std::env::var("ARX_STEP5A_HELD").unwrap() == "true";
    assert_eq!(f.try_lock().is_err(), held);
}
#[test]
fn retained_outer_and_store_locks_exclude_other_process_until_disposal() {
    let l = Layout::new();
    let job = Job::anchor();
    job.prepare(
        l.plan("create-founder"),
        &intent(&l, "create-founder"),
        None,
    );
    for held in [true, false] {
        for name in ["cluster-owner", "cluster-state", "cluster-log"] {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "cluster::consensus::bound_start::tests::cross_process_lock_probe",
                ])
                .env(
                    "ARX_STEP5A_PROBE",
                    l.root.path().join(format!("state/{name}.lock")),
                )
                .env("ARX_STEP5A_HELD", held.to_string())
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stdout)
            );
        }
        if held {
            dispose(job);
        }
    }
}
#[test]
fn first_failed_close_result_survives_repeated_clean_claim() {
    let job = Job::anchor();
    job.set(Status::EngineMayExist);
    job.request_close();
    assert!(job.begin_close());
    let report = ShutdownReport {
        first: Phase::Engine,
        engine: EngineEnd::JoinFailed,
        engine_ms: 0,
        owners_done_at_start: 0,
        owners_waited: 0,
        owners_left: 0,
        owners_panicked: 0,
        last_owner_join_ms: 0,
        propose_held: 0,
        read_held: 0,
        log_strong: 0,
        sm_strong: 0,
        total_ms: 0,
    };
    job.finish_close(Some(report), false, true);
    drop(job.request_close());
    job.finish_close(None, true, true);
    assert_eq!(job.state().close_failure, Some(report));
    assert_eq!(job.state().status, Status::Unconfirmed);
    assert!(!job.begin_close());
}
