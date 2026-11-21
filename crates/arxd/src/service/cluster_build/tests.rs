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

#[tokio::test]
async fn commit_racing_shutdown_has_one_terminal_row_and_no_escaped_run() {
    let (_root, activity, execution, backend, _release) = fixture(false);
    let reserve = execution.reserve_build().ok().unwrap();
    let (committed, recorded) = tokio::sync::oneshot::channel();
    let (release, wait) = std::sync::mpsc::channel();
    let owned_activity = activity.clone();
    let worker = tokio::task::spawn_blocking(move || {
        Execution::dispatch_build(
            reserve,
            owned_activity,
            command(),
            move |activity| {
                let id = activity.start_delegated_build([8; 32], [9; 32], None)?;
                committed.send(id).unwrap();
                wait.recv_timeout(Duration::from_secs(5)).unwrap();
                Ok(Some(id))
            },
            |_, _| panic!("a worker entered the prepared broker after shutdown"),
        )
    });
    let id = tokio::time::timeout(Duration::from_secs(5), recorded)
        .await
        .unwrap()
        .unwrap();
    execution.shutdown();
    release.send(()).unwrap();
    let (dispatched_id, status) = worker.await.unwrap().ok().unwrap();
    assert_eq!(dispatched_id, id);
    assert!(status.await.is_err());
    execution.drain().await.unwrap();
    assert_eq!(backend.starts.load(Ordering::SeqCst), 0);
    let result = activity
        .delegated_build([8; 32], [9; 32], false)
        .unwrap()
        .unwrap();
    assert_eq!(result.activity_id, id);
    assert_eq!(result.outcome, Outcome::Refused);
    assert!(
        activity
            .start_delegated_build([8; 32], [9; 32], None)
            .is_err()
    );
}

#[tokio::test]
async fn unconfigured_worker_has_no_delegated_listener() {
    super::super::cluster_runtime::confirm_build_opt_out().await;
}

#[tokio::test]
#[ignore = "non-root QEMU protected UID fixture"]
async fn protected_agent_endpoint_checks_kernel_identity_group_and_pins() {
    let root = std::path::Path::new("/d4-fixture");
    super::super::cluster_build_executor::workspace_controls(root);
    crate::local::endpoint_controls(root).await;
}

struct Applied {
    revoked: std::sync::atomic::AtomicBool,
    target_key: std::sync::atomic::AtomicU8,
}

impl Authority for Applied {
    fn identity(&self) -> Option<([u8; 16], NodeId)> {
        Some(([8; 16], node(2)))
    }
    fn binding(&self, cluster: [u8; 16], id: NodeId) -> Option<[u8; 32]> {
        if cluster != [8; 16] || id == node(1) && self.revoked.load(Ordering::SeqCst) {
            return None;
        }
        if id == node(2) {
            return Some([self.target_key.load(Ordering::SeqCst); 32]);
        }
        Some([id.get() as u8; 32])
    }
    fn client(&self, _: NodeId) -> Option<Arc<rustls::ClientConfig>> {
        None
    }
    fn server(&self) -> Option<Arc<rustls::ServerConfig>> {
        None
    }
}

fn node(value: u64) -> NodeId {
    NodeId::new(value).unwrap()
}

fn descriptor(workspace: &str) -> Descriptor {
    let request = command();
    Descriptor {
        workspace: workspace.into(),
        tool: request.tool.unwrap(),
        args: request.args,
        timeout_s: request.timeout_s,
        output_bytes: request.output_bytes,
    }
}

async fn ask(
    service: &Arc<BuildService>,
    authority: &Arc<Applied>,
    source: u64,
    query: Query,
) -> Response {
    let key = [source as u8; 32];
    service
        .handle(
            authority.clone(),
            Identity {
                cluster: [8; 16],
                node: node(source),
                key,
            },
            Envelope {
                version: VERSION,
                cluster: [8; 16],
                source: node(source),
                target: node(2),
                agent: "fixture".into(),
                query,
            },
        )
        .await
}

fn spend(operation: Operation, lease: Secret, workspace: &str) -> Query {
    Query::Spend {
        operation,
        lease,
        descriptor: descriptor(workspace).offer().unwrap(),
    }
}

