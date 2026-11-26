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
        sessions.channel.retire().await;
    };
    let (pending, queued, ()) = tokio::join!(pending, queued, retire);
    assert!(pending.is_err() && queued.is_err());
    assert!(!sessions.lifecycle.is_open());
    assert_eq!(
        peer.read_u8().await.unwrap_err().kind(),
        std::io::ErrorKind::UnexpectedEof
    );
}

#[tokio::test]
async fn static_compose_private_routes_use_the_exact_origin_and_wire_boot() -> Result<()> {
    let (_dir, db, target, generation) = backup_owner_fixture()?;
    let mut origin = origin(generation);
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
            .lifecycle
            .install_private_routes(&db, [1; 16], &origin, &routes)
            .await
            .is_err()
    );
    let origin_boot = origin.boot;
    origin.boot = [5; 16];
    assert!(
        sessions
            .lifecycle
            .install_private_routes(&db, target.vm, &origin, &routes)
            .await
            .is_err()
    );
    origin.boot = origin_boot;
    let readiness =
        super::super::static_web::StaticOriginIdentity::for_generation(origin.origin_generation);
    let peer_route = async {
        let Req::ComposeInstallRoutes {
            id,
            generation,
            readiness_host,
            zone,
            routes: sent,
        } = read(&mut peer).await
        else {
            panic!("expected only the fixed route installation action");
        };
        assert_eq!(id, 1);
        assert_eq!(generation, 1);
        assert_eq!(readiness_host, readiness.provisioning().server_name());
        assert_eq!(zone, routes.zone);
        assert_eq!(sent, routes.routes);
        write(&mut peer, Resp::ComposeRoutesInstalled { id, generation }).await;
    };
    let (result, ()) = tokio::join!(
        sessions
            .lifecycle
            .install_private_routes(&db, target.vm, &origin, &routes),
        peer_route,
    );
    result?;
    origin.retired.retire();
    assert!(
        sessions
            .lifecycle
            .install_private_routes(&db, target.vm, &origin, &routes)
            .await
            .is_err()
    );
    Ok(())
}
