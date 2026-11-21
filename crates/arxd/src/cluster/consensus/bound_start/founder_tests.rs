#[cfg(not(target_os = "linux"))]
use super::*;
use crate::cluster::consensus::peer_transport::{self, Credentials, Health, TransportOwner};
use crate::cluster::enrollment::NodeId;
use std::sync::Mutex;
use std::time::Duration;

struct Script(Mutex<std::collections::VecDeque<Result<u64, ClockError>>>);

impl CheckedClock for Script {
    fn sample_ms(&self) -> Result<u64, ClockError> {
        self.0
            .try_lock()
            .map_err(|_| ClockError::Unavailable)?
            .pop_front()
            .unwrap()
    }
}

#[test]
fn founder_clock_requires_exactly_four_healthy_samples() {
    let clock = Script(Mutex::new([Ok(1), Ok(2), Ok(3), Ok(4)].into()));
    let gate = ClockBridge::new(&clock);
    for ms in 1..=4 {
        assert!(gate.seal(()).is_err());
        assert_eq!(gate.now_ms(), ms);
    }
    assert_eq!(gate.seal(7), Ok(7));
    assert_eq!(gate.now_ms(), u64::MAX);
    assert!(gate.seal(7).is_err());
}

#[test]
fn founder_clock_latches_each_site_including_raw_success_at_four() {
    for refused_at in 1..=4 {
        for failure in [Err(ClockError::Unavailable), Ok(MAX_TIME_MS + 1)] {
            let clock = Script(Mutex::new(
                (1..=5)
                    .map(|i| if i == refused_at { failure } else { Ok(i) })
                    .collect(),
            ));
            let gate = ClockBridge::new(&clock);
            for i in 1..=4 {
                assert_eq!(gate.now_ms(), if i < refused_at { i } else { u64::MAX });
            }
            assert!(gate.seal("raw success").is_err());
            assert_eq!(clock.0.lock().unwrap().len(), (5 - refused_at) as usize);
        }
    }
}

#[test]
fn founder_clock_contention_and_poison_are_terminal() {
    let clock = Script(Mutex::new([Ok(1)].into()));
    let gate = ClockBridge::new(&clock);
    let held = clock.0.lock().unwrap();
    assert_eq!(gate.now_ms(), u64::MAX);
    drop(held);
    assert_eq!(gate.now_ms(), u64::MAX);
    let poison = std::panic::catch_unwind(|| {
        let _held = clock.0.lock().unwrap();
        panic!("deliberate clock test poison");
    });
    assert!(poison.is_err());
    let gate = ClockBridge::new(&clock);
    assert_eq!(gate.now_ms(), u64::MAX);
    assert!(gate.seal(()).is_err());
}

struct NoDispatch;

impl peer_transport::AppendDispatch for NoDispatch {
    fn append(
        &self,
        _: openraft::raft::AppendEntriesRequest<crate::cluster::consensus::Arx>,
    ) -> peer_transport::DispatchFuture {
        panic!("pending transport dispatched work");
    }
}

fn invalid_credentials() -> Credentials {
    Credentials {
        root: vec![0].into(),
        leaf: vec![0].into(),
        key: rustls::pki_types::PrivatePkcs8KeyDer::from(vec![0]).into(),
        time: std::sync::Arc::new(rustls::time_provider::DefaultTimeProvider),
    }
}

#[tokio::test]
async fn founder_pending_denies_admission_and_spends_failed_attach() {
    use crate::cluster::{
        consensus::{Arx, SmAdapter},
        state_store::{Init, StateStore},
    };
    use openraft::{
        BasicNode,
        network::{NetAppend, RPCOption, RaftNetworkFactory},
        raft::AppendEntriesRequest,
    };
    use std::sync::Arc;

    let cluster = [1; 16];
    let node = NodeId::new(1).unwrap();
    let (mut owner, mut factory) = TransportOwner::pending_founder(cluster, node);
    assert_eq!(owner.health(), Health::Pending);
    let address = "192.0.2.1:7001";
    let mut client = factory.new_client(1, &BasicNode::new(address)).await;
    let result = client
        .append_entries(
            AppendEntriesRequest::<Arx> {
                vote: openraft::Vote::new(1, 1),
                prev_log_id: None,
                entries: Vec::new(),
                leader_commit: None,
            },
            RPCOption::new(Duration::from_secs(1)),
        )
        .await;
    assert_eq!(
        result.unwrap_err().to_string(),
        peer_transport::rpc_error(peer_transport::Refusal::NotReady).to_string()
    );

    let dir = crate::service::application::tests::private_temp();
    let sm = SmAdapter(Arc::new(Mutex::new(
        StateStore::open(
            dir.path(),
            Init {
                cluster,
                first_free: NodeId::new(2).unwrap(),
            },
        )
        .unwrap(),
    )));
    for refusal in [
        peer_transport::Refusal::Tls,
        peer_transport::Refusal::NotReady,
    ] {
        assert_eq!(
            owner.attach_founder(&sm, Arc::new(NoDispatch), address, invalid_credentials()),
            Err(refusal)
        );
    }
    assert_eq!(owner.health(), Health::Attaching);
    owner.close().await.unwrap();
    assert!(owner.joined());
    assert_eq!(
        owner.attach_founder(&sm, Arc::new(NoDispatch), address, invalid_credentials()),
        Err(peer_transport::Refusal::Closed)
    );
}

