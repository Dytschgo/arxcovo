use super::*;
use crate::{
    store::vm::backup_owner_fixture,
    vm::{paths::ArtifactPin, runtime::publication::Origin},
};
use arx_guest::{
    compose::routing::RouteTarget,
    protocol::{MAX_FRAME, write_frame},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream, duplex};

async fn read(peer: &mut DuplexStream) -> Req {
    let len = peer.read_u32().await.unwrap() as usize;
    assert!((1..=MAX_FRAME).contains(&len));
    let mut bytes = vec![0; len];
    peer.read_exact(&mut bytes).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

async fn write(peer: &mut DuplexStream, response: Resp) {
    let mut bytes = Vec::new();
    write_frame(&mut bytes, &response).unwrap();
    peer.write_all(&bytes).await.unwrap();
}

async fn hello(peer: &mut DuplexStream, exact_caps: bool) {
    let Req::ComposeHello {
        version,
        challenge,
        generation,
        mut capabilities,
    } = read(peer).await
    else {
        panic!("the verified pipe must negotiate Compose directly");
    };
    if !exact_caps {
        capabilities.reverse();
    }
    write(
        peer,
        Resp::ComposeHelloAck {
            version,
            challenge,
            generation,
            capabilities,
            cleared_staging: 0,
            ambiguous_operations: 0,
        },
    )
    .await;
}

fn origin(generation: Id) -> Origin {
    Origin {
        generation,
        origin_generation: [9; 16],
        boot: [8; 16],
        port: 8443.try_into().unwrap(),
        ca_path: "fixture-ca".into(),
        ca_receipt: ArtifactPin {
            size: 1,
            sha256: [7; 32],
        },
        retired: Default::default(),
    }
}

#[tokio::test]
async fn static_compose_negotiation_and_shared_exact_boot_retirement() {
    let origin = origin([6; 16]);
    let (host, mut peer) = duplex(8192);
    let (_, refused) = tokio::join!(
        hello(&mut peer, false),
        PreparedStaticSessions::negotiate(host, &origin, 1)
    );
    assert!(refused.is_err());
    let (host, mut peer) = duplex(8192);
    assert!(
        PreparedStaticSessions::negotiate(host, &origin, 0)
            .await
            .is_err()
    );
    assert_eq!(
        peer.read_u8().await.unwrap_err().kind(),
        std::io::ErrorKind::UnexpectedEof
    );

    let (host, mut peer) = duplex(8192);
    let (_, sessions) = tokio::join!(
        hello(&mut peer, true),
        PreparedStaticSessions::negotiate(host, &origin, 2)
    );
    let sessions = sessions.unwrap();
    assert_eq!(sessions.lifecycle.generation, origin.generation);
    assert_eq!(sessions.lifecycle.origin_boot, Some(origin.boot));
    assert_eq!(sessions.backup.generation, origin.generation);
    assert_eq!(
        (sessions.lifecycle.wire_boot, sessions.backup.wire_boot),
        (2, 2)
    );
    let cancel = Shutdown::default();
    let mut channel = sessions.channel.shared();
    let mut queued = sessions.channel.shared();
    let pending = async {
        channel
            .exchange(&cancel, |id| Req::ComposeStatus {
                id,
                project_id: "one".into(),
            })
            .await
    };
    let (received, witnessed) = tokio::sync::oneshot::channel();
    let (enqueued, ready) = tokio::sync::oneshot::channel();
    let queued = async {
        witnessed.await.unwrap();
        enqueued.send(()).unwrap();
        queued
            .exchange(&cancel, |_| panic!("retired boot issued a queued write"))
            .await
    };
    let retire = async {
        assert!(matches!(
            read(&mut peer).await,
            Req::ComposeStatus { id: 1, .. }
        ));
        received.send(()).unwrap();
        ready.await.unwrap();
        origin.retired.retire();
    };
    let (pending, queued, ()) = tokio::join!(pending, queued, retire);
    sessions.channel.retire().await;
    assert!(pending.is_err() && queued.is_err());
    assert!(!sessions.lifecycle.is_open());
    assert_eq!(
        peer.read_u8().await.unwrap_err().kind(),
        std::io::ErrorKind::UnexpectedEof
    );
}

#[tokio::test]
async fn static_compose_start_commits_boot_and_sessions_together() -> Result<()> {
    use crate::store::vm::static_vm;
    use crate::vm::{
        Admission, Capacity, ClockReading, Completion, DiskClaim, HostObservation, Operation,
        Request, Resources, Status, VmController, VmPolicy,
    };

    const NOW: i64 = 1_800_000_000_000;
    struct FixedClock;
    impl Clock for FixedClock {
        fn now(&self) -> ClockReading {
            ClockReading {
                utc_ms: NOW,
                monotonic: std::time::Instant::now(),
            }
        }
    }
    let (dir, db, target, generation) = backup_owner_fixture()?;
    db.transact(|tx| {
        tx.execute(
            "UPDATE vm_static_instances SET state=1 WHERE id=?1",
            [target.vm],
        )?;
        Ok(())
    })?;
    drop(db);
    let controller = VmController::open(dir.path(), VmPolicy::default(), FixedClock)?;
    let db = controller.compose_store();
    let request = |nonce, operation| Request {
        principal: "alice",
        vm: target.vm,
        request_id: crate::vm::RequestId {
            key: [nonce; 16],
            issued_at_ms: NOW,
        },
        disk: DiskClaim::default(),
        operation,
    };
    let host_observation = HostObservation {
        owner_idle: Some(true),
        logical_cpus: Some(8),
        available_ram: Some(64 << 30),
        capacity: Some(Capacity {
            free_bytes: 250 << 30,
            managed_bytes: 0,
        }),
    };
    let resources = Resources {
        vcpus: 2,
        guest_ram: 4 << 30,
        overhead_ram: 3 << 30,
    };
    let Admission::Admitted(worker) =
        controller.admit(request(1, Operation::Start), host_observation, resources)?
    else {
        panic!("static start could not be admitted");
    };
    let origin_generation: Id = db.transact(|tx| static_vm::origin_generation(tx, target.vm))?;
    let origin = Origin {
        origin_generation,
        ..origin(generation)
    };
    let host = Host::default();
    let backup_host = crate::vm::compose_backup::BackupHost::default();

    for (wire, proof) in [
        (
            2,
            Origin {
                origin_generation: [7; 16],
                ..origin.clone()
            },
        ),
        (3, origin.clone()),
        (4, origin.clone()),
    ] {
        let (transport, mut peer) = duplex(8192);
        let (_, sessions) = tokio::join!(
            hello(&mut peer, true),
            PreparedStaticSessions::negotiate(transport, &proof, wire)
        );
        let checks = std::cell::Cell::new(0);
        let committed =
            sessions?.activate(&controller, &worker, &host, &backup_host, target.vm, || {
                checks.set(checks.get() + 1);
                wire != 3 || checks.get() == 1
            });
        if wire == 4 {
            assert!(committed.is_ok());
            assert_eq!(host.session(target.vm).unwrap().wire_boot, wire);
            assert_eq!(backup_host.session(target.vm).unwrap().wire_boot, wire);
            assert!(db.transact(|tx| store::live_boot(tx, target.vm, generation, wire))?);
        } else {
            assert!(committed.is_err());
            assert!(host.session(target.vm).is_none() && backup_host.session(target.vm).is_none());
            db.transact(|tx| {
                assert_eq!(
                    static_vm::owned_state(tx, "alice", target.vm)?,
                    Some(crate::vm::static_web::StaticState::Stopped)
                );
                assert!(!store::live_boot(tx, target.vm, generation, wire)?);
                Ok(())
            })?;
        }
    }
    origin.retired.retire();
    assert!(!host.session(target.vm).unwrap().is_open());
    let Admission::Admitted(stop) =
        controller.admit(request(2, Operation::Stop), host_observation, resources)?
    else {
        panic!("static stop could not be admitted");
    };
    assert_eq!(
        controller.finish(&stop, Completion::Succeeded)?,
        Status::Succeeded
    );
    assert!(!db.transact(|tx| store::live_boot(tx, target.vm, generation, 4))?);
    Ok(())
}

#[tokio::test]
async fn static_compose_private_routes_use_the_exact_origin_and_wire_boot() -> Result<()> {
    let (_dir, _db, target, generation) = backup_owner_fixture()?;
    let mut origin = origin(generation);
    let route_watch = Shutdown::default();
    origin.retired = super::super::runtime::publication::Retirement::on_route(route_watch.clone());
    let routes = PrivateRoutes {
        vm: target.vm,
        zone: "owner.invalid".into(),
        routes: ["a", "b"]
            .into_iter()
            .enumerate()
            .map(|(slot, name)| RouteTarget {
                hostname: format!("{name}.owner.invalid"),
                project_id: format!("app-{name}"),
                service: "web".into(),
                project_slot: slot as u8,
                service_index: 0,
                route_slot: slot as u8,
            })
            .collect(),
    };
    routes.validate()?;
    let (host, mut peer) = duplex(8192);
    let (_, sessions) = tokio::join!(
        hello(&mut peer, true),
        PreparedStaticSessions::negotiate(host, &origin, 1)
    );
    let sessions = sessions?;
    assert!(
        sessions
            .install_private_routes([1; 16], &routes, || true)
            .await
            .is_err()
    );
    let mut other_origin = origin.clone();
    other_origin.boot = [5; 16];
    assert!(!sessions.lifecycle.matches_origin(&other_origin));
    assert!(
        sessions
            .install_private_routes(target.vm, &routes, || false)
            .await
            .is_err()
    );
    let readiness =
        super::super::static_web::StaticOriginIdentity::for_generation(origin.origin_generation);
    let peer_route = async {
        let Req::ComposeInstallRoutes {
            id,
            generation,
            readiness_host,
            zone,
            routes: sent,
            private,
        } = read(&mut peer).await
        else {
            panic!("expected only the fixed route installation action");
        };
        assert_eq!(id, 1);
        assert_eq!(generation, 1);
        assert_eq!(readiness_host, readiness.provisioning().server_name());
        assert_eq!(zone, routes.zone);
        assert_eq!(sent, routes.routes);
        assert!(private);
        write(&mut peer, Resp::ComposeRoutesInstalled { id, generation }).await;
    };
    let (result, ()) = tokio::join!(
        sessions.install_private_routes(target.vm, &routes, || true),
        peer_route,
    );
    result?;
    let retire_while_installing = async {
        assert!(matches!(
            read(&mut peer).await,
            Req::ComposeInstallRoutes { private: true, .. }
        ));
        route_watch.cancel();
    };
    let (retired, ()) = tokio::join!(
        sessions.install_private_routes(target.vm, &routes, || true),
        retire_while_installing,
    );
    assert!(retired.is_err());
    assert!(
        sessions
            .install_private_routes(target.vm, &routes, || true)
            .await
            .is_err()
    );
    Ok(())
}