fn test_service(
    activity: Arc<Activity>,
    execution: Arc<Execution>,
    broker: BuildBroker,
) -> (
    Arc<BuildService>,
    Arc<Applied>,
    Arc<std::sync::atomic::AtomicBool>,
) {
    let applied = Arc::new(Applied {
        revoked: false.into(),
        target_key: 2.into(),
    });
    let policy = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let current = policy.clone();
    let service = Arc::new(BuildService {
        activity,
        execution: Some(execution),
        broker: Some(broker),
        grants: vec![
            Grant {
                source: node(1),
                key: [1; 32],
                agent: "fixture".into(),
                commands: vec![descriptor("bin")],
            },
            Grant {
                source: node(3),
                key: [3; 32],
                agent: "fixture".into(),
                commands: vec![descriptor("lib")],
            },
        ],
        routes: BTreeMap::new(),
        agents: BTreeMap::new(),
        authority: RwLock::new(None),
        policy_current: Arc::new(move || current.load(Ordering::SeqCst)),
        leases: Leases::default(),
        stop: Shutdown::default(),
    });
    (service, applied, policy)
}

#[tokio::test]
async fn stale_authority_under_admission_refuses_before_writes_or_execute() {
    use super::super::cluster_build_executor::refusal_fixture;
    let (temp, activity, execution, backend, _) = fixture(false);
    let starts = Arc::new(AtomicUsize::new(0));
    let (broker, _, task) = refusal_fixture(temp.path(), "/usr".into(), Some(starts.clone()));
    let (service, authority, policy) = test_service(activity, execution.clone(), broker);
    let db = rusqlite::Connection::open(temp.path().join("activity.sqlite3")).unwrap();
    let changes: [&dyn Fn(); 3] = [
        &|| authority.revoked.store(true, Ordering::SeqCst),
        &|| policy.store(false, Ordering::SeqCst),
        &|| authority.target_key.store(9, Ordering::SeqCst),
    ];
    for change in changes {
        let offer = descriptor("bin").offer().unwrap();
        let Response::Granted { operation, lease } =
            ask(&service, &authority, 1, Query::Grant(offer)).await
        else {
            panic!("fresh grant refused");
        };
        let (entered, barrier) = tokio::sync::oneshot::channel();
        let entered = Mutex::new(Some(entered));
        let (release, wait) = std::sync::mpsc::channel();
        let wait = Mutex::new(wait);
        execution.admission_barrier_for_test(Arc::new(move || {
            entered.lock().unwrap().take().unwrap().send(()).unwrap();
            wait.lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
        }));
        let owner = service.clone();
        let applied = authority.clone();
        let request =
            tokio::spawn(
                async move { ask(&owner, &applied, 1, spend(operation, lease, "bin")).await },
            );
        tokio::time::timeout(Duration::from_secs(5), barrier)
            .await
            .unwrap()
            .unwrap();
        change();
        release.send(()).unwrap();
        assert!(matches!(request.await.unwrap(), Response::Refused));
        assert_eq!(starts.load(Ordering::SeqCst), 0);
        assert_eq!(backend.starts.load(Ordering::SeqCst), 0);
        for sql in [
            "SELECT count(*) FROM activity",
            "SELECT count(*) FROM delegated_build",
        ] {
            assert_eq!(db.query_row::<u64, _, _>(sql, [], |r| r.get(0)).unwrap(), 0);
        }
        authority.revoked.store(false, Ordering::SeqCst);
        policy.store(true, Ordering::SeqCst);
        authority.target_key.store(2, Ordering::SeqCst);
    }
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    execution.drain().await.unwrap();
}