#[cfg(target_os = "linux")]
mod hosted {
    use super::*;
    use crate::cluster::{join_receipt::PublicIdentity, startup_plan::tests::Layout};
    use std::{
        fs::{self, File, OpenOptions, TryLockError},
        net::{SocketAddr, TcpListener, UdpSocket},
        os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
        path::Path,
    };

    const LOCKS: [&str; 3] = [
        "cluster-owner.lock",
        "cluster-state.lock",
        "cluster-log.lock",
    ];

    fn address() -> SocketAddr {
        // UDP connect chooses this runner's non-loopback interface without
        // sending traffic. The TCP observation connects only to this fixture.
        let route = UdpSocket::bind("0.0.0.0:0").unwrap();
        route.connect("192.0.2.1:9").unwrap();
        let listener =
            TcpListener::bind(SocketAddr::new(route.local_addr().unwrap().ip(), 0)).unwrap();
        listener.local_addr().unwrap()
    }

    fn fixture(layout: &Layout, address: SocketAddr) -> (Plan, StartIntent, PathBuf, PathBuf) {
        let mut plan = layout.plan("create-founder");
        plan.peer = address.to_string();
        let intent = StartIntent {
            mode: Mode::CreateFounder,
            cluster: [1; 16],
            node: FOUNDER,
            peer: address,
            enrollment: plan.enrollment.parse().unwrap(),
        };
        let [wrap, store] = ["wrap", "custody"].map(|name| {
            let path = layout.root.path().join(name);
            fs::create_dir(&path).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
            path
        });
        (plan, intent, wrap, store)
    }

    fn guard(file: &File) -> (u64, u64) {
        let meta = file.metadata().unwrap();
        assert_eq!(meta.nlink(), 1);
        (meta.dev(), meta.ino())
    }

    fn probe(dir: &Path, held: bool) -> [(u64, u64); 3] {
        LOCKS.map(|name| {
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .custom_flags(libc::O_NOFOLLOW)
                .open(dir.join(name))
                .unwrap();
            if held {
                assert!(
                    matches!(file.try_lock(), Err(TryLockError::WouldBlock)),
                    "{name}"
                );
            } else {
                file.try_lock().unwrap();
                file.unlock().unwrap();
            }
            guard(&file)
        })
    }

