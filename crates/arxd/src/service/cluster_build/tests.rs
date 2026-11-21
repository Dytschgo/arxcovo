use super::*;
use crate::{
    development::Cancellation,
    service::executor::{Executor, Report, Request},
    store::Activity,
};
use std::{
    os::unix::fs::PermissionsExt,
    sync::atomic::{AtomicUsize, Ordering},
};

struct Backend {
    starts: AtomicUsize,
    release: std::sync::Mutex<Option<std::sync::mpsc::Receiver<()>>>,
}

impl Executor for Backend {
    fn preflight(&self) -> Result<(), super::super::preflight::ExecutionUnavailable> {
        Ok(())
    }

    fn run(&self, _: Option<&Request>, _: &Cancellation, _: &Cancellation) -> Report {
        self.starts.fetch_add(1, Ordering::SeqCst);
        if let Some(wait) = self.release.lock().unwrap().take() {
            assert!(wait.recv_timeout(Duration::from_secs(5)).is_ok());
        }
        Report::Unavailable(super::super::preflight::ExecutionUnavailable::PreflightFailed)
    }
}

fn fixture(
    block: bool,
) -> (
    tempfile::TempDir,
    Arc<Activity>,
    Arc<Execution>,
    Arc<Backend>,
    std::sync::mpsc::Sender<()>,
) {
    let temp = tempfile::tempdir().unwrap();
    std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let activity = Arc::new(Activity::open(temp.path()).unwrap());
    let (send, receive) = std::sync::mpsc::channel();
    let backend = Arc::new(Backend {
        starts: AtomicUsize::new(0),
        release: std::sync::Mutex::new(block.then_some(receive)),
    });
    let execution = Execution::cluster_broker(backend.clone(), 21002, 21002, 1);
    (temp, activity, execution, backend, send)
}

fn command() -> Request {
    Request {
        tool: Some("cargo".into()),
        executable: None,
        args: vec!["--version".into()],
        cwd: String::new(),
        timeout_s: 1,
        output_bytes: 128,
    }
}

#[tokio::test]
async fn ordinary_and_delegated_capacity_compete_both_ways() {
    let (_root, activity, execution, backend, release) = fixture(true);
    let id = activity.start_dedicated("fixture", "run_command").unwrap();
    let local = execution.dispatch(Some(command()), activity.clone(), id);
    tokio::time::timeout(Duration::from_secs(5), async {
        while backend.starts.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(matches!(
        execution.reserve_build(),
        Err(super::super::preflight::ExecutionUnavailable::Busy)
    ));
    release.send(()).unwrap();
    assert!(local.await.is_err());
    let reservation = execution.reserve_build().ok().unwrap();
    let id = activity.start_dedicated("fixture", "run_command").unwrap();
    let local = execution.dispatch(Some(command()), activity, id).await;
    assert!(local.err().is_some_and(
        |err| err.execution_reason == Some(super::super::preflight::ExecutionUnavailable::Busy)
    ));
    assert_eq!(backend.starts.load(Ordering::SeqCst), 1);
    drop(reservation);
    execution.drain().await.unwrap();
}

#[tokio::test]
async fn unconfirmed_activity_commit_releases_capacity_without_dispatch() {
    let (_root, activity, execution, backend, _release) = fixture(false);
    for visible in [false, true] {
        let reserve = execution.reserve_build().ok().unwrap();
        let result = Execution::dispatch_build(
            reserve,
            activity.clone(),
            command(),
            |activity| {
                if visible {
                    activity.start_delegated_build([1; 32], [2; 32], None)?;
                }
                anyhow::bail!("injected activity refusal");
            },
            |_, _| panic!("unconfirmed terminal transaction dispatched"),
        );
        assert!(matches!(result, Err(BuildAdmissionError::Storage)));
    }
    assert_eq!(backend.starts.load(Ordering::SeqCst), 0);
    assert_eq!(
        activity
            .delegated_build([1; 32], [2; 32], false)
            .unwrap()
            .unwrap()
            .outcome,
        Outcome::Started
    );
    assert!(execution.reserve_build().is_ok());
    execution.drain().await.unwrap();
}

#[tokio::test]
async fn sealed_worker_refuses_a_held_reservation_before_storage() {
    let (_root, activity, execution, backend, _release) = fixture(false);
    let reserve = execution.reserve_build().ok().unwrap();
    execution.drain().await.unwrap();
    let result = Execution::dispatch_build(
        reserve,
        activity,
        command(),
        |_| panic!("stopped before persistence"),
        |_, _| panic!("stopped before execution"),
    );
    assert!(matches!(result, Err(BuildAdmissionError::Unavailable(_))));
    assert_eq!(backend.starts.load(Ordering::SeqCst), 0);
}

#[test]
fn single_claim_refuses_mutation_wrong_scope_and_expiry() {
    let registry = Leases::default();
    let source = NodeId::new(1).unwrap();
    let make = |expires| Lease {
        expires,
        source,
        scope: [1; 32],
        descriptor: [2; 32],
        secret: [3; 32],
        ticket: [4; 32],
    };
    assert!(registry.insert([5; 32], make(Instant::now() + LEASE_TTL)));
    assert!(registry.take([5; 32], [9; 32], [3; 32], [2; 32]).is_none());
    assert!(registry.take([5; 32], [1; 32], [3; 32], [9; 32]).is_none());
    assert!(registry.take([5; 32], [1; 32], [9; 32], [2; 32]).is_none());
    std::thread::scope(|s| {
        let a = s.spawn(|| registry.take([5; 32], [1; 32], [3; 32], [2; 32]).is_some());
        let b = s.spawn(|| registry.take([5; 32], [1; 32], [3; 32], [2; 32]).is_some());
        assert_ne!(a.join().unwrap(), b.join().unwrap());
    });
    assert!(registry.insert([6; 32], make(Instant::now())));
    assert!(registry.take([6; 32], [1; 32], [3; 32], [2; 32]).is_none());
    assert!(
        Leases::default()
            .take([5; 32], [1; 32], [3; 32], [2; 32])
            .is_none()
    );
}
