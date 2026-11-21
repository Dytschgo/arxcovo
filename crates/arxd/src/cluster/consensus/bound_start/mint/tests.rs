use super::*;
use crate::cluster::enrollment::{BoundVoters, SKEW_MS};

fn anchor(key: u8) -> JoinAnchor {
    let mut bytes = vec![1];
    bytes.extend(2_u64.to_be_bytes());
    bytes.extend([key; 32]);
    BoundVoters::decode(&bytes).unwrap().anchor([1; 16])
}

fn view() -> View {
    View {
        term: 4,
        leader: 2,
        membership: LogPos {
            term: 3,
            leader: 1,
            index: 7,
        },
        anchor: anchor(9),
        root: [8; 32],
        endpoint: "192.168.0.1:9000".parse().unwrap(),
        listener: 12,
    }
}

const FLOOR: LogPos = LogPos {
    term: 4,
    leader: 2,
    index: 10,
};
const MINT: LogPos = LogPos { index: 11, ..FLOOR };
const EXPIRES: u64 = 900_100;

#[test]
fn owner_mint_accepts_only_first_result_from_exact_epoch_above_floor() {
    let outcome = || {
        Some(Outcome::Minted {
            replay: false,
            expires_ms: EXPIRES,
        })
    };
    assert_eq!(first_expiry(view(), FLOOR, MINT, outcome()), Some(EXPIRES));
    for position in [
        FLOOR,
        LogPos { term: 5, ..MINT },
        LogPos { leader: 1, ..MINT },
    ] {
        assert_eq!(first_expiry(view(), FLOOR, position, outcome()), None);
    }
    assert_eq!(first_expiry(view(), FLOOR, MINT, None), None);
    assert_eq!(
        first_expiry(
            view(),
            FLOOR,
            MINT,
            Some(Outcome::Minted {
                replay: true,
                expires_ms: EXPIRES
            })
        ),
        None
    );
}