    #[test]
    fn founder_reservation_refusal_preserves_original_owner_and_files() {
        for collision in [true, false] {
            let layout = Layout::new();
            let (plan, intent, wrap, store) = fixture(&layout, "192.0.2.1:7001".parse().unwrap());
            let state_dir = plan.state.clone();
            let reservation = plan.credentials.join("identity.json");
            let (transport, _) = TransportOwner::pending_founder(intent.cluster, FOUNDER);
            let job = Job::anchor();
            job.prepare(plan, &intent, None);
            assert_eq!(job.state().status, Status::Prepared);
            let ids = probe(&state_dir, true);
            if collision {
                layout.write("credentials/identity.json", b"occupied");
            } else {
                // Failure in the next effect after actual durable reservation.
                fs::remove_dir(&wrap).unwrap();
            }
            job.state().founder = Some(Operation {
                wrap,
                store,
                custody: None,
                slot: None,
                history: None,
                transport: Arc::new(tokio::sync::Mutex::new(transport)),
                fixture: None,
            });
            assert!(prepare(job, &intent).is_none());
            let state = job.state();
            let expected = if collision {
                Error::Publication(issuance_publication::Error::Layout)
            } else {
                Error::Custody
            };
            assert_eq!(state.founder_failure, Some(expected));
            assert_eq!(state.status, Status::FounderRefused);
            assert!(state.owner.is_some() && state.node.is_none() && !state.engine_may_exist);
            drop(state);
            assert_eq!(probe(&state_dir, true), ids);
            let bytes: &[u8] = if collision { b"occupied" } else { b"" };
            assert_eq!(fs::read(reservation).unwrap(), bytes);
            assert!(job.begin_close());
            job.finish_close(None, true, false);
            assert_eq!(job.state().status, Status::ClosedAfterFounderRefusal);
            assert_eq!(probe(&state_dir, false), ids);
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn bound_start_founder_publishes_attaches_and_closes() {
        let layout = Layout::new();
        let endpoint = address();
        let (plan, intent, wrap, store) = fixture(&layout, endpoint);
        let state_dir = plan.state.clone();
        let (job, mut observer, supervisor) = dispatch_founder(
            plan,
            intent,
            Params::default(),
            wrap,
            store,
            HostedFixture(()),
        )
        .unwrap();

        let live = tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                match observer.status() {
                    Status::FounderReady => break,
                    Status::Preparing
                    | Status::Prepared
                    | Status::EngineMayExist
                    | Status::Running
                    | Status::Founding => {}
                    status => {
                        return Err(format!(
                            "founder stopped in {status:?}: {:?}",
                            job.state().founder_failure
                        ));
                    }
                }
                observer.changed().await.ok_or("observer closed")?;
            }
            let (transport, node, receipt, root, ids) = {
                let state = job.state();
                let owner = state.owner.as_ref().ok_or("missing retained owner")?;
                owner.publication_originals().map_err(|_| "originals")?;
                let record = PublicIdentity::decode(
                    &owner
                        .read("identity.json", 8192)
                        .map_err(|_| "receipt read")?,
                )
                .map_err(|_| "receipt decode")?;
                let transport = state
                    .founder
                    .as_ref()
                    .ok_or("missing transport owner")?
                    .transport
                    .clone();
                let ids = LOCKS.map(|name| guard(&owner.bound_guard(name).unwrap()));
                let node = state.node.as_ref().ok_or("missing retained node")?.clone();
                let root = owner.read("ca.der", 4096).map_err(|_| "root read")?;
                (transport, node, record, root, ids)
            };
            assert_eq!(probe(&state_dir, true), ids);
            let expected = {
                let node = node.lock().await;
                let log = node.log_weak.upgrade().ok_or("missing live log")?;
                let entries = log
                    .lock()
                    .unwrap()
                    .entries(0, 16, 16)
                    .map_err(|_| "live log read")?;
                let mut found = entries.into_iter().filter_map(|(e, entry)| {
                    let StoredEntry::Enrollment(Command::Found {
                        cluster, members, ..
                    }) = entry
                    else {
                        return None;
                    };
                    let &[(node, key)] = members.as_slice() else {
                        return None;
                    };
                    Some(ExpectedIdentity {
                        cluster,
                        node,
                        key,
                        ca: crate::cluster::pki::ca::fingerprint_of(&root),
                        origin: Origin::Found {
                            term: e.term,
                            leader: e.leader,
                            index: e.index,
                        },
                    })
                });
                let result = found.next().ok_or("missing original Found commit")?;
                if found.next().is_some() {
                    return Err("extra Found commit".into());
                }
                result
            };
            receipt
                .verify(&expected, &root, UnixTime::now())
                .map_err(|_| "receipt did not preserve original Found commit")?;
            let (client, server_authenticated) = transport
                .lock()
                .await
                .founder_tls_probe()
                .map_err(|_| "TLS setup")?;
            let tcp = tokio::net::TcpStream::connect(endpoint)
                .await
                .map_err(|_| "TCP")?;
            let name =
                rustls::pki_types::ServerName::try_from(crate::cluster::pki::verify::SERVER_NAME)
                    .map_err(|_| "TLS name")?;
            let tls = tokio_rustls::TlsConnector::from(client)
                .connect(name, tcp)
                .await
                .map_err(|_| "TLS handshake")?;
            if !server_authenticated.await {
                return Err("server did not authenticate member possession".into());
            }
            if tls
                .get_ref()
                .1
                .peer_certificates()
                .and_then(|certs| certs.first())
                .map(AsRef::as_ref)
                != Some(receipt.leaf())
            {
                return Err("published leaf did not match live transport".into());
            }
            drop(tls);
            Ok(ids)
        })
        .await;
        if !job.state().close_requested {
            drop(job.request_close());
        }
        tokio::time::timeout(Duration::from_secs(10), supervisor)
            .await
            .expect("owned close exceeded the test wait")
            .expect("supervisor join");
        let ids = live
            .expect("founder live wait elapsed")
            .unwrap_or_else(|reason| panic!("{reason}"));
        {
            let state = job.state();
            assert_eq!(
                state.status,
                Status::Clean,
                "shutdown: {:?}",
                state.close_failure
            );
            assert!(state.close_requested && state.close_dispatched && state.engine_may_exist);
            assert!(state.node.is_none() && state.owner.is_none() && state.founder.is_none());
        }

        assert_eq!(probe(&state_dir, false), ids);
        // Listener release is observed independently of the transport health.
        let error = tokio::net::TcpStream::connect(endpoint).await.unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::ConnectionRefused);
    }
}
