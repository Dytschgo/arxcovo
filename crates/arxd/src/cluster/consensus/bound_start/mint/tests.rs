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

#[cfg(target_os = "linux")]
mod proposal_boundary {
    use super::*;
    use crate::service::{
        Shutdown,
        cluster_owner::{DispatchReply, MintReceipt, Paste, Reply},
    };
    use linux::ResultMode;
    use std::{cell::Cell, future::pending, task::Poll, time::Duration};
    use tokio::time::{Instant, advance};

    const WAIT: Duration = Duration::from_secs(1);

    fn receipt() -> DispatchReply {
        DispatchReply::MintReceipt(MintReceipt {
            position: MINT,
            digest: [3; 32],
        })
    }

    #[tokio::test(start_paused = true)]
    async fn stopped_or_expired_view_never_submits() {
        for expire in [false, true] {
            let stop = Shutdown::default();
            let proposals = Cell::new(0);
            let start = Instant::now();
            let mode = ResultMode::receipt(&stop, start + WAIT);
            let mut mint = std::pin::pin!(mode.at_proposal(pending::<Option<()>>(), |()| async {
                proposals.set(proposals.get() + 1);
                receipt()
            }));
            std::future::poll_fn(|cx| {
                assert!(mint.as_mut().poll(cx).is_pending());
                Poll::Ready(())
            })
            .await;
            if expire {
                advance(WAIT).await;
            } else {
                stop.cancel();
            }
            assert!(matches!(mint.await, DispatchReply::Control(Reply::Refused)));
            assert_eq!(proposals.get(), 0);
            if !expire {
                assert_eq!(Instant::now(), start);
            }
        }
    }

    #[tokio::test(start_paused = true)]
    async fn stop_or_expiry_as_view_finishes_never_submits() {
        for expire in [false, true] {
            let stop = Shutdown::default();
            let deadline = Instant::now() + WAIT;
            let proposals = Cell::new(0);
            let reply = ResultMode::receipt(&stop, deadline)
                .at_proposal(
                    std::future::poll_fn(|cx| {
                        if expire {
                            // advance changes the clock before yielding to the timer driver.
                            assert!(std::pin::pin!(advance(WAIT)).poll(cx).is_pending());
                            assert_eq!(Instant::now(), deadline);
                        } else {
                            stop.cancel();
                        }
                        Poll::Ready(Some(()))
                    }),
                    |()| async {
                        proposals.set(proposals.get() + 1);
                        receipt()
                    },
                )
                .await;
            assert!(matches!(reply, DispatchReply::Control(Reply::Refused)));
            assert_eq!(proposals.get(), 0);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn stop_or_expiry_after_submission_returns_unknown() {
        for expire in [false, true] {
            let stop = Shutdown::default();
            let proposals = Cell::new(0);
            let start = Instant::now();
            let mode = ResultMode::receipt(&stop, start + WAIT);
            let mut mint = std::pin::pin!(mode.at_proposal(async { Some(()) }, |()| async {
                proposals.set(proposals.get() + 1);
                // Make a receipt available in the deadline poll or leave
                // a pending receipt waiter for lifecycle stop to cancel.
                if expire {
                    tokio::time::sleep(WAIT).await;
                } else {
                    pending::<()>().await;
                }
                receipt()
            }));
            std::future::poll_fn(|cx| {
                assert!(mint.as_mut().poll(cx).is_pending());
                Poll::Ready(())
            })
            .await;
            assert_eq!(proposals.get(), 1);
            if expire {
                advance(WAIT).await;
            } else {
                stop.cancel();
            }
            assert!(matches!(mint.await, DispatchReply::Control(Reply::Unknown)));
            assert_eq!(proposals.get(), 1);
            if !expire {
                assert_eq!(Instant::now(), start);
            }
        }
    }

    #[tokio::test(start_paused = true)]
    async fn stopped_completion_cannot_release_a_ready_receipt() {
        let stop = Shutdown::default();
        let proposals = Cell::new(0);
        let mode = ResultMode::receipt(&stop, Instant::now() + WAIT);
        let reply = mode
            .at_proposal(async { Some(()) }, |()| async {
                proposals.set(proposals.get() + 1);
                stop.cancel();
                receipt()
            })
            .await;
        assert_eq!(proposals.get(), 1);
        assert!(matches!(reply, DispatchReply::Control(Reply::Unknown)));
    }

    #[tokio::test(start_paused = true)]
    async fn timely_private_receipt_and_ordinary_paste_are_delivered() {
        let stop = Shutdown::default();
        let deadline = Instant::now() + WAIT;
        let mode = ResultMode::receipt(&stop, deadline);
        let reply = mode
            .at_proposal(async { Some(()) }, |()| async { receipt() })
            .await;
        assert!(matches!(reply, DispatchReply::MintReceipt(receipt) if receipt.position == MINT));

        let proposals = Cell::new(0);
        let reply = ResultMode::Paste
            .at_proposal(
                async {
                    stop.cancel();
                    advance(WAIT).await;
                    Some(())
                },
                |()| async {
                    assert!(stop.is_cancelled() && Instant::now() >= deadline);
                    proposals.set(proposals.get() + 1);
                    linux::mint_result(Paste::copy(b"ordinary paste").unwrap(), None, MINT, EXPIRES)
                },
            )
            .await;
        assert_eq!(proposals.get(), 1);
        assert!(matches!(reply, DispatchReply::Control(Reply::Minted(paste))
            if paste.bytes() == b"ordinary paste"));
    }
}

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
            Some(Outcome::Rejected(
                crate::cluster::enrollment::Reject::CodeIdCollision
            )),
        ),
        None,
    );
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

#[cfg(target_os = "linux")]
#[test]
fn mint_result_modes_only_release_the_requested_representation() {
    use crate::{
        cluster::enrollment::{Command, JoinCode, MintReceiptInput},
        service::cluster_owner::{DispatchReply, Paste, Reply},
    };
    let code = JoinCode::generate([1; 16], [2; 32], anchor(9), |buffer| {
        buffer.fill(7);
        Ok::<_, ()>(())
    })
    .unwrap();
    let mint = Command::Mint {
        cluster: [1; 16],
        actor: [3; 16],
        request: [4; 16],
        code: code.code_id(),
        verifier: code.verifier(),
        proposed_at_ms: 100,
    };
    let input = MintReceiptInput::new(&mint);
    let protected = || Paste::copy(b"test-only protected bytes").unwrap();
    let result = linux::mint_result(protected(), Some(&input), MINT, EXPIRES);
    let DispatchReply::MintReceipt(receipt) = result else {
        panic!("receipt mode returned a different representation");
    };
    assert_eq!(receipt.position, MINT);
    assert_eq!(Some(receipt.digest), input.digest(MINT, EXPIRES));
    assert_ne!(Some(receipt.digest), input.digest(FLOOR, EXPIRES));
    let result = linux::mint_result(protected(), None, MINT, EXPIRES);
    assert!(
        matches!(result, DispatchReply::Control(Reply::Minted(paste))
        if paste.bytes() == b"test-only protected bytes")
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