#[tokio::test]
async fn broker_busy_drain_and_revocation_refuse_before_activity() {
    use super::super::cluster_build_executor::refusal_fixture;
    let (temp, activity, execution, backend, _) = fixture(false);
    let (broker, capacity, task) = refusal_fixture(temp.path(), "/usr".into(), None);
    let (service, applied, policy) = test_service(activity, execution.clone(), broker);
    let grant = |slot| Query::Grant(descriptor(slot).offer().unwrap());
    assert!(matches!(
        ask(&service, &applied, 3, grant("lib")).await,
        Response::Granted { .. }
    ));
    assert!(matches!(
        ask(&service, &applied, 1, grant("lib")).await,
        Response::Refused
    ));
    let Response::Granted { operation, lease } = ask(&service, &applied, 1, grant("bin")).await
    else {
        panic!("B-local slot failed its no-spawn check")
    };
    assert!(matches!(
        ask(&service, &applied, 1, spend(operation, lease, "lib")).await,
        Response::Refused
    ));
    policy.store(false, Ordering::SeqCst);
    assert!(matches!(
        ask(&service, &applied, 1, spend(operation, lease, "bin")).await,
        Response::Refused
    ));
    policy.store(true, Ordering::SeqCst);
    applied.revoked.store(true, Ordering::SeqCst);
    assert!(matches!(
        ask(&service, &applied, 1, spend(operation, lease, "bin")).await,
        Response::Refused
    ));
    applied.revoked.store(false, Ordering::SeqCst);
    let held = capacity.clone().try_acquire_owned().unwrap();
    assert!(matches!(
        ask(&service, &applied, 1, spend(operation, lease, "bin")).await,
        Response::Refused
    ));
    assert!(matches!(
        ask(&service, &applied, 1, Query::Status { operation }).await,
        Response::Refused
    ));
    drop(held);

    let Response::Granted { operation, lease } = ask(&service, &applied, 1, grant("bin")).await
    else {
        panic!("mirror capacity did not reopen")
    };
    capacity.close();
    assert!(matches!(
        ask(&service, &applied, 1, spend(operation, lease, "bin")).await,
        Response::Refused
    ));
    assert!(matches!(
        ask(&service, &applied, 1, Query::Status { operation }).await,
        Response::Refused
    ));
    assert_eq!(backend.starts.load(Ordering::SeqCst), 0);
    let db = rusqlite::Connection::open(temp.path().join("activity.sqlite3")).unwrap();
    assert_eq!(
        db.query_row::<u64, _, _>("SELECT count(*) FROM activity", [], |r| r.get(0))
            .unwrap(),
        0
    );
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    execution.drain().await.unwrap();
}

#[tokio::test]
async fn authenticated_http_inventory_cannot_submit_an_explicit_target() {
    use rmcp::{
        ServiceExt,
        model::CallToolRequestParams,
        transport::{
            StreamableHttpClientTransport,
            streamable_http_client::StreamableHttpClientTransportConfig,
        },
    };
    let (root, activity, _, backend, _) = fixture(false);
    let token = "a".repeat(64);
    let config = Arc::new(crate::config::Config {
        agents: vec![crate::config::Agent {
            principal: "fixture".into(),
            token_sha256: hex::encode(Sha256::digest(token.as_bytes())),
        }],
        state_dir: root.path().into(),
        ..Default::default()
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = crate::http::router(config, address, activity);
    let stop = Shutdown::default();
    let closed = stop.clone();
    let server = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async move { closed.cancelled().await })
            .await
            .unwrap();
    });
    let transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(format!("http://{address}/mcp"))
            .auth_header(token),
    );
    let client = tokio::time::timeout(Duration::from_secs(5), ().serve(transport))
        .await
        .unwrap()
        .unwrap();
    let tools = client.list_tools(None).await.unwrap();
    assert!(
        tools
            .tools
            .iter()
            .all(|t| t.name != "delegated_build_status")
    );
    let arguments = json!({"target":2,"workspace":"bin","tool":"cargo","args":["--version"]});
    let request = CallToolRequestParams::new("run_command")
        .with_arguments(arguments.as_object().unwrap().clone());
    assert!(client.call_tool(request).await.is_err());
    assert_eq!(backend.starts.load(Ordering::SeqCst), 0);
    let db = rusqlite::Connection::open(root.path().join("activity.sqlite3")).unwrap();
    assert_eq!(
        db.query_row::<u64, _, _>("SELECT count(*) FROM delegated_build", [], |r| r.get(0))
            .unwrap(),
        0
    );
    let _ = client.cancel().await;
    stop.cancel();
    tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .unwrap()
        .unwrap();
}