#[test]
fn owner_mint_conceals_on_view_change_or_unusable_expiry() {
    let before = view();
    let now = EXPIRES - 2 * SKEW_MS - 1;
    assert!(may_reveal(before, before, MINT, MINT, now, EXPIRES));
    for after in [
        View { term: 5, ..before },
        View {
            leader: 3,
            ..before
        },
        View {
            membership: FLOOR,
            ..before
        },
        View {
            anchor: anchor(1),
            ..before
        },
        View {
            root: [1; 32],
            ..before
        },
        View {
            endpoint: "192.168.0.1:9001".parse().unwrap(),
            ..before
        },
        View {
            listener: 13,
            ..before
        },
    ] {
        assert!(!may_reveal(before, after, MINT, MINT, now, EXPIRES));
    }
    assert!(!may_reveal(before, before, FLOOR, MINT, now, EXPIRES));
    assert!(!may_reveal(
        before,
        before,
        LogPos { leader: 1, ..MINT },
        MINT,
        now,
        EXPIRES
    ));
    assert!(!may_reveal(before, before, MINT, MINT, now + 1, EXPIRES));
    assert!(!may_reveal(before, before, MINT, MINT, u64::MAX, EXPIRES));
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn enrollment_listener_witness_cannot_follow_replacement() {
    use super::super::enrollment_ingress::ServingListener;
    let old = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = old.local_addr().unwrap();
    let original = ServingListener::capture(&old, address).unwrap();
    assert!(original.active());
    assert!(ServingListener::capture(&old, "127.0.0.1:0".parse().unwrap()).is_none());
    drop(old);
    let new = tokio::net::TcpListener::bind(address).await.unwrap();
    let replacement = ServingListener::capture(&new, address).unwrap();
    assert!(replacement.active());
    assert_ne!(original.cookie, replacement.cookie);
    assert!(!original.active());
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn mutually_authenticated_peer_cannot_dispatch_owner_mint_or_remove() {
    use super::super::super::{
        Arx, SmAdapter, peer_registry,
        peer_transport::{
            AppendDispatch, Credentials, DispatchFuture, Event, Stage, TransportOwner,
        },
    };
    use crate::cluster::{
        enrollment::{Command, FoundMembers, KeyDigest, NodeId},
        pki::{self, verify},
        state_store::{Entry, Init, StateStore},
    };
    use rcgen::{
        BasicConstraints, CertificateParams, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
        KeyUsagePurpose, PublicKeyData, SanType,
    };
    use rustls::pki_types::{PrivatePkcs8KeyDer, ServerName};
    use std::{
        sync::{Arc, Mutex},
        time::Duration,
    };
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream},
        time::timeout,
    };
    use tokio_rustls::TlsConnector;

    struct DenyDispatch;
    impl AppendDispatch for DenyDispatch {
        fn append(&self, _: openraft::raft::AppendEntriesRequest<Arx>) -> DispatchFuture {
            panic!("owner request reached peer dispatch")
        }
    }

    const CLUSTER: [u8; 16] = [2; 16];
    let time = Arc::new(pki::FixedTime(1_800_000_000));
    let a = NodeId::new(1).unwrap();
    let b = NodeId::new(2).unwrap();
    let directory = crate::service::application::tests::private_temp();
    let store = SmAdapter(Arc::new(Mutex::new(
        StateStore::open(
            directory.path(),
            Init {
                cluster: CLUSTER,
                first_free: NodeId::new(3).unwrap(),
            },
        )
        .unwrap(),
    )));
    let ca_key = KeyPair::generate().unwrap();
    let mut ca = CertificateParams::default();
    ca.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
    ca.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    let root = ca.self_signed(&ca_key).unwrap().der().clone();
    let issuer = Issuer::new(ca, ca_key);
    let issue = |node| {
        let key = KeyPair::generate().unwrap();
        let mut params = CertificateParams::default();
        params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        params.subject_alt_names = vec![SanType::URI(
            pki::node_uri(CLUSTER, node).try_into().unwrap(),
        )];
        params.extended_key_usages = vec![
            ExtendedKeyUsagePurpose::ServerAuth,
            ExtendedKeyUsagePurpose::ClientAuth,
        ];
        let leaf = params.signed_by(&key, &issuer).unwrap().der().clone();
        let digest = KeyDigest::of_spki(&key.subject_public_key_info()).unwrap();
        (
            leaf,
            PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
            digest,
        )
    };
    let (server_cert, server_key, server_spki) = issue(a);
    let (client_cert, client_key, client_spki) = issue(b);
    store
        .0
        .lock()
        .unwrap()
        .apply(
            LogPos {
                term: 1,
                leader: 1,
                index: 0,
            },
            &Entry::Enrollment(Command::Found {
                cluster: CLUSTER,
                members: FoundMembers::new([(a, server_spki), (b, client_spki)]).unwrap(),
                proposed_at_ms: 100,
            }),
        )
        .unwrap();
    let verifier =
        verify::PeerVerifier::new(root.clone(), CLUSTER, peer_registry::registry(&store)).unwrap();
    let client = TlsConnector::from(Arc::new(
        verify::client_config(verifier.client(a), client_cert, client_key, time.clone()).unwrap(),
    ));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let mut peer = TransportOwner::new(
        CLUSTER,
        a,
        &store,
        Credentials {
            root,
            leaf: server_cert,
            key: server_key,
            time,
        },
        Arc::new(DenyDispatch),
    )
    .unwrap();
    let mut stages = peer.dispatch_observer();
    peer.listen_on(listener).unwrap();
    timeout(Duration::from_secs(5), async {
        for opcode in [1, 3] {
            let socket = TcpStream::connect(address).await.unwrap();
            let mut session = client
                .connect(ServerName::try_from("arxcovo.invalid").unwrap(), socket)
                .await
                .unwrap();
            loop {
                if matches!(
                    stages.recv().await.unwrap(),
                    Event::Stage(Stage::ServerAuthenticated)
                ) {
                    break;
                }
            }
            let mut request = [0; 48];
            request[..5].copy_from_slice(&[b'A', b'R', b'X', b'O', opcode]);
            session.write_all(&request).await.unwrap();
            session.shutdown().await.unwrap();
            let mut reply = [0; 1];
            assert!(matches!(session.read(&mut reply).await, Ok(0) | Err(_)));
        }
    })
    .await
    .unwrap();
    peer.close().await.unwrap();
    while let Ok(event) = stages.try_recv() {
        assert!(!matches!(event, Event::Stage(Stage::BeforeDispatch)));
    }
}
