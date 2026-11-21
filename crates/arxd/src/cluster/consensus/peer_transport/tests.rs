use super::super::{Members, StoredVote, id, vote};
use super::*;
use crate::cluster::{
    enrollment::{Command, FoundMembers, KeyDigest, NodeId, Outcome},
    pki::{self, verify},
    state_store::{Entry as StoredEntry, Init, LogPos, StateStore},
};
use crate::service::application::tests::private_temp;
use openraft::network::NetAppend;
use openraft::type_config::async_runtime::watch::WatchReceiver;
use openraft::{
    Entry, EntryPayload,
    network::RPCOption,
    raft::{AppendEntriesRequest, AppendError, AppendResult},
};
use rcgen::{
    BasicConstraints, CertificateParams, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose, PublicKeyData, SanType,
};
use rustls::{
    ClientConfig, ServerConfig,
    pki_types::{CertificateDer, PrivatePkcs8KeyDer, ServerName},
};
use std::{
    collections::BTreeMap,
    io,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{mpsc, oneshot},
    time::timeout,
};
use tokio_rustls::{TlsAcceptor, TlsConnector};

const CLUSTER: [u8; 16] = [1; 16];
const OBSERVE: Duration = Duration::from_secs(10);
fn node(n: u64) -> NodeId {
    NodeId::new(n).unwrap()
}
fn log_id(term: u64, leader: u64, index: u64) -> Id {
    id(LogPos {
        term,
        leader,
        index,
    })
}
fn route() -> peer_wire::Route {
    peer_wire::Route {
        cluster: CLUSTER,
        source: 1,
        target: 2,
    }
}
fn reverse() -> peer_wire::Route {
    peer_wire::Route {
        cluster: CLUSTER,
        source: 2,
        target: 1,
    }
}
fn heartbeat() -> AppendEntriesRequest<Arx> {
    AppendEntriesRequest {
        vote: vote(StoredVote {
            term: 7,
            node: 1,
            committed: true,
        }),
        prev_log_id: None,
        leader_commit: None,
        entries: vec![],
    }
}
fn entries() -> AppendEntriesRequest<Arx> {
    let mut request = heartbeat();
    request.entries = vec![
        Entry {
            log_id: log_id(2, 9, 0),
            payload: EntryPayload::Blank,
        },
        Entry {
            log_id: log_id(7, 1, 1),
            payload: EntryPayload::Normal(Command::Remove {
                cluster: CLUSTER,
                node: node(2),
                proposed_at_ms: 102,
            }),
        },
    ];
    request
}
struct Identity {
    cert: CertificateDer<'static>,
    key: KeyPair,
}
impl Identity {
    fn digest(&self) -> KeyDigest {
        KeyDigest::of_spki(&self.key.subject_public_key_info()).unwrap()
    }
    fn private_key(&self) -> rustls::pki_types::PrivateKeyDer<'static> {
        PrivatePkcs8KeyDer::from(self.key.serialize_der()).into()
    }
}
struct Fixture {
    _dir: tempfile::TempDir,
    sm: SmAdapter,
    root: CertificateDer<'static>,
    issuer: Issuer<'static, KeyPair>,
    client: Identity,
    server: Identity,
}
impl Fixture {
    fn new() -> Self {
        let fixture = Self::unseeded();
        let applied = fixture
            .sm
            .0
            .lock()
            .unwrap()
            .apply(
                LogPos {
                    term: 1,
                    leader: 1,
                    index: 0,
                },
                &StoredEntry::Enrollment(Command::Found {
                    cluster: CLUSTER,
                    members: FoundMembers::new([
                        (node(1), fixture.client.digest()),
                        (node(2), fixture.server.digest()),
                    ])
                    .unwrap(),
                    proposed_at_ms: 100,
                }),
            )
            .unwrap();
        assert!(matches!(applied, Some(Outcome::Founded { replay: false })));
        fixture
    }

    fn unseeded() -> Self {
        let mut p = CertificateParams::default();
        p.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
        p.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        p.not_before = time::OffsetDateTime::from_unix_timestamp(800).unwrap();
        p.not_after = time::OffsetDateTime::from_unix_timestamp(20000).unwrap();
        let key = KeyPair::generate().unwrap();
        let root = p.self_signed(&key).unwrap().der().clone();
        let issuer = Issuer::new(p, key);
        let client = Self::issue(&issuer, CLUSTER, 1, 940, 10000);
        let server = Self::issue(&issuer, CLUSTER, 2, 940, 10000);
        let dir = private_temp();
        let store = StateStore::open(
            dir.path(),
            Init {
                cluster: CLUSTER,
                first_free: node(3),
            },
        )
        .unwrap();
        Self {
            _dir: dir,
            sm: SmAdapter(Arc::new(Mutex::new(store))),
            root,
            issuer,
            client,
            server,
        }
    }
    fn issue(
        issuer: &Issuer<'static, KeyPair>,
        cluster: [u8; 16],
        n: u64,
        not_before: i64,
        not_after: i64,
    ) -> Identity {
        let key = KeyPair::generate().unwrap();
        Self::sign(issuer, cluster, n, key, not_before, not_after)
    }
    fn reissue(
        issuer: &Issuer<'static, KeyPair>,
        cluster: [u8; 16],
        n: u64,
        identity: &Identity,
        not_before: i64,
        not_after: i64,
    ) -> Identity {
        Self::sign(
            issuer,
            cluster,
            n,
            KeyPair::try_from(identity.key.serialize_der()).unwrap(),
            not_before,
            not_after,
        )
    }
    fn sign(
        issuer: &Issuer<'static, KeyPair>,
        cluster: [u8; 16],
        n: u64,
        key: KeyPair,
        not_before: i64,
        not_after: i64,
    ) -> Identity {
        let mut p = CertificateParams::default();
        p.not_before = time::OffsetDateTime::from_unix_timestamp(not_before).unwrap();
        p.not_after = time::OffsetDateTime::from_unix_timestamp(not_after).unwrap();
        p.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        p.extended_key_usages = vec![
            ExtendedKeyUsagePurpose::ClientAuth,
            ExtendedKeyUsagePurpose::ServerAuth,
        ];
        p.subject_alt_names = vec![SanType::URI(
            pki::node_uri(cluster, node(n)).try_into().unwrap(),
        )];
        Identity {
            cert: p.signed_by(&key, issuer).unwrap().der().clone(),
            key,
        }
    }
    fn verifier(&self) -> verify::PeerVerifier {
        verify::PeerVerifier::new(
            self.root.clone(),
            CLUSTER,
            peer_registry::registry(&self.sm),
        )
        .unwrap()
    }
    fn client_config(&self, identity: &Identity, expected: u64) -> Arc<ClientConfig> {
        Arc::new(
            verify::client_config(
                self.verifier().client(node(expected)),
                identity.cert.clone(),
                identity.private_key(),
                Arc::new(pki::FixedTime(1001)),
            )
            .unwrap(),
        )
    }
    fn server_config(&self, identity: &Identity) -> Arc<ServerConfig> {
        Arc::new(
            verify::server_config(
                self.verifier().server(),
                identity.cert.clone(),
                identity.private_key(),
                Arc::new(pki::FixedTime(1001)),
            )
            .unwrap(),
        )
    }
    fn remove(&self, n: u64) -> bool {
        let Ok(mut store) = self.sm.0.lock() else {
            return false;
        };
        matches!(
            store.apply(
                LogPos {
                    term: 1,
                    leader: 1,
                    index: 1
                },
                &StoredEntry::Enrollment(Command::Remove {
                    cluster: CLUSTER,
                    node: node(n),
                    proposed_at_ms: 101,
                }),
            ),
            Ok(Some(Outcome::Removed { replay: false }))
        )
    }
}
async fn connect(
    config: Arc<ClientConfig>,
    addr: std::net::SocketAddr,
) -> io::Result<tokio_rustls::client::TlsStream<TcpStream>> {
    let tcp = TcpStream::connect(addr).await?;
    TlsConnector::from(config)
        .connect_with(
            ServerName::try_from(verify::SERVER_NAME).unwrap(),
            tcp,
            |c| c.set_buffer_limit(Some(65536)),
        )
        .await
}
async fn read_frame(stream: &mut (impl AsyncRead + Unpin)) -> io::Result<Vec<u8>> {
    let mut header = [0u8; 44];
    stream.read_exact(&mut header).await?;
    let inspected = peer_wire::inspect_header(&header)
        .map_err(|_| io::Error::other("invalid test peer header"))?;
    let mut bytes = header.to_vec();
    bytes.resize(44 + inspected.payload_len, 0);
    stream.read_exact(&mut bytes[44..]).await?;
    Ok(bytes)
}
async fn clean_eof(stream: &mut (impl AsyncRead + Unpin)) -> io::Result<()> {
    let mut byte = [0u8; 1];
    match stream.read(&mut byte).await? {
        0 => Ok(()),
        _ => Err(io::Error::other("extra test peer payload")),
    }
}
async fn send_frame(
    stream: &mut (impl AsyncWrite + Unpin),
    bytes: &[u8],
    fragmented: bool,
) -> io::Result<()> {
    if fragmented {
        for part in bytes.chunks(13) {
            stream.write_all(part).await?;
            stream.flush().await?;
        }
    } else {
        stream.write_all(bytes).await?;
    }
    stream.flush().await?;
    stream.shutdown().await
}

impl Fixture {
    fn owner(&self, n: u64, dispatch: Arc<dyn AppendDispatch>) -> TransportOwner {
        let identity = if n == 1 { &self.client } else { &self.server };
        self.owner_with(n, identity, dispatch)
    }
    fn owner_with(
        &self,
        n: u64,
        identity: &Identity,
        dispatch: Arc<dyn AppendDispatch>,
    ) -> TransportOwner {
        TransportOwner::new(
            CLUSTER,
            node(n),
            &self.sm,
            Credentials {
                root: self.root.clone(),
                leaf: identity.cert.clone(),
                key: identity.private_key(),
                time: Arc::new(pki::FixedTime(1001)),
            },
            dispatch,
        )
        .unwrap()
    }
}
struct Spy {
    calls: Mutex<Vec<Vec<u8>>>,
    answer: Mutex<Option<AppendResult<Arx>>>,
    unavailable: bool,
    panic: bool,
    entered: Mutex<Option<oneshot::Sender<()>>>,
    gate: Option<Arc<Semaphore>>,
}
impl Spy {
    // None is an explicit unavailable dispatcher for refusal-only fixtures.
    // Positive fixtures always provide the exact semantic result in advance.
    fn exact(answer: Option<AppendResult<Arx>>) -> Arc<Self> {
        Arc::new(Self {
            calls: Mutex::new(vec![]),
            answer: Mutex::new(answer),
            unavailable: false,
            panic: false,
            entered: Mutex::new(None),
            gate: None,
        })
    }
    fn count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }
}
impl AppendDispatch for Spy {
    fn append(&self, request: AppendEntriesRequest<Arx>) -> DispatchFuture {
        self.calls
            .lock()
            .unwrap()
            .push(peer_wire::encode_request(route(), &request).unwrap());
        if let Some(entered) = self.entered.lock().unwrap().take() {
            let _ = entered.send(());
        }
        let answer = self.answer.lock().unwrap().clone();
        let unavailable = self.unavailable;
        let panic = self.panic;
        let gate = self.gate.clone();
        Box::pin(async move {
            if let Some(gate) = gate {
                let _permit = gate.acquire().await;
            }
            assert!(!panic, "deliberate transport dispatcher panic");
            if unavailable {
                Err(Refusal::Dispatch)
            } else {
                answer.ok_or(Refusal::Dispatch)
            }
        })
    }
}
fn events(owner: &TransportOwner) -> mpsc::Receiver<Event> {
    let (tx, rx) = mpsc::channel(512);
    *owner.shared.hooks.events.lock().unwrap() = Some(tx);
    rx
}
fn gate(owner: &TransportOwner, phase: Stage) -> Arc<Semaphore> {
    let gate = Arc::new(Semaphore::new(0));
    owner
        .shared
        .hooks
        .gates
        .lock()
        .unwrap()
        .insert(phase, gate.clone());
    gate
}
fn release(owner: &TransportOwner, phase: Stage, gate: &Semaphore) {
    owner.shared.hooks.gates.lock().unwrap().remove(&phase);
    gate.add_permits(64);
}
async fn observe(rx: &mut mpsc::Receiver<Event>, wanted: Event) -> bool {
    timeout(OBSERVE, async {
        while let Some(event) = rx.recv().await {
            if event == wanted {
                return true;
            }
        }
        false
    })
    .await
    .unwrap_or(false)
}
async fn listening(owner: &mut TransportOwner) -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    owner.listen_on(listener).unwrap();
    address
}
fn reclaimed(owner: &TransportOwner) -> bool {
    let state = owner.shared.state.lock().unwrap();
    state.finished
        && state.tasks.is_empty()
        && state.join_errors.is_empty()
        && state.accept.is_none()
        && state.resources.is_none()
        && state.keeper.is_none()
        && owner.shared.connections.available_permits() == 32
        && owner.shared.handshakes.available_permits() == 4
}
async fn bounded_close(owner: &mut TransportOwner) -> Result<(), Refusal> {
    timeout(Duration::from_secs(15), owner.close())
        .await
        .unwrap_or(Err(Refusal::Unclean))
}
fn independent_heartbeat() -> Vec<u8> {
    // Independent protocol fields, never the production request encoder.
    let mut payload = Vec::new();
    payload.extend(7u64.to_be_bytes());
    payload.extend(1u64.to_be_bytes());
    payload.extend([1, 0, 0, 0]);
    let mut bytes = b"ARXP\x02\x01\0\0".to_vec();
    bytes.extend((payload.len() as u32).to_be_bytes());
    bytes.extend(CLUSTER);
    bytes.extend(1u64.to_be_bytes());
    bytes.extend(2u64.to_be_bytes());
    bytes.extend(payload);
    bytes
}
async fn raw_request(
    f: &Fixture,
    owner: &mut TransportOwner,
    bytes: &[u8],
    fragmented: bool,
) -> io::Result<Vec<u8>> {
    let address = listening(owner).await;
    let exchange = async {
        let mut stream = connect(f.client_config(&f.client, 2), address).await?;
        send_frame(&mut stream, bytes, fragmented).await?;
        let reply = read_frame(&mut stream).await?;
        clean_eof(&mut stream).await?;
        Ok(reply)
    };
    timeout(OBSERVE, exchange)
        .await
        .unwrap_or_else(|_| Err(io::Error::other("test observation expired")))
}
#[tokio::test]
async fn actual_tls_half_close_independent_heartbeat_fragmentation_and_exact_entries() {
    for (request, fragmented, independent) in [(heartbeat(), false, true), (entries(), true, false)]
    {
        let f = Fixture::new();
        let spy = Spy::exact(Some(Ok(request
            .entries
            .last()
            .map(|e| e.log_id)
            .or(request.prev_log_id))));
        let mut owner = f.owner(2, spy.clone());
        let bytes = if independent {
            independent_heartbeat()
        } else {
            peer_wire::encode_request(route(), &request).unwrap()
        };
        let reply = raw_request(&f, &mut owner, &bytes, fragmented).await;
        let closed = bounded_close(&mut owner).await;
        let recovered = reclaimed(&owner);
        assert!(closed.is_ok() && recovered);
        let reply = reply.unwrap();
        let (r, result) = peer_wire::decode_reply(&reply).unwrap();
        assert!(r == reverse());
        assert_eq!(
            result,
            Ok(request
                .entries
                .last()
                .map(|e| e.log_id)
                .or(request.prev_log_id))
        );
        assert_eq!(
            spy.calls.lock().unwrap().as_slice(),
            &[peer_wire::encode_request(route(), &request).unwrap()]
        );
    }
}
#[tokio::test]
async fn largest_canonical_membership_request_crosses_send_buffer() {
    let f = Fixture::new();
    let spy = Spy::exact(Some(Ok(Some(log_id(7, 1, 64)))));
    let mut owner = f.owner(2, spy.clone());
    let members = Members::new(
        vec![(1..=16).collect(), (1..=16).collect()],
        (1..=16)
            .map(|n| (n, openraft::BasicNode::new("a".repeat(200))))
            .collect::<BTreeMap<_, _>>(),
    )
    .unwrap();
    let mut request = heartbeat();
    request.prev_log_id = Some(log_id(6, 9, 0));
    request.leader_commit = Some(log_id(7, 1, 64));
    request.entries = (1..=64)
        .map(|index| Entry {
            log_id: log_id(7, 1, index),
            payload: EntryPayload::Membership(members.clone()),
        })
        .collect();
    let bytes = peer_wire::encode_request(route(), &request).unwrap();
    let size = bytes.len();
    let result = raw_request(&f, &mut owner, &bytes, false).await;
    let closed = bounded_close(&mut owner).await;
    let recovered = reclaimed(&owner);
    assert!(closed.is_ok() && recovered);
    assert!(size > 65536);
    assert_eq!(size, 232688);
    assert_eq!(
        peer_wire::decode_reply(&result.unwrap()).unwrap().1,
        Ok(Some(log_id(7, 1, 64)))
    );
    assert_eq!(spy.count(), 1);
}
#[tokio::test]
async fn typed_client_preserves_success_conflict_and_higher_vote() {
    let mut previous = heartbeat();
    previous.prev_log_id = Some(log_id(6, 9, 8));
    let higher = vote(StoredVote {
        term: 8,
        node: 2,
        committed: true,
    });
    let higher_same_term = vote(StoredVote {
        term: 7,
        node: 2,
        committed: true,
    });
    assert_eq!(
        higher_same_term.partial_cmp(&previous.vote),
        Some(Ordering::Greater)
    );
    for answer in [
        Ok(Some(log_id(6, 9, 8))),
        Err(AppendError::Conflict {
            log_id: log_id(6, 9, 8),
            hint: None,
        }),
        Err(AppendError::HigherVote(higher)),
        Err(AppendError::HigherVote(higher_same_term)),
    ] {
        let f = Fixture::new();
        let spy = Spy::exact(Some(answer.clone()));
        let mut server = f.owner(2, spy.clone());
        let mut client_owner = f.owner(1, Spy::exact(None));
        let address = listening(&mut server).await;
        let mut client = client_owner.loopback_client(2, address);
        let result = timeout(
            OBSERVE,
            client.append_entries(previous.clone(), RPCOption::new(OBSERVE)),
        )
        .await;
        let client_closed = bounded_close(&mut client_owner).await;
        let server_closed = bounded_close(&mut server).await;
        assert!(
            client_closed.is_ok()
                && server_closed.is_ok()
                && reclaimed(&server)
                && reclaimed(&client_owner)
        );
        assert_eq!(result.unwrap().unwrap(), answer);
        assert_eq!(spy.count(), 1);
    }
}

#[tokio::test]
async fn authenticated_request_shape_and_route_refusals_never_dispatch() {
    let mut requests = Vec::new();
    let mut r = heartbeat();
    r.vote.committed = false;
    requests.push(r);
    let mut r = heartbeat();
    r.vote.leader_id.node_id = 2;
    requests.push(r);
    let mut r = entries();
    r.entries.remove(0);
    requests.push(r);
    let mut r = entries();
    r.entries[1].log_id = log_id(7, 1, 3);
    requests.push(r);
    let mut r = entries();
    r.prev_log_id = Some(log_id(6, 8, 0));
    r.entries = vec![Entry {
        log_id: log_id(5, 9, 1),
        payload: EntryPayload::Blank,
    }];
    requests.push(r);
    let mut cases: Vec<Vec<u8>> = requests
        .iter()
        .map(|r| peer_wire::encode_request(route(), r).unwrap())
        .collect();
    for r in [
        peer_wire::Route {
            source: 2,
            ..route()
        },
        peer_wire::Route {
            target: 1,
            ..route()
        },
        peer_wire::Route {
            cluster: [2; 16],
            ..route()
        },
    ] {
        cases.push(peer_wire::encode_request(r, &heartbeat()).unwrap());
    }
    let mut r = heartbeat();
    r.prev_log_id = Some(log_id(1, 1, i64::MAX as u64));
    r.entries = vec![Entry {
        log_id: log_id(2, 1, 0),
        payload: EntryPayload::Blank,
    }];
    cases.push(peer_wire::encode_request(route(), &r).unwrap());
    for bytes in cases {
        let f = Fixture::new();
        let spy = Spy::exact(None);
        let mut owner = f.owner(2, spy.clone());
        let mut rx = events(&owner);
        let result = raw_request(&f, &mut owner, &bytes, false).await;
        let authenticated = observe(&mut rx, Event::Stage(Stage::ServerAuthenticated)).await;
        let closed = bounded_close(&mut owner).await;
        let recovered = reclaimed(&owner);
        assert!(authenticated && closed.is_ok() && recovered);
        assert!(result.is_err());
        assert_eq!(spy.count(), 0);
    }
}
#[tokio::test]
async fn independently_corrupted_headers_bodies_and_clean_truncation_refuse() {
    let baseline = independent_heartbeat();
    let mut cases = Vec::new();
    for (offset, value) in [(4, 1), (5, 3), (5, 2), (6, 1), (60, 2)] {
        let mut bytes = baseline.clone();
        bytes[offset] = value;
        cases.push(bytes);
    }
    let mut bytes = baseline[..44].to_vec();
    bytes[8..12].copy_from_slice(&264069u32.to_be_bytes());
    cases.push(bytes);
    let mut bytes = baseline[..44].to_vec();
    bytes[8..12].copy_from_slice(&u32::MAX.to_be_bytes());
    cases.push(bytes);
    for length in [1, 43, 44, baseline.len() - 1] {
        cases.push(baseline[..length].to_vec());
    }
    for bytes in cases {
        let f = Fixture::new();
        let spy = Spy::exact(None);
        let mut owner = f.owner(2, spy.clone());
        let result = raw_request(&f, &mut owner, &bytes, false).await;
        let closed = bounded_close(&mut owner).await;
        let recovered = reclaimed(&owner);
        assert!(closed.is_ok() && recovered);
        assert!(result.is_err());
        assert_eq!(spy.count(), 0);
    }
}
#[tokio::test]
async fn extra_request_application_byte_refuses_before_dispatch() {
    let f = Fixture::new();
    let spy = Spy::exact(None);
    let mut owner = f.owner(2, spy.clone());
    let mut bytes = independent_heartbeat();
    bytes.push(0xa5);
    let result = raw_request(&f, &mut owner, &bytes, false).await;
    let closed = bounded_close(&mut owner).await;
    let recovered = reclaimed(&owner);
    assert!(closed.is_ok() && recovered);
    assert_eq!(spy.count(), 0, "extra-byte boundary must precede dispatch");
    assert!(result.is_err());
}
#[tokio::test]
async fn raw_tcp_request_write_close_is_not_authenticated_tls_termination() {
    let f = Fixture::new();
    let spy = Spy::exact(None);
    let mut owner = f.owner(2, spy.clone());
    let address = listening(&mut owner).await;
    let result = timeout(OBSERVE, async {
        let mut stream = connect(f.client_config(&f.client, 2), address).await?;
        stream.write_all(&independent_heartbeat()).await?;
        stream.flush().await?;
        stream.get_mut().0.shutdown().await?;
        read_frame(&mut stream).await
    })
    .await;
    let closed = bounded_close(&mut owner).await;
    let recovered = reclaimed(&owner);
    assert!(closed.is_ok() && recovered);
    assert!(matches!(result, Ok(Err(_))));
    assert_eq!(spy.count(), 0);
}
#[tokio::test]
async fn final_applied_remove_after_authenticated_handshake_prevents_dispatch() {
    let f = Fixture::new();
    let spy = Spy::exact(None);
    let mut owner = f.owner(2, spy.clone());
    let mut rx = events(&owner);
    let hold = gate(&owner, Stage::BeforeDispatch);
    let address = listening(&mut owner).await;
    let config = f.client_config(&f.client, 2);
    let peer = tokio::spawn(async move {
        let mut stream = connect(config, address).await?;
        send_frame(&mut stream, &independent_heartbeat(), false).await?;
        read_frame(&mut stream).await
    });
    let reached = observe(&mut rx, Event::Stage(Stage::BeforeDispatch)).await;
    let removed = if reached { f.remove(1) } else { false };
    release(&owner, Stage::BeforeDispatch, &hold);
    let peer_result = join_peer(peer).await;
    let closed = bounded_close(&mut owner).await;
    let recovered = reclaimed(&owner);
    assert!(reached && removed && closed.is_ok() && recovered);
    assert_eq!(
        spy.count(),
        0,
        "actual applied Remove must be rechecked immediately before dispatch"
    );
    assert!(matches!(peer_result, Some(Ok(Err(_)))));
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn held_actual_store_mutex_refuses_final_authorization_without_waiting() {
    let f = Fixture::new();
    let spy = Spy::exact(None);
    let mut owner = f.owner(2, spy.clone());
    let mut rx = events(&owner);
    let hold = gate(&owner, Stage::BeforeDispatch);
    let address = listening(&mut owner).await;
    let config = f.client_config(&f.client, 2);
    let peer = tokio::spawn(async move {
        let mut stream = connect(config, address).await?;
        send_frame(&mut stream, &independent_heartbeat(), false).await?;
        read_frame(&mut stream).await
    });
    let reached = observe(&mut rx, Event::Stage(Stage::BeforeDispatch)).await;
    let store = f.sm.clone();
    let (locked_tx, locked_rx) = oneshot::channel();
    let (unlock_tx, unlock_rx) = std::sync::mpsc::channel();
    let locker = std::thread::spawn(move || {
        let _guard = store.0.lock().unwrap();
        let _ = locked_tx.send(());
        let _ = unlock_rx.recv_timeout(OBSERVE);
    });
    let locked = timeout(OBSERVE, locked_rx).await;
    release(&owner, Stage::BeforeDispatch, &hold);
    let finished = observe(&mut rx, Event::Finished(Some(Refusal::Identity))).await;
    let _ = unlock_tx.send(());
    let locker_joined = locker.join();
    let peer_result = join_peer(peer).await;
    let closed = bounded_close(&mut owner).await;
    let recovered = reclaimed(&owner);
    assert!(
        reached
            && matches!(locked, Ok(Ok(())))
            && locker_joined.is_ok()
            && finished
            && closed.is_ok()
            && recovered
    );
    assert_eq!(spy.count(), 0);
    assert!(matches!(peer_result, Some(Ok(Err(_)))));
}
#[tokio::test]
async fn poisoned_actual_store_mutex_refuses_final_authorization() {
    let f = Fixture::new();
    let spy = Spy::exact(None);
    let mut owner = f.owner(2, spy.clone());
    let mut rx = events(&owner);
    let hold = gate(&owner, Stage::BeforeDispatch);
    let address = listening(&mut owner).await;
    let config = f.client_config(&f.client, 2);
    let peer = tokio::spawn(async move {
        let mut stream = connect(config, address).await?;
        send_frame(&mut stream, &independent_heartbeat(), false).await?;
        read_frame(&mut stream).await
    });
    let reached = observe(&mut rx, Event::Stage(Stage::BeforeDispatch)).await;
    let store = f.sm.clone();
    let poisoned = std::thread::spawn(move || {
        let _guard = store.0.lock().unwrap();
        panic!("deliberate transport registry poison");
    })
    .join()
    .is_err();
    release(&owner, Stage::BeforeDispatch, &hold);
    let peer_result = join_peer(peer).await;
    let closed = bounded_close(&mut owner).await;
    let recovered = reclaimed(&owner);
    assert!(reached && poisoned && closed.is_ok() && recovered);
    assert_eq!(spy.count(), 0);
    assert!(matches!(peer_result, Some(Ok(Err(_)))));
}

async fn join_peer<T>(
    mut peer: tokio::task::JoinHandle<T>,
) -> Option<Result<T, tokio::task::JoinError>> {
    match timeout(OBSERVE, &mut peer).await {
        Ok(result) => Some(result),
        Err(_) => {
            peer.abort();
            let _ = peer.await;
            None
        }
    }
}
#[tokio::test]
async fn actual_tls_rejects_root_cluster_key_expiry_missing_certificate_and_wrong_target() {
    for case in 0..6 {
        let f = Fixture::new();
        let foreign = Fixture::new();
        let spy = Spy::exact(None);
        let mut owner = f.owner(2, spy.clone());
        let mut rx = events(&owner);
        let address = listening(&mut owner).await;
        let identity = match case {
            0 => Fixture::reissue(&foreign.issuer, CLUSTER, 1, &f.client, 940, 10000),
            1 => Fixture::reissue(&f.issuer, [2; 16], 1, &f.client, 940, 10000),
            2 => Fixture::issue(&f.issuer, CLUSTER, 1, 940, 10000),
            3 => Fixture::reissue(&f.issuer, CLUSTER, 1, &f.client, 800, 1000),
            _ => Fixture::reissue(&f.issuer, CLUSTER, 1, &f.client, 940, 10000),
        };
        let config = if case == 4 {
            let mut config = ClientConfig::builder_with_details(
                Arc::new(rustls::crypto::ring::default_provider()),
                Arc::new(pki::FixedTime(1001)),
            )
            .with_protocol_versions(&[&rustls::version::TLS13])
            .unwrap()
            .dangerous()
            .with_custom_certificate_verifier(f.verifier().client(node(2)))
            .with_no_client_auth();
            config.enable_sni = false;
            config.resumption = rustls::client::Resumption::disabled();
            Arc::new(config)
        } else if case == 5 {
            f.client_config(&f.client, 1)
        } else {
            f.client_config(&identity, 2)
        };
        let exchanged = timeout(OBSERVE, async {
            let mut stream = connect(config, address).await?;
            send_frame(&mut stream, &independent_heartbeat(), false).await?;
            read_frame(&mut stream).await
        })
        .await;
        let finished = observe(&mut rx, Event::Finished(Some(Refusal::Tls))).await;
        let closed = bounded_close(&mut owner).await;
        let recovered = reclaimed(&owner);
        assert!(closed.is_ok() && recovered);
        assert!(matches!(exchanged, Ok(Err(_))));
        assert_eq!(spy.count(), 0);
        // A wrong expected server may close the socket before the server completes its own handshake.
        assert!(finished || case == 5);
    }
}
#[tokio::test]
async fn configured_local_source_certificate_mismatch_refuses_at_authenticated_receiver() {
    let f = Fixture::new();
    let spy = Spy::exact(None);
    let mut server = f.owner(2, spy.clone());
    let mut sender = f.owner_with(1, &f.server, Spy::exact(None));
    let mut rx = events(&server);
    let address = listening(&mut server).await;
    let result = sender
        .loopback_client(2, address)
        .append_entries(heartbeat(), RPCOption::new(OBSERVE))
        .await;
    let authenticated = observe(&mut rx, Event::Stage(Stage::ServerAuthenticated)).await;
    let sender_closed = bounded_close(&mut sender).await;
    let server_closed = bounded_close(&mut server).await;
    assert!(
        authenticated
            && sender_closed.is_ok()
            && server_closed.is_ok()
            && reclaimed(&sender)
            && reclaimed(&server)
    );
    assert!(result.is_err());
    assert_eq!(spy.count(), 0);
}
#[derive(Clone, Copy)]
enum ReplyEnd {
    Clean,
    Extra,
    Missing,
    Raw,
    ShortHeader,
    ShortBody,
}
async fn adversarial_reply(
    request: AppendEntriesRequest<Arx>,
    answer: AppendResult<Arx>,
    reply_route: peer_wire::Route,
    end: ReplyEnd,
) -> (bool, bool, bool) {
    let f = Fixture::new();
    let mut owner = f.owner(1, Spy::exact(None));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let config = f.server_config(&f.server);
    let (release_tx, release_rx) = oneshot::channel();
    let peer = tokio::spawn(async move {
        let (socket, _) = listener.accept().await?;
        let mut stream = TlsAcceptor::from(config)
            .accept_with(socket, |c| c.set_buffer_limit(Some(65536)))
            .await?;
        let bytes = read_frame(&mut stream).await?;
        clean_eof(&mut stream).await?;
        let decoded = peer_wire::decode_request(&bytes)
            .map_err(|_| io::Error::other("test request decode"))?;
        let mut frame = peer_wire::encode_reply(reply_route, &answer)
            .map_err(|_| io::Error::other("test reply encode"))?;
        match end {
            ReplyEnd::Clean => send_frame(&mut stream, &frame, false).await?,
            ReplyEnd::Extra => {
                frame.push(0xa5);
                send_frame(&mut stream, &frame, false).await?;
            }
            ReplyEnd::Missing => {
                stream.write_all(&frame).await?;
                stream.flush().await?;
                let _ = release_rx.await;
            }
            ReplyEnd::Raw => {
                stream.write_all(&frame).await?;
                stream.flush().await?;
                stream.get_mut().0.shutdown().await?;
            }
            ReplyEnd::ShortHeader => {
                send_frame(&mut stream, &frame[..43], false).await?;
            }
            ReplyEnd::ShortBody => {
                send_frame(&mut stream, &frame[..frame.len() - 1], false).await?;
            }
        }
        Ok::<_, io::Error>(decoded.1.vote.committed)
    });
    let option = RPCOption::new(if matches!(end, ReplyEnd::Missing) {
        Duration::from_millis(400)
    } else {
        OBSERVE
    });
    let result = timeout(
        OBSERVE,
        owner
            .loopback_client(2, address)
            .append_entries(request, option),
    )
    .await;
    let _ = release_tx.send(());
    let peer_result = join_peer(peer).await;
    let closed = bounded_close(&mut owner).await;
    let recovered = reclaimed(&owner);
    (
        matches!(result, Ok(Err(_))),
        matches!(peer_result, Some(Ok(Ok(true)))),
        closed.is_ok() && recovered,
    )
}
#[tokio::test]
async fn sender_rejects_forged_reply_routes_and_unsent_full_acknowledgments() {
    let request = entries();
    let correct = Ok(Some(log_id(7, 1, 1)));
    for r in [
        peer_wire::Route {
            source: 1,
            ..reverse()
        },
        peer_wire::Route {
            target: 2,
            ..reverse()
        },
        peer_wire::Route {
            cluster: [2; 16],
            ..reverse()
        },
    ] {
        let (refused, observed, closed) =
            adversarial_reply(request.clone(), correct.clone(), r, ReplyEnd::Clean).await;
        assert!(refused && observed && closed);
    }
    for answer in [
        Ok(None),
        Ok(Some(log_id(7, 1, 2))),
        Ok(Some(log_id(6, 1, 1))),
        Ok(Some(log_id(7, 2, 1))),
        Err(AppendError::Conflict {
            log_id: log_id(2, 9, 0),
            hint: None,
        }),
        Err(AppendError::HigherVote(request.vote)),
        Err(AppendError::HigherVote(vote(StoredVote {
            term: 6,
            node: 2,
            committed: true,
        }))),
    ] {
        let (refused, observed, closed) =
            adversarial_reply(request.clone(), answer, reverse(), ReplyEnd::Clean).await;
        assert!(refused && observed && closed);
    }
    let mut request = heartbeat();
    request.prev_log_id = Some(log_id(6, 9, 3));
    let (refused, observed, closed) = adversarial_reply(
        request,
        Err(AppendError::Conflict {
            log_id: log_id(6, 9, 4),
            hint: None,
        }),
        reverse(),
        ReplyEnd::Clean,
    )
    .await;
    assert!(refused && observed && closed);
}
#[tokio::test]
async fn sender_requires_authenticated_clean_reply_end_even_after_valid_success() {
    for end in [
        ReplyEnd::Extra,
        ReplyEnd::Missing,
        ReplyEnd::Raw,
        ReplyEnd::ShortHeader,
        ReplyEnd::ShortBody,
    ] {
        let (refused, observed, closed) =
            adversarial_reply(heartbeat(), Ok(None), reverse(), end).await;
        assert!(
            refused && observed && closed,
            "a received success frame alone cannot complete the exchange"
        );
    }
}

#[tokio::test]
async fn original_inbound_deadline_spans_authenticated_phase_and_missing_request_terminator() {
    let f = Fixture::new();
    let spy = Spy::exact(None);
    let mut owner = f.owner(2, spy.clone());
    let mut rx = events(&owner);
    let handshake_gate = gate(&owner, Stage::ServerAuthenticated);
    let request_gate = gate(&owner, Stage::RequestRead);
    let address = listening(&mut owner).await;
    let mut stream = connect(f.client_config(&f.client, 2), address)
        .await
        .unwrap();
    let authenticated = observe(&mut rx, Event::Stage(Stage::ServerAuthenticated)).await;
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(4)).await;
    tokio::time::resume();
    release(&owner, Stage::ServerAuthenticated, &handshake_gate);
    let written = stream.write_all(&independent_heartbeat()).await;
    let flushed = stream.flush().await;
    let framed = observe(&mut rx, Event::Stage(Stage::RequestRead)).await;
    tokio::time::pause();
    tokio::time::advance(Duration::from_millis(1100)).await;
    tokio::time::resume();
    release(&owner, Stage::RequestRead, &request_gate);
    let expired = observe(&mut rx, Event::Finished(Some(Refusal::Deadline))).await;
    drop(stream);
    let closed = bounded_close(&mut owner).await;
    let recovered = reclaimed(&owner);
    assert!(
        authenticated
            && written.is_ok()
            && flushed.is_ok()
            && framed
            && expired
            && closed.is_ok()
            && recovered
    );
    assert_eq!(spy.count(), 0);
}
#[tokio::test]
async fn silent_inbound_socket_expires_before_any_rpc_metadata() {
    let f = Fixture::new();
    let spy = Spy::exact(None);
    let mut owner = f.owner(2, spy.clone());
    let mut rx = events(&owner);
    let hold = gate(&owner, Stage::ServerHandshake);
    let address = listening(&mut owner).await;
    let socket = TcpStream::connect(address).await.unwrap();
    let admitted = observe(&mut rx, Event::Stage(Stage::ServerHandshake)).await;
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(6)).await;
    tokio::time::resume();
    release(&owner, Stage::ServerHandshake, &hold);
    let expired = observe(&mut rx, Event::Finished(Some(Refusal::Deadline))).await;
    drop(socket);
    let closed = bounded_close(&mut owner).await;
    let recovered = reclaimed(&owner);
    assert!(admitted && expired && closed.is_ok() && recovered);
    assert_eq!(spy.count(), 0);
}
#[tokio::test]
async fn complete_request_without_close_notify_expires_in_real_terminator_read() {
    let f = Fixture::new();
    let spy = Spy::exact(None);
    let mut owner = f.owner(2, spy.clone());
    let mut rx = events(&owner);
    let address = listening(&mut owner).await;
    let mut stream = connect(f.client_config(&f.client, 2), address)
        .await
        .unwrap();
    let written = stream.write_all(&independent_heartbeat()).await;
    let flushed = stream.flush().await;
    let framed = observe(&mut rx, Event::Stage(Stage::RequestRead)).await;
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(6)).await;
    tokio::time::resume();
    let expired = observe(&mut rx, Event::Finished(Some(Refusal::Deadline))).await;
    drop(stream);
    let closed = bounded_close(&mut owner).await;
    assert!(
        written.is_ok()
            && flushed.is_ok()
            && framed
            && expired
            && closed.is_ok()
            && reclaimed(&owner)
    );
    assert_eq!(spy.count(), 0);
}
#[tokio::test]
async fn configured_receiver_target_mismatch_refuses_after_real_authentication() {
    let f = Fixture::new();
    let spy = Spy::exact(None);
    let mut owner = f.owner_with(3, &f.server, spy.clone());
    let mut rx = events(&owner);
    let result = raw_request(&f, &mut owner, &independent_heartbeat(), false).await;
    let authenticated = observe(&mut rx, Event::Stage(Stage::ServerAuthenticated)).await;
    let closed = bounded_close(&mut owner).await;
    assert!(authenticated && result.is_err() && closed.is_ok() && reclaimed(&owner));
    assert_eq!(spy.count(), 0);
}
#[tokio::test]
async fn outbound_expiry_before_connect_refuses_without_dial_and_zero_budget_is_lazy() {
    use futures_util::FutureExt;
    let f = Fixture::new();
    let mut owner = f.owner(1, Spy::exact(None));
    let mut rx = events(&owner);
    let hold = gate(&owner, Stage::BeforeConnect);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let mut client = owner.loopback_client(2, address);
    let zero = client
        .append_entries(heartbeat(), RPCOption::new(Duration::ZERO))
        .await;
    let overflow = client
        .append_entries(heartbeat(), RPCOption::new(Duration::MAX))
        .await;
    let mut malformed = heartbeat();
    malformed.prev_log_id = Some(log_id(1, 1, u64::MAX));
    malformed.entries = vec![Entry {
        log_id: log_id(2, 1, 0),
        payload: EntryPayload::Blank,
    }];
    let bad_shape = client
        .append_entries(malformed, RPCOption::new(OBSERVE))
        .await;
    let peer = tokio::spawn(async move {
        client
            .append_entries(heartbeat(), RPCOption::new(Duration::from_secs(2)))
            .await
    });
    let reached = observe(&mut rx, Event::Stage(Stage::BeforeConnect)).await;
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(3)).await;
    tokio::time::resume();
    release(&owner, Stage::BeforeConnect, &hold);
    let result = join_peer(peer).await;
    let no_dial = listener.accept().now_or_never().is_none();
    let closed = bounded_close(&mut owner).await;
    let recovered = reclaimed(&owner);
    assert!(
        zero.is_err()
            && overflow.is_err()
            && bad_shape.is_err()
            && reached
            && matches!(result, Some(Ok(Err(_))))
            && no_dial
            && closed.is_ok()
            && recovered
    );
}
#[tokio::test]
async fn numeric_address_policy_and_invalid_notready_closed_clients_never_dial() {
    use futures_util::FutureExt;
    for address in ["192.168.1.2:443", "100.64.0.2:8443", "[fd00::2]:443"] {
        assert!(endpoint(address).is_ok());
    }
    for address in [
        "127.0.0.1:443",
        "[::1]:443",
        "0.0.0.0:443",
        "[::]:443",
        "224.0.0.1:443",
        "[ff02::1]:443",
        "255.255.255.255:443",
        "[::ffff:192.168.1.2]:443",
        "[fe80::1%3]:443",
        "192.168.1.2:0",
        "192.168.001.2:443",
        "[FD00::2]:443",
        "localhost:443",
        "https://192.168.1.2:443",
    ] {
        assert_eq!(endpoint(address), Err(Refusal::Address));
    }
    let f = Fixture::new();
    let mut owner = f.owner(1, Spy::exact(None));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let lazy = owner.loopback_client(2, address);
    let before = owner.shared.connections.available_permits();
    let tasks = owner.shared.state.lock().unwrap().tasks.len();
    let invalid = owner
        .client(2, &address.to_string())
        .append_entries(heartbeat(), RPCOption::new(OBSERVE))
        .await;
    let target = owner
        .loopback_client(0, address)
        .append_entries(heartbeat(), RPCOption::new(OBSERVE))
        .await;
    owner.shared.state.lock().unwrap().resources.take();
    let notready = lazy
        .clone()
        .append_entries(heartbeat(), RPCOption::new(OBSERVE))
        .await;
    let closed = bounded_close(&mut owner).await;
    let denied = lazy
        .clone()
        .append_entries(heartbeat(), RPCOption::new(OBSERVE))
        .await;
    let no_dial = listener.accept().now_or_never().is_none();
    assert!(
        before == 32
            && tasks == 0
            && invalid.is_err()
            && target.is_err()
            && notready.is_err()
            && closed.is_ok()
            && denied.is_err()
            && no_dial
            && reclaimed(&owner)
    );
}

#[tokio::test]
async fn four_shared_handshake_units_refuse_fifth_inbound_and_outbound_without_queueing() {
    use futures_util::FutureExt;
    let f = Fixture::new();
    let spy = Spy::exact(None);
    let mut owner = f.owner(1, spy.clone());
    let mut rx = events(&owner);
    let hold = gate(&owner, Stage::ServerHandshake);
    let address = listening(&mut owner).await;
    let mut peers = Vec::new();
    let mut reached = 0;
    for _ in 0..4 {
        let config = f.client_config(&f.server, 1);
        peers.push(tokio::spawn(async move { connect(config, address).await }));
        if observe(&mut rx, Event::Stage(Stage::ServerHandshake)).await {
            reached += 1;
        }
    }
    let charged = owner.shared.connections.available_permits() == 28
        && owner.shared.handshakes.available_permits() == 0;
    let outbound_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let outbound_address = outbound_listener.local_addr().unwrap();
    let outgoing = owner
        .loopback_client(2, outbound_address)
        .append_entries(heartbeat(), RPCOption::new(OBSERVE))
        .await;
    let no_dial = outbound_listener.accept().now_or_never().is_none();
    let fifth = TcpStream::connect(address).await;
    let refused = observe(&mut rx, Event::Refused(Refusal::Busy)).await;
    release(&owner, Stage::ServerHandshake, &hold);
    let mut handshakes = 0;
    for peer in peers {
        if let Some(Ok(Ok(stream))) = join_peer(peer).await {
            handshakes += 1;
            drop(stream);
        }
    }
    drop(fifth);
    let closed = bounded_close(&mut owner).await;
    let recovered = reclaimed(&owner);
    assert!(
        reached == 4
            && charged
            && outgoing.is_err()
            && no_dial
            && refused
            && handshakes == 4
            && closed.is_ok()
            && recovered
    );
    assert_eq!(spy.count(), 0);
}
#[tokio::test]
async fn thirty_two_shared_established_tasks_bound_inbound_and_outbound_admission() {
    use futures_util::FutureExt;
    let f = Fixture::new();
    let spy = Spy::exact(None);
    let mut owner = f.owner(1, spy.clone());
    let mut rx = events(&owner);
    let inbound_hold = gate(&owner, Stage::ServerAuthenticated);
    let outbound_hold = gate(&owner, Stage::ClientAuthenticated);
    let address = listening(&mut owner).await;
    let inbound_config = f.client_config(&f.server, 1);
    let remote_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let remote_address = remote_listener.local_addr().unwrap();
    let config = f.server_config(&f.server);
    let (release_tx, release_rx) = oneshot::channel();
    let extra_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let extra_address = extra_listener.local_addr().unwrap();
    // The fixture freezes only Tokio policy time while all 32 real socket/TLS
    // exchanges are established. A bounded blocking observer inhibits Tokio's
    // automatic clock jump; it performs no handshake, crypto or transport I/O.
    let (clock_tx, clock_rx) = std::sync::mpsc::channel();
    let (clock_started_tx, clock_started_rx) = oneshot::channel();
    let clock_guard = tokio::task::spawn_blocking(move || {
        let _ = clock_started_tx.send(());
        clock_rx.recv_timeout(OBSERVE).is_ok()
    });
    let clock_started = timeout(OBSERVE, clock_started_rx).await;
    tokio::time::pause();
    let mut inbound = Vec::new();
    let mut reached = 0;
    for _ in 0..28 {
        let peer = timeout(OBSERVE, connect(inbound_config.clone(), address)).await;
        if let Ok(Ok(peer)) = peer {
            inbound.push(peer);
        }
        if observe(&mut rx, Event::Stage(Stage::ServerAuthenticated)).await {
            reached += 1;
        }
    }
    let remote = tokio::spawn(async move {
        let mut streams = Vec::new();
        for _ in 0..4 {
            let (socket, _) = remote_listener.accept().await?;
            streams.push(
                TlsAcceptor::from(config.clone())
                    .accept_with(socket, |c| c.set_buffer_limit(Some(65536)))
                    .await?,
            );
        }
        let _ = release_rx.await;
        for mut stream in streams {
            let _ = read_frame(&mut stream).await?;
            clean_eof(&mut stream).await?;
            send_frame(
                &mut stream,
                &peer_wire::encode_reply(reverse(), &Ok(None)).unwrap(),
                false,
            )
            .await?;
        }
        Ok::<_, io::Error>(())
    });
    let mut callers = Vec::new();
    for _ in 0..4 {
        let mut client = owner.loopback_client(2, remote_address);
        callers.push(tokio::spawn(async move {
            client
                .append_entries(heartbeat(), RPCOption::new(OBSERVE))
                .await
        }));
        if observe(&mut rx, Event::Stage(Stage::ClientAuthenticated)).await {
            reached += 1;
        }
    }
    let full = owner.shared.connections.available_permits() == 0
        && owner.shared.handshakes.available_permits() == 4;
    let refused_outbound = owner
        .loopback_client(2, extra_address)
        .append_entries(heartbeat(), RPCOption::new(OBSERVE))
        .await;
    let no_dial = extra_listener.accept().now_or_never().is_none();
    let extra_inbound = TcpStream::connect(address).await;
    let refused_inbound = observe(&mut rx, Event::Refused(Refusal::Busy)).await;
    release(&owner, Stage::ServerAuthenticated, &inbound_hold);
    drop(inbound);
    drop(extra_inbound);
    release(&owner, Stage::ClientAuthenticated, &outbound_hold);
    let _ = release_tx.send(());
    let mut completed = 0;
    for caller in callers {
        if matches!(join_peer(caller).await, Some(Ok(Ok(Ok(None))))) {
            completed += 1;
        }
    }
    let remote_result = join_peer(remote).await;
    tokio::time::resume();
    let _ = clock_tx.send(());
    let clock_joined = clock_guard.await;
    // A later explicit call is admitted after the completed tasks are reaped.
    let mut later = owner.loopback_client(2, extra_address);
    let later_call = tokio::spawn(async move {
        later
            .append_entries(heartbeat(), RPCOption::new(Duration::from_millis(200)))
            .await
    });
    let admitted_later = timeout(OBSERVE, extra_listener.accept()).await.is_ok();
    let _ = join_peer(later_call).await;
    let closed = bounded_close(&mut owner).await;
    let recovered = reclaimed(&owner);
    assert!(
        matches!(clock_started, Ok(Ok(())))
            && matches!(clock_joined, Ok(true))
            && reached == 32
            && full
            && refused_outbound.is_err()
            && no_dial
            && refused_inbound
            && completed == 4
            && matches!(remote_result, Some(Ok(Ok(()))))
            && admitted_later
            && closed.is_ok()
            && recovered
    );
    assert_eq!(spy.count(), 0);
}
#[tokio::test]
async fn dropped_outbound_callers_keep_owned_units_until_handshake_or_reply_task_finishes() {
    for phase in [Stage::ClientHandshake, Stage::ReplyRead] {
        let f = Fixture::new();
        let spy = Spy::exact(Some(Ok(None)));
        let mut server = f.owner(2, spy.clone());
        let address = listening(&mut server).await;
        let mut owner = f.owner(1, Spy::exact(None));
        let mut rx = events(&owner);
        let hold = gate(&owner, phase);
        let mut client = owner.loopback_client(2, address);
        let caller = tokio::spawn(async move {
            client
                .append_entries(heartbeat(), RPCOption::new(OBSERVE))
                .await
        });
        let reached = observe(&mut rx, Event::Stage(phase)).await;
        caller.abort();
        let joined = caller.await;
        let charged = owner.shared.connections.available_permits() == 31
            && owner.shared.handshakes.available_permits()
                == if phase == Stage::ClientHandshake {
                    3
                } else {
                    4
                };
        release(&owner, phase, &hold);
        let finished = observe(&mut rx, Event::Finished(Some(Refusal::Closed))).await;
        let closed = bounded_close(&mut owner).await;
        let server_closed = bounded_close(&mut server).await;
        assert!(
            reached
                && joined.is_err()
                && charged
                && finished
                && closed.is_ok()
                && server_closed.is_ok()
                && reclaimed(&owner)
                && reclaimed(&server)
        );
        assert_eq!(spy.count(), usize::from(phase == Stage::ReplyRead));
    }
}

#[tokio::test]
async fn receiver_refuses_unavailable_and_unrelated_dispatcher_results() {
    for answer in [
        None,
        Some(Ok(Some(log_id(7, 1, 9)))),
        Some(Err(AppendError::Conflict {
            log_id: log_id(1, 1, 0),
            hint: None,
        })),
        Some(Err(AppendError::HigherVote(heartbeat().vote))),
    ] {
        let f = Fixture::new();
        let spy = if answer.is_none() {
            Arc::new(Spy {
                calls: Mutex::new(vec![]),
                answer: Mutex::new(None),
                unavailable: true,
                panic: false,
                entered: Mutex::new(None),
                gate: None,
            })
        } else {
            Spy::exact(answer)
        };
        let mut owner = f.owner(2, spy.clone());
        let result = raw_request(&f, &mut owner, &independent_heartbeat(), false).await;
        let closed = bounded_close(&mut owner).await;
        assert!(result.is_err() && closed.is_ok() && reclaimed(&owner));
        assert_eq!(spy.count(), 1);
    }
}
#[tokio::test]
async fn sender_reauthorizes_actual_server_after_reply_before_returning_any_outcome() {
    let f = Fixture::new();
    let spy = Spy::exact(Some(Ok(None)));
    let mut server = f.owner(2, spy.clone());
    let address = listening(&mut server).await;
    let mut owner = f.owner(1, Spy::exact(None));
    let mut rx = events(&owner);
    let hold = gate(&owner, Stage::ReplyRead);
    let mut client = owner.loopback_client(2, address);
    let caller = tokio::spawn(async move {
        client
            .append_entries(heartbeat(), RPCOption::new(OBSERVE))
            .await
    });
    let reached = observe(&mut rx, Event::Stage(Stage::ReplyRead)).await;
    let removed = if reached { f.remove(2) } else { false };
    release(&owner, Stage::ReplyRead, &hold);
    let result = join_peer(caller).await;
    let closed = bounded_close(&mut owner).await;
    let server_closed = bounded_close(&mut server).await;
    assert!(
        reached
            && removed
            && matches!(result, Some(Ok(Err(_))))
            && closed.is_ok()
            && server_closed.is_ok()
            && reclaimed(&owner)
            && reclaimed(&server)
    );
    assert_eq!(spy.count(), 1);
}
#[tokio::test]
async fn close_fences_admission_and_dispatch_across_real_socket_phases() {
    use futures_util::FutureExt;
    for phase in [
        Stage::ServerHandshake,
        Stage::ServerAuthenticated,
        Stage::RequestRead,
        Stage::BeforeDispatch,
        Stage::BeforeReply,
    ] {
        let f = Fixture::new();
        let spy = Spy::exact(Some(Ok(None)));
        let mut owner = f.owner(2, spy.clone());
        let mut rx = events(&owner);
        let hold = gate(&owner, phase);
        let address = listening(&mut owner).await;
        let config = f.client_config(&f.client, 2);
        let peer = tokio::spawn(async move {
            let mut stream = connect(config, address).await?;
            send_frame(&mut stream, &independent_heartbeat(), false).await?;
            read_frame(&mut stream).await
        });
        let reached = observe(&mut rx, Event::Stage(phase)).await;
        let mut closing = Box::pin(owner.close());
        let pending = closing.as_mut().now_or_never().is_none();
        drop(closing);
        let closed_gate = owner.shared.state.lock().unwrap().closed;
        let denied = owner
            .loopback_client(1, address)
            .append_entries(heartbeat(), RPCOption::new(OBSERVE))
            .await;
        release(&owner, phase, &hold);
        let peer_result = join_peer(peer).await;
        let closed = bounded_close(&mut owner).await;
        assert!(
            reached
                && pending
                && closed_gate
                && denied.is_err()
                && peer_result.is_some()
                && closed.is_ok()
                && reclaimed(&owner)
        );
        assert_eq!(spy.count(), usize::from(phase == Stage::BeforeReply));
    }
}
#[tokio::test]
async fn dispatcher_task_panic_is_joined_and_close_remains_unclean() {
    let f = Fixture::new();
    let spy = Arc::new(Spy {
        calls: Mutex::new(vec![]),
        answer: Mutex::new(None),
        unavailable: false,
        panic: true,
        entered: Mutex::new(None),
        gate: None,
    });
    let mut owner = f.owner(2, spy.clone());
    let result = raw_request(&f, &mut owner, &independent_heartbeat(), false).await;
    let closed = bounded_close(&mut owner).await;
    assert!(result.is_err());
    assert_eq!(closed, Err(Refusal::Unclean));
    assert!(reclaimed(&owner));
    assert_eq!(spy.count(), 1);
}
struct PanicPayload {
    owner: std::sync::Weak<Shared>,
    dropped: Arc<std::sync::atomic::AtomicUsize>,
}
impl Drop for PanicPayload {
    fn drop(&mut self) {
        let unlocked = self
            .owner
            .upgrade()
            .is_some_and(|owner| owner.state.try_lock().is_ok());
        self.dropped.store(
            if unlocked { 2 } else { 1 },
            std::sync::atomic::Ordering::SeqCst,
        );
    }
}
struct PanicDispatcher {
    owner: Mutex<Option<std::sync::Weak<Shared>>>,
    dropped: Arc<std::sync::atomic::AtomicUsize>,
    calls: std::sync::atomic::AtomicUsize,
}
impl AppendDispatch for PanicDispatcher {
    fn append(&self, _request: AppendEntriesRequest<Arx>) -> DispatchFuture {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let owner = self.owner.lock().unwrap().clone().unwrap();
        let dropped = self.dropped.clone();
        Box::pin(async move { std::panic::panic_any(PanicPayload { owner, dropped }) })
    }
}
#[tokio::test]
async fn joined_panic_payload_destructor_runs_outside_ownership_gate() {
    let f = Fixture::new();
    let dropped = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let dispatch = Arc::new(PanicDispatcher {
        owner: Mutex::new(None),
        dropped: dropped.clone(),
        calls: std::sync::atomic::AtomicUsize::new(0),
    });
    let mut owner = f.owner(2, dispatch.clone());
    *dispatch.owner.lock().unwrap() = Some(Arc::downgrade(&owner.shared));
    let result = raw_request(&f, &mut owner, &independent_heartbeat(), false).await;
    let closed = bounded_close(&mut owner).await;
    assert!(result.is_err());
    assert_eq!(closed, Err(Refusal::Unclean));
    assert!(reclaimed(&owner));
    assert_eq!(dispatch.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(dropped.load(std::sync::atomic::Ordering::SeqCst), 2);
}
#[tokio::test]
async fn cancelled_close_retains_original_deadline_aborted_tasks_and_keeper_until_join() {
    use futures_util::FutureExt;
    let f = Fixture::new();
    let spy = Spy::exact(None);
    let mut owner = f.owner(2, spy.clone());
    let mut rx = events(&owner);
    let work_hold = gate(&owner, Stage::ServerAuthenticated);
    let abort_hold = gate(&owner, Stage::CloseAborted);
    let address = listening(&mut owner).await;
    let stream = connect(f.client_config(&f.client, 2), address)
        .await
        .unwrap();
    let reached = observe(&mut rx, Event::Stage(Stage::ServerAuthenticated)).await;
    let shared = owner.shared.clone();
    let mut closing = Box::pin(owner.close());
    let first_pending = closing.as_mut().now_or_never().is_none();
    drop(closing);
    let original = shared.state.lock().unwrap().close_deadline;
    let kept_after_cancel = {
        let state = shared.state.lock().unwrap();
        state.closed && state.keeper.is_some() && !state.finished && !state.tasks.is_empty()
    };
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(6)).await;
    tokio::time::resume();
    let mut closing = Box::pin(owner.close());
    let abort_pending = closing.as_mut().now_or_never().is_none();
    drop(closing);
    let at_abort = observe(&mut rx, Event::Stage(Stage::CloseAborted)).await;
    let kept_before_join = {
        let state = shared.state.lock().unwrap();
        state.keeper.is_some()
            && !state.finished
            && state.aborting
            && state.close_deadline == original
    };
    release(&owner, Stage::ServerAuthenticated, &work_hold);
    release(&owner, Stage::CloseAborted, &abort_hold);
    let closed = bounded_close(&mut owner).await;
    drop(stream);
    assert!(
        reached
            && first_pending
            && kept_after_cancel
            && abort_pending
            && at_abort
            && kept_before_join
    );
    assert_eq!(closed, Err(Refusal::Unclean));
    assert!(reclaimed(&owner));
    assert_eq!(spy.count(), 0);
}
#[tokio::test]
async fn cancelled_close_during_dispatch_keeps_store_until_resumed_close_joins() {
    use futures_util::FutureExt;
    let f = Fixture::new();
    let hold = Arc::new(Semaphore::new(0));
    let (entered_tx, entered_rx) = oneshot::channel();
    let spy = Arc::new(Spy {
        calls: Mutex::new(vec![]),
        answer: Mutex::new(Some(Ok(None))),
        unavailable: false,
        panic: false,
        entered: Mutex::new(Some(entered_tx)),
        gate: Some(hold.clone()),
    });
    let mut owner = f.owner(2, spy.clone());
    let address = listening(&mut owner).await;
    let config = f.client_config(&f.client, 2);
    let peer = tokio::spawn(async move {
        let mut stream = connect(config, address).await?;
        send_frame(&mut stream, &independent_heartbeat(), false).await?;
        read_frame(&mut stream).await
    });
    let entered = timeout(OBSERVE, entered_rx).await;
    let shared = owner.shared.clone();
    let mut closing = Box::pin(owner.close());
    let pending = closing.as_mut().now_or_never().is_none();
    drop(closing);
    let retained = {
        let state = shared.state.lock().unwrap();
        state.closed && state.keeper.is_some() && !state.tasks.is_empty()
    };
    hold.add_permits(1);
    let closed = bounded_close(&mut owner).await;
    let peer_result = join_peer(peer).await;
    assert!(
        matches!(entered, Ok(Ok(())))
            && pending
            && retained
            && closed.is_ok()
            && peer_result.is_some()
            && reclaimed(&owner)
    );
    assert_eq!(spy.count(), 1);
}
#[tokio::test]
async fn successful_close_releases_store_with_clients_shared_dispatcher_and_configs_retained() {
    let f = Fixture::new();
    let released = f.sm.0.lock().unwrap().released();
    let weak = Arc::downgrade(&f.sm.0);
    let spy = Spy::exact(None);
    let mut owner = f.owner(1, spy.clone());
    let shared = owner.shared.clone();
    let config = f.client_config(&f.client, 2);
    let registry = peer_registry::registry(&f.sm);
    let client = owner.loopback_client(2, "127.0.0.1:1".parse().unwrap());
    let before = Arc::strong_count(&f.sm.0);
    let closed = bounded_close(&mut owner).await;
    let after = Arc::strong_count(&f.sm.0);
    let recovered = reclaimed(&owner);
    drop(f.sm);
    let gone = weak.upgrade().is_none();
    let acknowledged = released.is_set();
    let denied = client
        .clone()
        .append_entries(heartbeat(), RPCOption::new(OBSERVE))
        .await;
    let callback = registry(CLUSTER, node(1), f.client.digest());
    assert!(
        before == 2
            && closed.is_ok()
            && after == 1
            && recovered
            && gone
            && acknowledged
            && denied.is_err()
            && callback.is_err()
    );
    drop((client, shared, spy, config, registry));
}

#[tokio::test]
async fn cancelled_close_retains_aborted_outbound_task_until_join() {
    use futures_util::FutureExt;

    let f = Fixture::new();
    let mut owner = f.owner(1, Spy::exact(None));
    let mut rx = events(&owner);
    let work_hold = gate(&owner, Stage::ClientAuthenticated);
    let abort_hold = gate(&owner, Stage::CloseAborted);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let config = f.server_config(&f.server);
    let mut client = owner.loopback_client(2, address);
    let option = RPCOption::new(Duration::from_secs(30));
    let exceeds_grace = option.soft_ttl() > CLOSE_BUDGET;
    let (remote_authenticated_tx, remote_authenticated_rx) = oneshot::channel();
    let (clock_tx, clock_rx) = std::sync::mpsc::channel();
    let (clock_started_tx, clock_started_rx) = oneshot::channel();

    // Freeze policy time through real TLS setup and the two close cancellations.
    // The bounded observer prevents automatic time jumps and performs no TLS or I/O.
    // All fallible fixture setup precedes its start; cleanup precedes assertions.
    let clock_guard = tokio::task::spawn_blocking(move || {
        let _ = clock_started_tx.send(());
        clock_rx.recv_timeout(OBSERVE).is_ok()
    });
    let clock_started = timeout(OBSERVE, clock_started_rx).await;
    tokio::time::pause();
    let remote = tokio::spawn(async move {
        let (socket, _) = listener.accept().await?;
        let mut stream = TlsAcceptor::from(config)
            .accept_with(socket, |connection| {
                connection.set_buffer_limit(Some(65536))
            })
            .await?;
        let tls13 = stream.get_ref().1.protocol_version() == Some(rustls::ProtocolVersion::TLSv1_3);
        let _ = remote_authenticated_tx.send(tls13);
        // The client is held after actual TLS authentication, before any frame write.
        Ok::<_, io::Error>(read_frame(&mut stream).await.is_err())
    });
    let caller = tokio::spawn(async move { client.append_entries(heartbeat(), option).await });
    let authenticated = observe(&mut rx, Event::Stage(Stage::ClientAuthenticated)).await;
    let remote_authenticated = timeout(OBSERVE, remote_authenticated_rx).await;
    let shared = owner.shared.clone();

    let mut closing = Box::pin(owner.close());
    let first_pending = closing.as_mut().now_or_never().is_none();
    drop(closing);
    let original_deadline = shared.state.lock().unwrap().close_deadline;
    let first_cancel_retained = {
        let state = shared.state.lock().unwrap();
        state.closed && !state.tasks.is_empty() && state.keeper.is_some() && !state.finished
    };

    // The RPC's soft TTL outlives the original five-second close grace.
    // One poll on this current-thread runtime issues abort without scheduling its
    // task to completion or acknowledging its JoinSet record.
    tokio::time::advance(CLOSE_BUDGET + Duration::from_secs(1)).await;
    let mut closing = Box::pin(owner.close());
    let abort_pending = closing.as_mut().now_or_never().is_none();
    let observed_abort = rx
        .try_recv()
        .is_ok_and(|event| event == Event::Stage(Stage::CloseAborted));
    let at_abort = {
        let state = shared.state.lock().unwrap();
        state.aborting
            && !state.tasks.is_empty()
            && state.keeper.is_some()
            && !state.finished
            && state.close_deadline == original_deadline
    };
    drop(closing);
    let second_cancel_retained = {
        let state = shared.state.lock().unwrap();
        state.aborting
            && !state.tasks.is_empty()
            && state.keeper.is_some()
            && !state.finished
            && state.close_deadline == original_deadline
    };
    release(&owner, Stage::ClientAuthenticated, &work_hold);
    release(&owner, Stage::CloseAborted, &abort_hold);
    tokio::time::resume();
    let closed = bounded_close(&mut owner).await;
    let caller_result = join_peer(caller).await;
    let remote_result = join_peer(remote).await;
    let recovered = reclaimed(&owner);
    let _ = clock_tx.send(());
    let clock_joined = clock_guard.await;

    assert!(
        exceeds_grace
            && matches!(clock_started, Ok(Ok(())))
            && authenticated
            && matches!(remote_authenticated, Ok(Ok(true)))
            && first_pending
            && original_deadline.is_some()
            && first_cancel_retained
            && abort_pending
            && at_abort
            && second_cancel_retained
            && observed_abort
            && matches!(caller_result, Some(Ok(Err(_))))
            && matches!(remote_result, Some(Ok(Ok(true))))
            && matches!(clock_joined, Ok(true))
    );
    assert_eq!(closed, Err(Refusal::Unclean));
    assert!(recovered);
}
// Full-RPC recording fixtures use actual TLS identities and applied Registry.
struct RpcSpy {
    calls: Mutex<Vec<Vec<u8>>>,
    deny: bool,
    snapshot_vote: Option<BallotVote>,
    snapshot_gate: Option<Arc<Semaphore>>,
    snapshot_entered: Mutex<Option<oneshot::Sender<()>>>,
}
impl AppendDispatch for RpcSpy {
    fn append(&self, r: AppendEntriesRequest<Arx>) -> DispatchFuture {
        let last = r.entries.last().map(|e| e.log_id).or(r.prev_log_id);
        self.calls
            .lock()
            .unwrap()
            .push(peer_wire::encode_request(route(), &r).unwrap_or_default());
        Box::pin(async move { Ok(Ok(last)) })
    }
    fn ballot(&self, r: Ballot, pre: bool) -> BallotFuture {
        let bytes = peer_wire::encode_vote(
            route(),
            if pre {
                peer_wire::Kind::PreVoteRequest
            } else {
                peer_wire::Kind::VoteRequest
            },
            &r,
        );
        if let Ok(b) = bytes {
            self.calls.lock().unwrap().push(b)
        }
        let reply = BallotReply {
            vote: if pre {
                vote(StoredVote {
                    term: 2,
                    node: 1,
                    committed: false,
                })
            } else {
                r.vote
            },
            last_log_id: r.last_log_id,
            vote_granted: !self.deny,
        };
        Box::pin(async move { Ok(reply) })
    }
    fn snapshot(&self, v: BallotVote, s: Snapshot) -> SnapshotFuture {
        self.calls
            .lock()
            .unwrap()
            .push(peer_wire::encode_snapshot(route(), v, &s).unwrap_or_default());
        if let Some(entered) = self.snapshot_entered.lock().unwrap().take() {
            let _ = entered.send(());
        }
        let gate = self.snapshot_gate.clone();
        let v = self.snapshot_vote.unwrap_or(v);
        Box::pin(async move {
            if let Some(g) = gate {
                let _ = g.acquire().await;
            }
            Ok(openraft::raft::SnapshotResponse { vote: v })
        })
    }
}
fn rpc_spy(deny: bool) -> Arc<RpcSpy> {
    Arc::new(RpcSpy {
        calls: Mutex::new(Vec::new()),
        deny,
        snapshot_vote: None,
        snapshot_gate: None,
        snapshot_entered: Mutex::new(None),
    })
}
fn ballot_request() -> Ballot {
    Ballot {
        vote: vote(StoredVote {
            term: 7,
            node: 1,
            committed: false,
        }),
        last_log_id: Some(log_id(1, 1, 0)),
        leadership_transfer_from: Some(vote(StoredVote {
            term: 3,
            node: 8,
            committed: true,
        })),
    }
}
fn full_snapshot(f: &Fixture) -> Snapshot {
    Snapshot {
        meta: openraft::SnapshotMeta {
            last_log_id: Some(log_id(1, 1, 0)),
            last_membership: openraft::StoredMembership::default(),
        },
        snapshot: f.sm.0.lock().unwrap().snapshot(),
    }
}
#[tokio::test]
async fn rpc_ballot_exact_semantics_over_tls() {
    use openraft::network::NetVote;
    for pre in [false, true] {
        for deny in [false, true] {
            let f = Fixture::new();
            let spy = rpc_spy(deny);
            let mut receiver = f.owner(2, spy.clone());
            let address = listening(&mut receiver).await;
            let mut sender = f.owner(1, Spy::exact(None));
            let mut client = sender.loopback_client(2, address);
            let r = ballot_request();
            let expected = peer_wire::encode_vote(
                route(),
                if pre {
                    peer_wire::Kind::PreVoteRequest
                } else {
                    peer_wire::Kind::VoteRequest
                },
                &r,
            )
            .unwrap();
            let result = if pre {
                client
                    .pre_vote(r, RPCOption::new(Duration::from_secs(4)))
                    .await
            } else {
                client.vote(r, RPCOption::new(Duration::from_secs(4))).await
            };
            let a = sender.close().await;
            let b = receiver.close().await;
            assert!(a.is_ok() && b.is_ok() && reclaimed(&sender) && reclaimed(&receiver));
            let reply = result.unwrap();
            assert_eq!(reply.vote_granted, !deny);
            assert_eq!(
                reply.vote,
                if pre {
                    vote(StoredVote {
                        term: 2,
                        node: 1,
                        committed: false,
                    })
                } else {
                    ballot_request().vote
                }
            );
            assert_eq!(reply.last_log_id, Some(log_id(1, 1, 0)));
            assert_eq!(*spy.calls.lock().unwrap(), vec![expected]);
        }
    }
}
#[tokio::test]
async fn rpc_snapshot_exact_blob_and_metadata_over_tls() {
    use openraft::network::NetSnapshot;
    let f = Fixture::new();
    let spy = rpc_spy(false);
    let mut receiver = f.owner(2, spy.clone());
    let address = listening(&mut receiver).await;
    let mut sender = f.owner(1, Spy::exact(None));
    let mut client = sender.loopback_client(2, address);
    let v = heartbeat().vote;
    let s = full_snapshot(&f);
    let expected = peer_wire::encode_snapshot(route(), v, &s).unwrap();
    let result = client
        .full_snapshot(
            v,
            s,
            std::future::pending(),
            RPCOption::new(Duration::from_secs(4)),
        )
        .await;
    let a = sender.close().await;
    let b = receiver.close().await;
    assert!(a.is_ok() && b.is_ok() && reclaimed(&sender) && reclaimed(&receiver));
    assert_eq!(result.unwrap().vote, v);
    assert_eq!(*spy.calls.lock().unwrap(), vec![expected]);
}
#[tokio::test]
async fn rpc_snapshot_independent_mismatches_zero_dispatch() {
    let f = Fixture::new();
    let valid = peer_wire::encode_snapshot(route(), heartbeat().vote, &full_snapshot(&f)).unwrap();
    let mut cases = Vec::new();
    for offset in [52, 60, 61, 69, 77, 85, 86, valid.len() - 1] {
        let mut b = valid.clone();
        b[offset] ^= 1;
        cases.push(b)
    }
    let mut old = valid.clone();
    old[4] = 1;
    cases.push(old);
    for b in cases {
        let spy = rpc_spy(false);
        let mut receiver = f.owner(2, spy.clone());
        let result = raw_request(&f, &mut receiver, &b, false).await;
        let closed = receiver.close().await;
        assert!(closed.is_ok() && reclaimed(&receiver));
        assert!(result.is_err());
        assert!(spy.calls.lock().unwrap().is_empty())
    }
    let mut huge = full_snapshot(&f);
    huge.snapshot = vec![0; 16385];
    assert!(peer_wire::encode_snapshot(route(), heartbeat().vote, &huge).is_err());
}
#[tokio::test]
async fn rpc_ballot_sender_route_and_final_registry_refusals() {
    for pre in [false, true] {
        for substitution in 0..5 {
            let f = Fixture::new();
            let spy = rpc_spy(false);
            let mut receiver = f.owner(2, spy.clone());
            let kind = if pre {
                peer_wire::Kind::PreVoteRequest
            } else {
                peer_wire::Kind::VoteRequest
            };
            let mut r = ballot_request();
            let mut rt = route();
            match substitution {
                0 => r.vote.leader_id.node_id = 2,
                1 => rt.source = 2,
                2 => rt.target = 1,
                3 => rt.cluster = [2; 16],
                _ => {}
            }
            let bytes = peer_wire::encode_vote(rt, kind, &r).unwrap();
            let result = if substitution == 4 {
                let gate = gate(&receiver, Stage::BeforeDispatch);
                let mut events = events(&receiver);
                let addr = listening(&mut receiver).await;
                let config = f.client_config(&f.client, 2);
                let work = tokio::spawn(async move {
                    let mut stream = connect(config, addr).await?;
                    send_frame(&mut stream, &bytes, false).await?;
                    read_frame(&mut stream).await
                });
                let observed = observe(&mut events, Event::Stage(Stage::BeforeDispatch)).await;
                let removed = f.remove(1);
                gate.add_permits(1);
                let out = work.await;
                match out {
                    Ok(out) if observed && removed => out,
                    _ => Err(io::Error::other("observation")),
                }
            } else {
                raw_request(&f, &mut receiver, &bytes, false).await
            };
            let closed = receiver.close().await;
            assert!(closed.is_ok() && reclaimed(&receiver));
            assert!(result.is_err());
            assert!(spy.calls.lock().unwrap().is_empty());
        }
    }
}
#[tokio::test]
async fn rpc_snapshot_cancellation_and_pending_weak_factory() {
    use openraft::network::{NetSnapshot, NetVote, RaftNetworkFactory};
    let f = Fixture::new();
    let credentials = Credentials {
        root: f.root.clone(),
        leaf: f.client.cert.clone(),
        key: f.client.private_key(),
        time: Arc::new(pki::FixedTime(1001)),
    };
    let (mut owner, mut factory) = TransportOwner::pending(CLUSTER, node(1), credentials);
    factory.loopback = true;
    let n = openraft::BasicNode::new("127.0.0.1:43182");
    let mut c = factory.new_client(2, &n).await;
    let h = factory.new_heartbeat_client(2, &n).await;
    let s = factory.new_snapshot_client(2, &n).await;
    let pending = c
        .pre_vote(ballot_request(), RPCOption::new(Duration::from_secs(4)))
        .await;
    let cancel = openraft::errors::ReplicationClosed::new("test cancellation");
    let result = c
        .full_snapshot(
            heartbeat().vote,
            full_snapshot(&f),
            std::future::ready(cancel),
            RPCOption::new(Duration::from_secs(4)),
        )
        .await;
    let untouched = owner.shared.connections.available_permits() == 32
        && owner.shared.handshakes.available_permits() == 4
        && owner.shared.state.lock().unwrap().tasks.is_empty();
    let closed = owner.close().await;
    drop(owner);
    assert!(closed.is_ok() && untouched && pending.is_err());
    assert!(matches!(
        result,
        Err(openraft::errors::StreamingError::Closed(_))
    ));
    assert_eq!(factory.shared.strong_count(), 0);
    drop((h, s));
    assert!(
        c.pre_vote(ballot_request(), RPCOption::new(Duration::from_secs(4)))
            .await
            .is_err()
    );
}
// These FOUR tests compile in this assignment and MUST NOT execute before
// independent exact-source review. No founder, initialize, election or retry.
fn seed_learner(
    dir: &std::path::Path,
    f: &Fixture,
    m: &super::super::MembershipBlob,
    tail: Option<LogPos>,
    releases: &mut Vec<crate::cluster::state_store::Released>,
) -> Result<Vec<u8>, &'static str> {
    use super::super::VoteLogStore;
    let init = Init {
        cluster: CLUSTER,
        first_free: node(3),
    };
    let e = StoredEntry::Enrollment(Command::Found {
        cluster: CLUSTER,
        members: FoundMembers::new([(node(1), f.client.digest()), (node(2), f.server.digest())])
            .map_err(|_| "found members")?,
        proposed_at_ms: 100,
    });
    let l0 = LogPos {
        term: 1,
        leader: 1,
        index: 0,
    };
    let l1 = LogPos { index: 1, ..l0 };
    let mut sm = StateStore::open(dir, init).map_err(|_| "seed sm open")?;
    releases.push(sm.released());
    if !matches!(
        sm.apply(l0, &e),
        Ok(Some(Outcome::Founded { replay: false }))
    ) {
        return Err("seed Found");
    }
    sm.apply(l1, &StoredEntry::Membership(m.clone()))
        .map_err(|_| "seed membership")?;
    let mut rows = vec![(l0, e), (l1, StoredEntry::Membership(m.clone()))];
    if let Some(p) = tail {
        sm.apply(p, &StoredEntry::Blank).map_err(|_| "seed tail")?;
        rows.push((p, StoredEntry::Blank))
    }
    let snapshot = sm.snapshot();
    let sr = sm.released();
    let mut log = VoteLogStore::open(dir, CLUSTER).map_err(|_| "seed log open")?;
    let lr = log.released();
    releases.push(lr.clone());
    log.append(&rows).map_err(|_| "seed log append")?;
    let v = StoredVote {
        term: if tail.is_some() { 3 } else { 2 },
        node: 1,
        committed: tail.is_some(),
    };
    log.save_vote(v).map_err(|_| "seed vote")?;
    if log.entries(0, 3, 3).map_err(|_| "seed read")? != rows
        || log.vote().map_err(|_| "seed read vote")? != Some(v)
    {
        return Err("seed readback");
    }
    drop(log);
    drop(sm);
    if !sr.is_set() || !lr.is_set() {
        return Err("seed release");
    }
    Ok(snapshot)
}
// Process-persistent ownership: cancellation of an observer never drops its worker.
// Entries are bounded by the four once-only histories. Incomplete joins retain
// the entire case; no background cleanup, abort, or clean result is fabricated.
type LearnerWorker = Arc<tokio::sync::Mutex<tokio::task::JoinHandle<Result<(), &'static str>>>>;
static LEARNER_WORKERS: Mutex<Vec<LearnerWorker>> = Mutex::new(Vec::new());
static LEARNER_EVIDENCE: Mutex<BTreeMap<u8, Vec<u8>>> = Mutex::new(BTreeMap::new());
static LEARNER_RETAINED: Mutex<Vec<Box<dyn std::any::Any + Send>>> = Mutex::new(Vec::new());
#[derive(Default, Debug, serde::Serialize)]
struct LearnerEvidence {
    #[serde(skip)]
    output: Option<std::path::PathBuf>,
    setup: Option<SetupEvidence>,
    artifact_error: Option<&'static str>,
    first: Option<&'static str>,
    startup: Option<&'static str>,
    body: Option<Result<(), &'static str>>,
    transport: Option<(String, String)>,
    reclaimed: Option<(bool, bool)>,
    shutdown: Option<serde_json::Value>,
    released: Option<(bool, bool)>,
    readback: Option<Result<(), &'static str>>,
    readback_observed: Option<bool>,
    readback_released: Option<(bool, bool, bool)>,
    weak_refused: Option<bool>,
}
#[derive(Debug, serde::Serialize)]
struct SetupEvidence {
    outcome: Result<(), &'static str>,
    dropped_reservations: Option<usize>,
    registered_store_releases: Vec<bool>,
    reopen: &'static str,
    unregistered_resources: &'static str,
}
type SetupAttempt<T> = Result<Result<T, &'static str>, Box<dyn std::any::Any + Send>>;
fn finish_learner_setup<T>(
    case: u8,
    evidence: &mut LearnerEvidence,
    attempt: SetupAttempt<T>,
    reservations: usize,
    releases: &[crate::cluster::state_store::Released],
) -> Result<T, &'static str> {
    let result = attempt.unwrap_or(Err("setup panic; unregistered cleanup unconfirmed"));
    evidence.setup = Some(SetupEvidence {
        outcome: result.as_ref().map(|_| ()).map_err(|e| *e),
        dropped_reservations: result.is_err().then_some(reservations),
        registered_store_releases: releases.iter().map(|r| r.is_set()).collect(),
        reopen: if result.is_err() {
            "not attempted: seed may be partial; reopen unconfirmed"
        } else {
            "deferred to normal joined teardown"
        },
        unregistered_resources: if result.is_err() {
            "setup future dropped; unregistered release unconfirmed"
        } else {
            "setup owners transferred to supervised history"
        },
    });
    if let Err(reason) = &result {
        evidence.fail(reason);
    }
    evidence.checkpoint(case);
    match result {
        Err(_) => Err(evidence.first.unwrap()),
        Ok(value) if evidence.first.is_some() => {
            // An unusable evidence destination must stop before Engine startup.
            drop(value);
            let reason = evidence.first.unwrap();
            let setup = evidence.setup.as_mut().unwrap();
            setup.outcome = Err(reason);
            setup.dropped_reservations = Some(reservations);
            setup.registered_store_releases = releases.iter().map(|r| r.is_set()).collect();
            setup.reopen = "not attempted: evidence failed; reopen unconfirmed";
            setup.unregistered_resources = "setup owners dropped before Engine startup";
            evidence.checkpoint(case);
            Err(reason)
        }
        Ok(value) => Ok(value),
    }
}
fn learner_evidence_root(
    requested: Option<std::ffi::OsString>,
) -> Result<std::path::PathBuf, &'static str> {
    // Local commands set TMP/TEMP on D:. Hosted runners use their private temp.
    // Never fall back after a malformed explicit root.
    let root = match requested {
        Some(root) => std::path::PathBuf::from(root),
        None => {
            let temp = std::env::temp_dir()
                .canonicalize()
                .map_err(|_| "observer temp root")?;
            tempfile::Builder::new()
                .prefix("arx-m3-evidence-")
                .tempdir_in(temp)
                .map_err(|_| "observer private directory")?
                .keep()
        }
    };
    if !root.is_absolute() || !root.is_dir() {
        return Err("unsafe observer root");
    }
    for ancestor in root.ancestors() {
        if observer_reparse(ancestor)? {
            return Err("observer reparse point");
        }
    }
    Ok(root)
}
impl LearnerEvidence {
    fn checkpoint(&mut self, case: u8) {
        if self.output.is_none() {
            match learner_evidence_root(std::env::var_os("ARX_OBSERVER_ROOT")) {
                Ok(root) => self.output = Some(root),
                Err(reason) => self.fail(reason),
            }
        }
        if let Err(reason) = self.save(case) {
            self.artifact_error = Some(reason);
            self.fail("evidence artifact write failed");
        }
        let bytes = serde_json::to_vec(self).expect("bounded static evidence");
        // Failure evidence survives artifact errors in memory AND captured logs.
        if self.first.is_some() {
            eprintln!("ARX learner evidence: {}", String::from_utf8_lossy(&bytes));
        }
        LEARNER_EVIDENCE.lock().unwrap().insert(case, bytes);
    }
    fn fail(&mut self, reason: &'static str) {
        self.first.get_or_insert(reason);
    }
    fn save(&self, case: u8) -> Result<(), &'static str> {
        let root = self.output.as_ref().ok_or("observer root")?;
        for ancestor in root.ancestors() {
            if observer_reparse(ancestor)? {
                return Err("observer reparse point");
            }
        }
        let destination = root.join(format!("learner-{case}-evidence.json"));
        match std::fs::symlink_metadata(&destination) {
            Ok(_) if observer_reparse(&destination)? => return Err("evidence reparse point"),
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(_) => return Err("evidence metadata"),
        }
        std::fs::write(
            destination,
            serde_json::to_vec_pretty(self).map_err(|_| "evidence encode")?,
        )
        .map_err(|_| "evidence write")
    }
}
fn observer_reparse(path: &std::path::Path) -> Result<bool, &'static str> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| "observer metadata")?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        Ok(metadata.file_attributes() & 0x400 != 0)
    }
    #[cfg(not(windows))]
    {
        Ok(metadata.file_type().is_symlink())
    }
}
async fn supervised_learner_once(case: u8) -> Result<(), &'static str> {
    let worker = {
        let mut workers = LEARNER_WORKERS.lock().map_err(|_| "supervisor lock")?;
        if case >= 4 || workers.len() >= 4 {
            return Err("once-only supervisor full");
        }
        let worker = Arc::new(tokio::sync::Mutex::new(tokio::spawn(async move {
            learner_once(case).await
        })));
        workers.push(worker.clone());
        worker
    };
    observe_learner_worker(worker).await
}
async fn observe_learner_worker(worker: LearnerWorker) -> Result<(), &'static str> {
    let mut handle = worker.lock().await;
    (&mut *handle).await.map_err(|_| "supervisor join")?
}
async fn learner_once(case: u8) -> Result<(), &'static str> {
    use super::super::{ConsensusNode, MembershipBlob, Params, VoteLogStore, WireMembership};
    use futures_util::FutureExt;
    use openraft::network::{NetSnapshot, NetVote, RaftNetworkFactory};
    let mut evidence = LearnerEvidence::default();
    evidence.checkpoint(case);
    let mut reservations = 0;
    let mut setup_releases = Vec::new();
    // Every fallible pre-Engine step is in one scope. Errors and unwinding drop
    // its sockets/stores before the terminal checkpoint observes registered acks.
    let setup = std::panic::AssertUnwindSafe(async {
        let mut f = Fixture::new();
        setup_releases.push(
            f.sm.0
                .lock()
                .map_err(|_| "fixture release observer")?
                .released(),
        );
        let loopback = SocketAddr::from(([127, 0, 0, 1], 0));
        let sl = TcpListener::bind(loopback)
            .await
            .map_err(|_| "sender reservation")?;
        reservations += 1;
        let rl = TcpListener::bind(loopback)
            .await
            .map_err(|_| "receiver reservation")?;
        reservations += 1;
        let sa = sl.local_addr().map_err(|_| "sender address")?;
        let ra = rl.local_addr().map_err(|_| "receiver address")?;
        if sa.ip() != loopback.ip()
            || ra.ip() != loopback.ip()
            || sa.port() == 0
            || ra.port() == 0
            || sa == ra
        {
            return Err("reservation identity");
        }
        let m = MembershipBlob::new(
            WireMembership::new(
                vec![std::collections::BTreeSet::from([1])],
                BTreeMap::from([(1, sa.to_string()), (2, ra.to_string())]),
            )
            .map_err(|_| "membership")?
            .encode()
            .map_err(|_| "membership encoding")?,
        )
        .map_err(|_| "membership bounds")?;
        let sd = private_temp().keep();
        let rd = private_temp().keep();
        let original = seed_learner(&rd, &f, &m, None, &mut setup_releases)?;
        let sender_original = seed_learner(&sd, &f, &m, None, &mut setup_releases)?;
        let donor_snapshot = if case == 3 {
            let donor = private_temp().keep();
            Some(seed_learner(
                &donor,
                &f,
                &m,
                Some(LogPos {
                    term: 1,
                    leader: 1,
                    index: 2,
                }),
                &mut setup_releases,
            )?)
        } else {
            None
        };
        let init = Init {
            cluster: CLUSTER,
            first_free: node(3),
        };
        f.sm = SmAdapter(Arc::new(Mutex::new(
            StateStore::open(&sd, init).map_err(|_| "sender reopen")?,
        )));
        let mut original_enrollment = Vec::new();
        crate::cluster::enrollment::codec::encode(
            f.sm.0.lock().map_err(|_| "sender enrollment lock")?.state(),
            &mut original_enrollment,
        );
        let sender_release = f.sm.0.lock().map_err(|_| "sender lock")?.released();
        setup_releases.push(sender_release.clone());
        let sender_weak = Arc::downgrade(&f.sm.0);
        let credentials = |identity: &Identity| Credentials {
            root: f.root.clone(),
            leaf: identity.cert.clone(),
            key: identity.private_key(),
            time: Arc::new(pki::FixedTime(1001)),
        };
        let (sender, mut sf) = TransportOwner::pending(CLUSTER, node(1), credentials(&f.client));
        sf.loopback = true;
        let (receiver, mut rf) = TransportOwner::pending(CLUSTER, node(2), credentials(&f.server));
        rf.loopback = true;
        let mut pending = sf
            .new_client(2, &openraft::BasicNode::new(ra.to_string()))
            .await;
        let pending_refused = pending
            .pre_vote(ballot_request(), RPCOption::new(Duration::from_secs(4)))
            .await
            .is_err();
        Ok((
            f,
            sa,
            ra,
            sl,
            rl,
            m,
            sd,
            rd,
            original,
            sender_original,
            donor_snapshot,
            init,
            original_enrollment,
            sender_release,
            sender_weak,
            sender,
            sf,
            receiver,
            rf,
            pending,
            pending_refused,
        ))
    })
    .catch_unwind()
    .await;
    let (
        f,
        sa,
        ra,
        sl,
        rl,
        m,
        sd,
        rd,
        original,
        sender_original,
        donor_snapshot,
        init,
        original_enrollment,
        sender_release,
        sender_weak,
        mut sender,
        mut sf,
        mut receiver,
        rf,
        mut pending,
        pending_refused,
    ) = finish_learner_setup(case, &mut evidence, setup, reservations, &setup_releases)?;
    let rdir = rd.clone();
    let mut startup =
        tokio::spawn(
            async move { ConsensusNode::start(2, &rdir, init, Params::default(), rf).await },
        );
    // Preserve sole startup handle on observation expiry. Never abort/drop/retry it.
    let (startup_observed, started) = match timeout(OBSERVE, &mut startup).await {
        Ok(r) => (true, r),
        Err(_) => {
            evidence.startup = Some("observation expired; startup worker retained");
            evidence.fail("startup observation expired");
            evidence.checkpoint(case);
            (false, startup.await)
        }
    };
    evidence.startup = Some(if startup_observed {
        "observed"
    } else {
        "observation expired; worker joined"
    });
    if !startup_observed {
        evidence.fail("startup observation expired");
    }
    evidence.checkpoint(case);
    let mut receiver_node = match started {
        Ok(Ok(n)) => n,
        failure => {
            evidence.startup = Some(if failure.is_err() {
                "startup worker join failed"
            } else {
                "startup Fatal before node returned"
            });
            evidence.fail("startup failed; Engine cleanup unconfirmed");
            let sc = sender.close().await;
            let rc = receiver.close().await;
            evidence.transport = Some((format!("{sc:?}"), format!("{rc:?}")));
            evidence.reclaimed = Some((reclaimed(&sender), reclaimed(&receiver)));
            evidence.checkpoint(case);
            // Pre-return Fatal exposes no Engine/release handle. Only attempt one
            // owned offline reopen after transport joins; it cannot erase Fatal.
            if sender.joined() && receiver.joined() {
                let rdir = rd.clone();
                let mut reopen =
                    tokio::task::spawn_blocking(move || -> Result<(), &'static str> {
                        let log =
                            VoteLogStore::open(&rdir, CLUSTER).map_err(|_| "fatal log reopen")?;
                        let sm = StateStore::open(&rdir, init).map_err(|_| "fatal state reopen")?;
                        let lr = log.released();
                        let sr = sm.released();
                        drop(log);
                        drop(sm);
                        if !lr.is_set() || !sr.is_set() {
                            return Err("fatal reopened stores release");
                        }
                        Ok(())
                    });
                let result = match timeout(OBSERVE, &mut reopen).await {
                    Ok(r) => {
                        evidence.readback_observed = Some(true);
                        r
                    }
                    Err(_) => {
                        evidence.readback_observed = Some(false);
                        evidence.checkpoint(case);
                        reopen.await
                    }
                };
                evidence.readback = Some(result.unwrap_or(Err("fatal readback worker join")));
                evidence.checkpoint(case);
            }
            LEARNER_RETAINED
                .lock()
                .unwrap()
                .push(Box::new((sender, receiver, f, sl, rl, sf, pending)));
            return Err(evidence.first.unwrap());
        }
    };
    let log_release = receiver_node.log_released.clone();
    let sm_release = receiver_node.sm_released.clone();
    let log_weak = receiver_node.log_weak.clone();
    let sm_weak = receiver_node.sm_weak.clone();
    let mut expected_snapshot = if case < 2 {
        Some(original.clone())
    } else {
        None
    };
    let outcome = std::panic::AssertUnwindSafe(async {
        if !startup_observed || !pending_refused {
            return Err("startup/Pending observation");
        }
        let sm = receiver_node.sm.as_ref().ok_or("receiver sm")?;
        let raft = receiver_node.raft.as_ref().ok_or("receiver raft")?;
        receiver
            .attach_reserved(
                sm,
                Arc::new(super::super::peer_network::EngineDispatch { raft: raft.clone() }),
                rl,
                ra,
            )
            .map_err(|_| "receiver attach")?;
        sender
            .attach_reserved(&f.sm, Spy::exact(None), sl, sa)
            .map_err(|_| "sender attach")?;
        let metrics = raft.metrics();
        let initial = metrics.borrow_watched().clone();
        drop(metrics);
        let expected_membership = openraft::StoredMembership::new(
            Some(log_id(1, 1, 1)),
            super::super::decode(&m).map_err(|_| "initial membership decode")?,
        );
        if *initial.membership_config != expected_membership
            || *initial.committed_membership_config != expected_membership
            || initial.state != openraft::ServerState::Learner
            || initial.last_applied != Some(log_id(1, 1, 1))
            || initial.local_committed != Some(log_id(1, 1, 1))
            || initial.vote
                != vote(StoredVote {
                    term: 2,
                    node: 1,
                    committed: false,
                })
        {
            return Err("initial learner state");
        }
        let mut c = sf
            .new_client(2, &openraft::BasicNode::new(ra.to_string()))
            .await;
        let v3 = vote(StoredVote {
            term: 3,
            node: 1,
            committed: case >= 2,
        });
        let l1 = log_id(1, 1, 1);
        match case {
            0 | 1 => {
                let r = Ballot {
                    vote: v3,
                    last_log_id: Some(l1),
                    leadership_transfer_from: None,
                };
                let reply = if case == 0 {
                    c.pre_vote(r, RPCOption::new(Duration::from_secs(4))).await
                } else {
                    c.vote(r, RPCOption::new(Duration::from_secs(4))).await
                }
                .map_err(|_| "learner ballot RPC")?;
                if reply
                    != (BallotReply {
                        vote: if case == 0 {
                            vote(StoredVote {
                                term: 2,
                                node: 1,
                                committed: false,
                            })
                        } else {
                            v3
                        },
                        vote_granted: true,
                        last_log_id: Some(l1),
                    })
                {
                    return Err("learner exact ballot reply");
                }
            }
            2 => {
                let a2 = log_id(3, 1, 2);
                let r = AppendEntriesRequest {
                    vote: v3,
                    prev_log_id: Some(l1),
                    entries: vec![Entry {
                        log_id: a2,
                        payload: EntryPayload::Blank,
                    }],
                    leader_commit: Some(a2),
                };
                if c.append_entries(r, RPCOption::new(Duration::from_secs(4)))
                    .await
                    .map_err(|_| "learner append RPC")?
                    != Ok(Some(a2))
                {
                    return Err("learner exact append");
                }
                raft.wait(Some(OBSERVE))
                    .applied_index(Some(2), "learner append applied")
                    .await
                    .map_err(|_| "append application observation")?;
            }
            3 => {
                let bs = donor_snapshot.clone().ok_or("snapshot donor")?;
                let meta = openraft::SnapshotMeta {
                    last_log_id: Some(log_id(1, 1, 2)),
                    last_membership: openraft::StoredMembership::new(
                        Some(l1),
                        super::super::decode(&m).map_err(|_| "snapshot membership")?,
                    ),
                };
                let mut c = sf
                    .new_snapshot_client(2, &openraft::BasicNode::new(ra.to_string()))
                    .await;
                let reply = c
                    .full_snapshot(
                        v3,
                        Snapshot {
                            meta,
                            snapshot: bs.clone(),
                        },
                        std::future::pending(),
                        RPCOption::new(Duration::from_secs(4)),
                    )
                    .await
                    .map_err(|_| "learner snapshot RPC")?;
                expected_snapshot = Some(bs.clone());
                if reply.vote != v3 {
                    return Err("snapshot exact reply");
                }
                raft.wait(Some(OBSERVE))
                    .applied_index(Some(2), "learner snapshot applied")
                    .await
                    .map_err(|_| "snapshot applied observation")?;
                raft.wait(Some(OBSERVE))
                    .purged(Some(log_id(1, 1, 2)), "learner snapshot purge")
                    .await
                    .map_err(|_| "snapshot purge observation")?;
                if sm
                    .0
                    .try_lock()
                    .map_err(|_| "snapshot read lock")?
                    .snapshot()
                    != bs
                {
                    return Err("snapshot installed bytes");
                }
            }
            _ => return Err("case"),
        }
        Ok(())
    })
    .catch_unwind()
    .await;
    // No test assertions/panic before explicit joins and unchanged node shutdown.
    let body = outcome.unwrap_or(Err("fixture body panic"));
    evidence.body = Some(body);
    if let Err(reason) = body {
        evidence.fail(reason);
    }
    evidence.checkpoint(case);
    let sc = timeout(Duration::from_secs(15), sender.close())
        .await
        .unwrap_or(Err(Refusal::Deadline));
    let rc = timeout(Duration::from_secs(15), receiver.close())
        .await
        .unwrap_or(Err(Refusal::Deadline));
    evidence.transport = Some((format!("{sc:?}"), format!("{rc:?}")));
    evidence.reclaimed = Some((reclaimed(&sender), reclaimed(&receiver)));
    if sc.is_err() || rc.is_err() {
        evidence.fail("transport close failed");
    }
    evidence.checkpoint(case);
    if !sender.joined() || !receiver.joined() {
        evidence.fail("transport cleanup unconfirmed; node shutdown withheld");
        evidence.checkpoint(case);
        LEARNER_RETAINED.lock().unwrap().push(Box::new((
            sender,
            receiver,
            receiver_node,
            f,
            sf,
            pending,
        )));
        return Err(evidence.first.unwrap());
    }
    let shutdown = receiver_node.shutdown().await;
    evidence.shutdown = Some(match &shutdown {
        Ok(()) => serde_json::json!({"outcome": "Ok"}),
        Err(super::super::Orphaned(r)) => serde_json::json!({
            "outcome": "Orphaned", "first": format!("{:?}", r.first),
            "engine": format!("{:?}", r.engine), "engine_ms": r.engine_ms,
            "owners_done_at_start": r.owners_done_at_start, "owners_waited": r.owners_waited,
            "owners_left": r.owners_left, "owners_panicked": r.owners_panicked,
            "last_owner_join_ms": r.last_owner_join_ms, "propose_held": r.propose_held,
            "read_held": r.read_held, "log_strong": r.log_strong, "sm_strong": r.sm_strong,
            "total_ms": r.total_ms
        }),
    });
    if shutdown.is_err() {
        evidence.fail("node shutdown failed");
    }
    let owners_released = log_release.is_set()
        && sm_release.is_set()
        && log_weak.strong_count() == 0
        && sm_weak.strong_count() == 0;
    drop(receiver_node);
    drop(f.sm);
    let sender_released = sender_release.is_set() && sender_weak.strong_count() == 0;
    evidence.released = Some((owners_released, sender_released));
    if !owners_released || !sender_released {
        evidence.fail("store release incomplete");
    }
    evidence.checkpoint(case);
    // Weak factory/client handles remain live during exact same-directory readback.
    let expected_rows = {
        let e = StoredEntry::Enrollment(Command::Found {
            cluster: CLUSTER,
            members: FoundMembers::new([
                (node(1), f.client.digest()),
                (node(2), f.server.digest()),
            ])
            .map_err(|_| "readback key members")?,
            proposed_at_ms: 100,
        });
        let mut rows = vec![
            (
                LogPos {
                    term: 1,
                    leader: 1,
                    index: 0,
                },
                e,
            ),
            (
                LogPos {
                    term: 1,
                    leader: 1,
                    index: 1,
                },
                StoredEntry::Membership(m.clone()),
            ),
        ];
        if case == 2 {
            rows.push((
                LogPos {
                    term: 3,
                    leader: 1,
                    index: 2,
                },
                StoredEntry::Blank,
            ))
        }
        if case == 3 {
            rows.clear()
        }
        rows
    };
    let k1 = f.client.digest();
    let k2 = f.server.digest();
    let mut readback = tokio::task::spawn_blocking(move || {
        let mut log = VoteLogStore::open(&rd, CLUSTER).map_err(|_| "receiver log reopen")?;
        let sm = StateStore::open(&rd, init).map_err(|_| "receiver state reopen")?;
        let lr = log.released();
        let sr = sm.released();
        let receiver_result = (|| -> Result<(), &'static str> {
            let expected = LogPos {
                term: if case == 2 { 3 } else { 1 },
                leader: 1,
                index: if case >= 2 { 2 } else { 1 },
            };
            if sm.applied() != Some(expected)
                || sm.membership()
                    != Some(&(
                        LogPos {
                            term: 1,
                            leader: 1,
                            index: 1,
                        },
                        m,
                    ))
            {
                return Err("reopened position/membership");
            }
            if log.vote().map_err(|_| "reopen vote")?
                != Some(StoredVote {
                    term: if case == 0 { 2 } else { 3 },
                    node: 1,
                    committed: case >= 2,
                })
            {
                return Err("persisted vote");
            }
            if expected_snapshot.is_some_and(|b| sm.snapshot() != b) {
                return Err("reopened snapshot bytes");
            }
            if log.entries(0, 3, 3).map_err(|_| "reopened rows")? != expected_rows {
                return Err("reopened exact rows");
            }
            if sm.state().authorize_peer(CLUSTER, node(1), k1).is_err()
                || sm.state().authorize_peer(CLUSTER, node(2), k2).is_err()
            {
                return Err("reopened applied registry");
            }

            let state = log.log_state().map_err(|_| "reopened log state")?;
            if state.last != Some(expected)
                || state.purged != if case == 3 { Some(expected) } else { None }
            {
                return Err("reopened log bounds");
            }
            if log.live().map_err(|_| "reopen live")?
                != if case == 3 {
                    0
                } else if case == 2 {
                    3
                } else {
                    2
                }
            {
                return Err("reopened row count");
            }
            super::super::preflight(&mut log, &sm).map_err(|_| "reopened preflight")?;
            let mut enrollment = Vec::new();
            crate::cluster::enrollment::codec::encode(sm.state(), &mut enrollment);
            if enrollment != original_enrollment {
                return Err("receiver enrollment bytes changed");
            }
            Ok(())
        })();
        drop(log);
        drop(sm);
        let sender_result = (|| -> Result<bool, &'static str> {
            let sender = StateStore::open(&sd, init).map_err(|_| "sender state reopen")?;
            if sender.snapshot() != sender_original {
                return Err("sender seed changed");
            }
            let ss = sender.released();
            drop(sender);
            Ok(ss.is_set())
        })();
        let releases = (
            lr.is_set(),
            sr.is_set(),
            sender_result.as_ref().is_ok_and(|v| *v),
        );
        let result = receiver_result.and(sender_result.map(|_| ()));
        Ok::<_, &'static str>((result, releases))
    });
    let (readback_observed, reopened) = match timeout(OBSERVE, &mut readback).await {
        Ok(r) => (true, r),
        Err(_) => {
            evidence.readback_observed = Some(false);
            evidence.fail("readback observation expired; worker retained");
            evidence.checkpoint(case);
            (false, readback.await)
        }
    };
    let reopened = match reopened.unwrap_or(Err("readback worker join")) {
        Ok((result, releases)) => {
            evidence.readback_released = Some(releases);
            if let Err(reason) = result {
                evidence.fail(reason);
            }
            if releases != (true, true, true) {
                evidence.fail("reopened stores not released");
            }
            result
        }
        Err(reason) => Err(reason),
    };
    evidence.readback = Some(reopened);
    evidence.readback_observed = Some(readback_observed);
    if !readback_observed {
        evidence.fail("readback observation expired; worker joined");
    }
    if let Err(reason) = reopened {
        evidence.fail(reason);
    }
    let weak_refused = pending
        .pre_vote(ballot_request(), RPCOption::new(Duration::from_secs(4)))
        .await
        .is_err();
    evidence.weak_refused = Some(weak_refused);
    if !weak_refused {
        evidence.fail("closed weak client accepted");
    }
    if !reclaimed(&sender) || !reclaimed(&receiver) {
        evidence.fail("transport units not reclaimed");
    }
    evidence.checkpoint(case);
    // Factories and weak clients outlive all reopen oracles.
    drop((sf, pending));
    match evidence.first {
        Some(reason) => Err(reason),
        None => Ok(()),
    }
}
#[tokio::test]
async fn rpc_learner_p_once() {
    assert_eq!(supervised_learner_once(0).await, Ok(()))
}
#[tokio::test]
async fn rpc_learner_v_once() {
    assert_eq!(supervised_learner_once(1).await, Ok(()))
}
#[tokio::test]
async fn rpc_learner_a_once() {
    assert_eq!(supervised_learner_once(2).await, Ok(()))
}
#[tokio::test]
async fn rpc_learner_s_once() {
    assert_eq!(supervised_learner_once(3).await, Ok(()))
}
#[tokio::test]
async fn rpc_snapshot_cancel_owned_future_at_tls_and_reply_phases() {
    use openraft::network::NetSnapshot;
    for phase in [
        Stage::ClientHandshake,
        Stage::ClientAuthenticated,
        Stage::ReplyRead,
        Stage::SnapshotPartialBody,
    ] {
        let f = Fixture::new();
        let spy = rpc_spy(false);
        let mut receiver = f.owner(2, spy.clone());
        let address = listening(&mut receiver).await;
        let mut sender = f.owner(1, Spy::exact(None));
        let hold = gate(&sender, phase);
        let mut events = events(&sender);
        let mut client = sender.loopback_client(2, address);
        let snapshot = full_snapshot(&f);
        let (cancel, rx) = oneshot::channel();
        let reason = openraft::errors::ReplicationClosed::new("exact owned cancel");
        let expected = reason.clone();
        let call = tokio::spawn(async move {
            client
                .full_snapshot(
                    heartbeat().vote,
                    snapshot,
                    async move { rx.await.unwrap_or(reason) },
                    RPCOption::new(Duration::from_secs(4)),
                )
                .await
        });
        let reached = observe(&mut events, Event::Stage(phase)).await;
        let charged = sender.shared.connections.available_permits() == 31
            && (phase != Stage::SnapshotPartialBody
                || sender.shared.handshakes.available_permits() == 4);
        let _ = cancel.send(expected.clone());
        let result = call.await;
        release(&sender, phase, &hold);
        let a = sender.close().await;
        let b = receiver.close().await;
        assert!(
            reached
                && charged
                && a.is_ok()
                && b.is_ok()
                && reclaimed(&sender)
                && reclaimed(&receiver)
        );
        assert!(
            matches!(result,Ok(Err(openraft::errors::StreamingError::Closed(ref c))) if *c==expected)
        );
        if phase != Stage::ReplyRead {
            assert!(spy.calls.lock().unwrap().is_empty())
        } else {
            assert_eq!(spy.calls.lock().unwrap().len(), 1)
        }
    }
}
#[tokio::test]
async fn rpc_attach_once_close_and_observed_dispatch_failure() {
    use openraft::network::{NetVote, RaftNetworkFactory};
    let f = Fixture::new();
    let credentials = || Credentials {
        root: f.root.clone(),
        leaf: f.server.cert.clone(),
        key: f.server.private_key(),
        time: Arc::new(pki::FixedTime(1001)),
    };
    let (mut owner, factory) = TransportOwner::pending(CLUSTER, node(2), credentials());
    let reserved = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = reserved.local_addr().unwrap();
    let first = owner.attach_reserved(&f.sm, Spy::exact(None), reserved, address);
    let again = owner.begin_attach();
    let health = owner.health();
    let mut sender = f.owner(1, Spy::exact(None));
    let mut client = sender.loopback_client(2, address);
    let result = client
        .pre_vote(ballot_request(), RPCOption::new(Duration::from_secs(4)))
        .await;
    let failed = owner.health();
    let closed = owner.close().await;
    let sender_closed = sender.close().await;
    let after_close = owner.begin_attach();
    drop(owner);
    assert!(
        first.is_ok()
            && again.is_err()
            && health == Health::Ready
            && result.is_err()
            && failed == Health::EngineUnavailable
            && closed.is_ok()
            && sender_closed.is_ok()
            && after_close.is_err()
    );
    assert_eq!(factory.shared.strong_count(), 0);
    let (mut failed, mut factory) = TransportOwner::pending(CLUSTER, node(2), credentials());
    factory.loopback = true;
    let held = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = held.local_addr().unwrap();
    // Production endpoint refusal is terminal too, without consuming the reservation.
    let result = failed
        .attach(&f.sm, Spy::exact(None), &address.to_string())
        .await;
    let duplicate = failed.begin_attach();
    let mut client = factory
        .new_client(1, &openraft::BasicNode::new(address.to_string()))
        .await;
    let refused = client
        .pre_vote(ballot_request(), RPCOption::new(Duration::from_secs(4)))
        .await;
    let closed = failed.close().await;
    drop(held);
    assert!(
        result.is_err()
            && duplicate.is_err()
            && refused.is_err()
            && closed.is_ok()
            && reclaimed(&failed)
    );
}
#[tokio::test]
async fn rpc_snapshot_does_not_hold_heartbeat_client_and_stream_is_sequential() {
    use futures_util::StreamExt;
    use openraft::network::{NetSnapshot, NetStreamAppend, NetTransferLeader};
    let f = Fixture::new();
    let hold = Arc::new(Semaphore::new(0));
    let (entered, rx) = oneshot::channel();
    let spy = Arc::new(RpcSpy {
        calls: Mutex::new(Vec::new()),
        deny: false,
        snapshot_vote: None,
        snapshot_gate: Some(hold.clone()),
        snapshot_entered: Mutex::new(Some(entered)),
    });
    let mut receiver = f.owner(2, spy.clone());
    let address = listening(&mut receiver).await;
    let mut sender = f.owner(1, Spy::exact(None));
    let mut snapshot_client = sender.loopback_client(2, address);
    let mut heartbeat_client = sender.loopback_client(2, address);
    let snapshot = full_snapshot(&f);
    let pending = tokio::spawn(async move {
        snapshot_client
            .full_snapshot(
                heartbeat().vote,
                snapshot,
                std::future::pending(),
                RPCOption::new(Duration::from_secs(4)),
            )
            .await
    });
    let observed = timeout(OBSERVE, rx).await.is_ok_and(|r| r.is_ok());
    let heartbeat_result = heartbeat_client
        .append_entries(heartbeat(), RPCOption::new(Duration::from_secs(4)))
        .await;
    hold.add_permits(1);
    let snapshot_result = pending.await;
    let input = vec![heartbeat(), entries()];
    let expected: Vec<_> = input
        .iter()
        .map(|r| peer_wire::encode_request(route(), r).unwrap())
        .collect();
    let output = match heartbeat_client
        .stream_append(
            futures_util::stream::iter(input),
            RPCOption::new(Duration::from_secs(4)),
        )
        .await
    {
        Ok(stream) => stream.collect::<Vec<_>>().await,
        Err(e) => vec![Err(e)],
    };
    let before = sender.shared.connections.available_permits();
    let transfer = heartbeat_client
        .transfer_leader(
            openraft::raft::TransferLeaderRequest::new(heartbeat().vote, 2, None),
            RPCOption::new(Duration::from_secs(4)),
        )
        .await;
    let unchanged = sender.shared.connections.available_permits() == before;
    let a = sender.close().await;
    let b = receiver.close().await;
    assert!(a.is_ok() && b.is_ok() && reclaimed(&sender) && reclaimed(&receiver));
    assert!(
        observed
            && matches!(heartbeat_result, Ok(Ok(None)))
            && matches!(snapshot_result, Ok(Ok(_)))
            && transfer.is_err()
            && unchanged
    );
    assert_eq!(output.len(), 2);
    assert!(matches!(output[0], Ok(Ok(None))));
    assert!(matches!(output[1],Ok(Ok(Some(p))) if p==log_id(7,1,1)));
    assert_eq!(&spy.calls.lock().unwrap()[2..], expected.as_slice());
    // A protocol conflict terminates the pinned sequential stream after one RPC.
    let spy = Spy::exact(Some(Err(AppendError::Conflict {
        log_id: log_id(1, 1, 0),
        hint: None,
    })));
    let mut receiver = f.owner(2, spy.clone());
    let address = listening(&mut receiver).await;
    let mut sender = f.owner(1, Spy::exact(None));
    let mut c = sender.loopback_client(2, address);
    let mut r = heartbeat();
    r.prev_log_id = Some(log_id(1, 1, 0));
    let output = match c
        .stream_append(
            futures_util::stream::iter(vec![r, heartbeat()]),
            RPCOption::new(Duration::from_secs(4)),
        )
        .await
    {
        Ok(stream) => stream.collect::<Vec<_>>().await,
        Err(e) => vec![Err(e)],
    };
    let a = sender.close().await;
    let b = receiver.close().await;
    assert!(a.is_ok() && b.is_ok() && reclaimed(&sender) && reclaimed(&receiver));
    assert_eq!(output.len(), 1);
    assert!(matches!(output[0], Ok(Err(AppendError::Conflict { .. }))));
    assert_eq!(spy.count(), 1);
}
#[tokio::test]
async fn rpc_all_new_methods_fail_closed_on_final_registry_remove_contention_and_poison() {
    for method in 0..3 {
        for failure in 0..3 {
            let f = Fixture::new();
            let spy = rpc_spy(false);
            let mut receiver = f.owner(2, spy.clone());
            let hold = gate(&receiver, Stage::BeforeDispatch);
            let mut events = events(&receiver);
            let address = listening(&mut receiver).await;
            let bytes = if method == 2 {
                peer_wire::encode_snapshot(route(), heartbeat().vote, &full_snapshot(&f)).unwrap()
            } else {
                peer_wire::encode_vote(
                    route(),
                    if method == 0 {
                        peer_wire::Kind::VoteRequest
                    } else {
                        peer_wire::Kind::PreVoteRequest
                    },
                    &ballot_request(),
                )
                .unwrap()
            };
            let config = f.client_config(&f.client, 2);
            let work = tokio::spawn(async move {
                let mut stream = connect(config, address).await?;
                send_frame(&mut stream, &bytes, false).await?;
                read_frame(&mut stream).await
            });
            let observed = observe(&mut events, Event::Stage(Stage::BeforeDispatch)).await;
            let mut guard = None;
            let fenced = match failure {
                0 => f.remove(1),
                1 => {
                    guard = f.sm.0.lock().ok();
                    guard.is_some()
                }
                _ => {
                    let sm = f.sm.clone();
                    std::thread::spawn(move || {
                        let _guard = sm.0.lock().unwrap();
                        panic!("synthetic registry poison")
                    })
                    .join()
                    .is_err()
                }
            };
            release(&receiver, Stage::BeforeDispatch, &hold);
            // Contention is held through the actual refusal, then released before close.
            let result = work.await;
            drop(guard);
            let closed = receiver.close().await;
            assert!(observed && fenced && closed.is_ok() && reclaimed(&receiver));
            assert!(matches!(result, Ok(Err(_))));
            assert!(spy.calls.lock().unwrap().is_empty());
        }
    }
}

#[test]
fn rpc_evidence_keeps_first_failure_and_retained_ownership() {
    let mut e = LearnerEvidence {
        body: Some(Err("body first")),
        ..Default::default()
    };
    e.fail("body first");
    e.transport = Some(("Unclean".into(), "Ok".into()));
    e.fail("cleanup later");
    e.readback = Some(Err("readback last"));
    e.fail("readback last");
    let bytes = serde_json::to_vec(&e).unwrap();
    let decoded: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(decoded["first"], "body first");
    assert_eq!(decoded["body"]["Err"], "body first");
    assert_eq!(decoded["readback"]["Err"], "readback last");
    let owner = Arc::new(());
    let weak = Arc::downgrade(&owner);
    LEARNER_RETAINED.lock().unwrap().push(Box::new(owner));
    assert_eq!(weak.strong_count(), 1);
    let retained = LEARNER_RETAINED.lock().unwrap().pop().unwrap();
    assert_eq!(weak.strong_count(), 1);
    drop(retained);
    assert_eq!(weak.strong_count(), 0);
}
#[tokio::test]
async fn rpc_setup_error_and_panic_preserve_cleanup_evidence() {
    use futures_util::FutureExt;
    for panic in [false, true] {
        let dir = private_temp();
        let output = learner_evidence_root(None).unwrap();
        let mut evidence = LearnerEvidence {
            output: Some(output.clone()),
            ..Default::default()
        };
        let init = Init {
            cluster: CLUSTER,
            first_free: node(3),
        };
        let mut releases = Vec::new();
        let mut reservations = 0;
        let mut address = None;
        let attempt: SetupAttempt<()> = std::panic::AssertUnwindSafe(async {
            let store = StateStore::open(dir.path(), init).unwrap();
            releases.push(store.released());
            let socket = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let bound = socket.local_addr().unwrap();
            address = Some(bound);
            reservations += 1;
            if panic {
                panic!("controlled setup panic, no Engine");
            }
            // A genuine second reservation failure after a store and socket exist.
            TcpListener::bind(bound)
                .await
                .map_err(|_| "receiver reservation")?;
            Err("unexpected second bind")
        })
        .catch_unwind()
        .await;
        let result = finish_learner_setup(0, &mut evidence, attempt, reservations, &releases);
        let bytes = std::fs::read(output.join("learner-0-evidence.json")).unwrap();
        let recorded: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let expected = if panic {
            "setup panic; unregistered cleanup unconfirmed"
        } else {
            "receiver reservation"
        };
        // Independent safe cleanup observations; never start or retry an Engine.
        let rebound = TcpListener::bind(address.unwrap()).await;
        let reopened = StateStore::open(dir.path(), init).unwrap();
        let released = reopened.released();
        drop(reopened);
        let cleanup = rebound.is_ok() && released.is_set() && releases.iter().all(|r| r.is_set());
        drop(rebound);
        assert!(cleanup);
        assert_eq!(result, Err(expected));
        assert_eq!(recorded["first"], expected);
        assert_eq!(recorded["setup"]["outcome"]["Err"], expected);
        assert_eq!(recorded["setup"]["dropped_reservations"], 1);
        assert_eq!(
            recorded["setup"]["registered_store_releases"],
            serde_json::json!([true])
        );
        assert!(
            recorded["setup"]["reopen"]
                .as_str()
                .unwrap()
                .contains("unconfirmed")
        );
    }
}
#[test]
fn rpc_evidence_portable_roots_and_write_failure_preserve_primary() {
    let fallback = learner_evidence_root(None).unwrap();
    assert!(fallback.is_absolute() && fallback.is_dir());
    assert!(learner_evidence_root(Some("relative-root".into())).is_err());
    assert_eq!(
        learner_evidence_root(Some(fallback.clone().into_os_string())).unwrap(),
        fallback
    );
    let mut evidence = LearnerEvidence {
        output: Some(fallback.join("missing")),
        ..Default::default()
    };
    let attempt: SetupAttempt<()> = Ok(Err("setup original"));
    assert_eq!(
        finish_learner_setup(1, &mut evidence, attempt, 0, &[]),
        Err("setup original")
    );
    let bytes = LEARNER_EVIDENCE.lock().unwrap().get(&1).unwrap().clone();
    let recorded: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(recorded["first"], "setup original");
    assert!(!recorded["artifact_error"].is_null());
    assert_eq!(recorded["setup"]["outcome"]["Err"], "setup original");
    // If setup succeeds but evidence cannot be written, its owners are dropped
    // and startup is refused, with no discarded first failure.
    let owner = Arc::new(());
    let weak = Arc::downgrade(&owner);
    let attempt = Ok(Ok(owner));
    assert!(
        finish_learner_setup(
            2,
            &mut LearnerEvidence {
                output: Some(fallback.join("missing")),
                ..Default::default()
            },
            attempt,
            0,
            &[]
        )
        .is_err()
    );
    assert_eq!(weak.strong_count(), 0);
}
async fn recording_call(mut c: AppendClient, method: u8, s: Snapshot) -> bool {
    use openraft::network::{NetSnapshot, NetVote};
    let o = RPCOption::new(Duration::from_secs(4));
    match method {
        0 => c.vote(ballot_request(), o).await.is_ok(),
        1 => c.pre_vote(ballot_request(), o).await.is_ok(),
        2 => c
            .full_snapshot(heartbeat().vote, s, std::future::pending(), o)
            .await
            .is_ok(),
        _ => c.append_entries(heartbeat(), o).await.is_ok(),
    }
}
#[tokio::test]
async fn rpc_new_methods_reauthorize_target_after_reply() {
    for method in 0..3 {
        let f = Fixture::new();
        let spy = rpc_spy(false);
        let mut receiver = f.owner(2, spy.clone());
        let address = listening(&mut receiver).await;
        let mut sender = f.owner(1, Spy::exact(None));
        let hold = gate(&sender, Stage::ReplyRead);
        let mut rx = events(&sender);
        let c = sender.loopback_client(2, address);
        let s = full_snapshot(&f);
        let call = tokio::spawn(recording_call(c, method, s));
        let reached = observe(&mut rx, Event::Stage(Stage::ReplyRead)).await;
        let removed = f.remove(2);
        release(&sender, Stage::ReplyRead, &hold);
        let result = call.await;
        let a = sender.close().await;
        let b = receiver.close().await;
        assert!(reached && removed && matches!(result, Ok(false)));
        assert_eq!(spy.calls.lock().unwrap().len(), 1);
        assert!(a.is_ok() && b.is_ok() && reclaimed(&sender) && reclaimed(&receiver));
    }
}
#[tokio::test]
async fn rpc_snapshot_preserves_higher_vote_result() {
    use openraft::network::NetSnapshot;
    let f = Fixture::new();
    let higher = vote(StoredVote {
        term: 9,
        node: 2,
        committed: true,
    });
    let spy = Arc::new(RpcSpy {
        calls: Mutex::new(Vec::new()),
        deny: false,
        snapshot_vote: Some(higher),
        snapshot_gate: None,
        snapshot_entered: Mutex::new(None),
    });
    let mut receiver = f.owner(2, spy.clone());
    let address = listening(&mut receiver).await;
    let mut sender = f.owner(1, Spy::exact(None));
    let result = sender
        .loopback_client(2, address)
        .full_snapshot(
            heartbeat().vote,
            full_snapshot(&f),
            std::future::pending(),
            RPCOption::new(Duration::from_secs(4)),
        )
        .await;
    let a = sender.close().await;
    let b = receiver.close().await;
    assert_eq!(result.unwrap().vote, higher);
    assert_eq!(spy.calls.lock().unwrap().len(), 1);
    assert!(a.is_ok() && b.is_ok() && reclaimed(&sender) && reclaimed(&receiver));
}
#[tokio::test]
async fn rpc_new_method_caller_loss_and_close_races() {
    for method in 0..3 {
        for close_first in [false, true] {
            let f = Fixture::new();
            let spy = rpc_spy(false);
            let mut receiver = f.owner(2, spy.clone());
            let address = listening(&mut receiver).await;
            let mut sender = f.owner(1, Spy::exact(None));
            let hold = gate(&sender, Stage::ClientAuthenticated);
            let mut rx = events(&sender);
            let c = sender.loopback_client(2, address);
            let call = tokio::spawn(recording_call(c, method, full_snapshot(&f)));
            let reached = observe(&mut rx, Event::Stage(Stage::ClientAuthenticated)).await;
            let charged = sender.shared.connections.available_permits() == 31;
            let mut close_pending = true;
            if close_first {
                // Poll close once to fence admission; cancellation retains original owner.
                use futures_util::FutureExt;
                close_pending = sender.close().now_or_never().is_none();
            } else {
                call.abort();
            }
            release(&sender, Stage::ClientAuthenticated, &hold);
            let result = call.await;
            let a = sender.close().await;
            let b = receiver.close().await;
            assert!(
                reached
                    && charged
                    && close_pending
                    && (result.is_err() || matches!(result, Ok(false)))
            );
            assert!(spy.calls.lock().unwrap().is_empty());
            assert!(a.is_ok() && b.is_ok() && reclaimed(&sender) && reclaimed(&receiver));
        }
    }
}
#[tokio::test]
async fn rpc_mixed_methods_share_established_and_handshake_limits() {
    for handshake in [false, true] {
        let f = Fixture::new();
        let spy = rpc_spy(false);
        let mut receiver = f.owner(2, spy.clone());
        let address = listening(&mut receiver).await;
        let mut sender = f.owner(1, Spy::exact(None));
        let phase = if handshake {
            Stage::ClientHandshake
        } else {
            Stage::ClientAuthenticated
        };
        let hold = gate(&sender, phase);
        let mut rx = events(&sender);
        let n = if handshake { 4 } else { 32 };
        let mut calls = Vec::new();
        let mut reached = true;
        for i in 0..n {
            calls.push(tokio::spawn(recording_call(
                sender.loopback_client(2, address),
                (i % 4) as u8,
                full_snapshot(&f),
            )));
            reached &= observe(&mut rx, Event::Stage(phase)).await;
        }
        let charged = sender.shared.connections.available_permits() == 32 - n
            && sender.shared.handshakes.available_permits() == if handshake { 0 } else { 4 };
        let mut refused = true;
        for method in 0..4 {
            refused &= !recording_call(
                sender.loopback_client(2, address),
                method,
                full_snapshot(&f),
            )
            .await;
        }
        hold.add_permits(n);
        let mut completed = true;
        for c in calls {
            completed &= c.await.is_ok_and(|v| v);
        }
        let a = sender.close().await;
        let b = receiver.close().await;
        assert!(reached && charged && refused && completed);
        assert_eq!(spy.calls.lock().unwrap().len(), n);
        assert!(a.is_ok() && b.is_ok() && reclaimed(&sender) && reclaimed(&receiver));
    }
}
#[tokio::test]
async fn rpc_partial_attach_and_publish_after_close_release_resources() {
    let f = Fixture::new();
    let credentials = || Credentials {
        root: f.root.clone(),
        leaf: f.server.cert.clone(),
        key: f.server.private_key(),
        time: Arc::new(pki::FixedTime(1001)),
    };
    for wrong_address in [true, false] {
        let (mut owner, factory) = TransportOwner::pending(CLUSTER, node(2), credentials());
        let baseline = Arc::strong_count(&f.sm.0);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let result = if wrong_address {
            owner.attach_reserved(
                &f.sm,
                rpc_spy(false),
                listener,
                "127.0.0.1:1".parse().unwrap(),
            )
        } else {
            let c = owner.begin_attach().unwrap();
            let resources = make_resources(CLUSTER, node(2), &f.sm, c, rpc_spy(false)).unwrap();
            let closed = owner.close().await;
            assert!(closed.is_ok());
            owner.publish(&f.sm, resources, listener)
        };
        let closed = owner.close().await;
        let rebound = TcpListener::bind(addr).await;
        assert!(result.is_err() && closed.is_ok() && reclaimed(&owner));
        assert_eq!(Arc::strong_count(&f.sm.0), baseline);
        assert!(rebound.is_ok());
        assert!(factory.shared.upgrade().is_some());
    }
}

// Independent wire construction deliberately bypasses the codec's sender checks.
fn independent_snapshot_frame(
    cluster: [u8; 16],
    position: (u64, u64, u64),
    membership_position: (u64, u64, u64),
    membership: &[u8],
    blob: &[u8],
) -> Vec<u8> {
    let mut body = Vec::new();
    for n in [7u64, 1] {
        body.extend(n.to_be_bytes());
    }
    body.push(1); // committed vote
    body.push(1);
    for n in [position.0, position.1, position.2] {
        body.extend(n.to_be_bytes());
    }
    body.push(1);
    for n in [
        membership_position.0,
        membership_position.1,
        membership_position.2,
    ] {
        body.extend(n.to_be_bytes());
    }
    body.extend((membership.len() as u16).to_be_bytes());
    body.extend(membership);
    body.extend((blob.len() as u16).to_be_bytes());
    body.extend(blob);
    let mut frame = b"ARXP".to_vec();
    frame.extend([2, 7, 0, 0]);
    frame.extend((body.len() as u32).to_be_bytes());
    frame.extend(cluster);
    frame.extend(1u64.to_be_bytes());
    frame.extend(2u64.to_be_bytes());
    frame.extend(body);
    frame
}
#[tokio::test]
async fn rpc_snapshot_nondefault_membership_independent_binding_matrix() {
    use super::super::{MembershipBlob, WireMembership};
    let f = Fixture::new();
    let membership = |voter, address: &str| {
        WireMembership::new(
            vec![std::collections::BTreeSet::from([voter])],
            BTreeMap::from([(1, "127.0.0.1:43181".into()), (2, address.into())]),
        )
        .unwrap()
        .encode()
        .unwrap()
    };
    let m = membership(1, "127.0.0.1:43182");
    f.sm.0
        .lock()
        .unwrap()
        .apply(
            LogPos {
                term: 1,
                leader: 1,
                index: 1,
            },
            &StoredEntry::Membership(MembershipBlob::new(m.clone()).unwrap()),
        )
        .unwrap();
    let blob = f.sm.0.lock().unwrap().snapshot();
    let valid = independent_snapshot_frame(CLUSTER, (1, 1, 1), (1, 1, 1), &m, &blob);
    // Independent positive bytes must agree with canonical decoder before negatives.
    let (_, v, snap) = peer_wire::decode_snapshot(&valid).unwrap();
    assert_ne!(
        snap.meta.last_membership,
        openraft::StoredMembership::default()
    );
    assert_eq!(
        peer_wire::encode_snapshot(route(), v, &snap).unwrap(),
        valid
    );
    let config = membership(2, "127.0.0.1:43182");
    let address = membership(1, "127.0.0.1:43184");
    let mut corrupt = blob.clone();
    corrupt[10] ^= 1;
    let mut digest = blob.clone();
    let last = digest.len() - 1;
    digest[last] ^= 1;
    let foreign_dir = private_temp();
    let mut foreign = StateStore::open(
        foreign_dir.path(),
        Init {
            cluster: [2; 16],
            first_free: node(3),
        },
    )
    .unwrap();
    foreign
        .apply(
            LogPos {
                term: 1,
                leader: 1,
                index: 0,
            },
            &StoredEntry::Enrollment(Command::Found {
                cluster: [2; 16],
                members: FoundMembers::new([
                    (node(1), f.client.digest()),
                    (node(2), f.server.digest()),
                ])
                .unwrap(),
                proposed_at_ms: 100,
            }),
        )
        .unwrap();
    foreign
        .apply(
            LogPos {
                term: 1,
                leader: 1,
                index: 1,
            },
            &StoredEntry::Membership(MembershipBlob::new(m.clone()).unwrap()),
        )
        .unwrap();
    let foreign_blob = foreign.snapshot();
    assert!(StateStore::inspect_snapshot(&foreign_blob, [2; 16]).is_ok());
    drop(foreign);
    let mut oversized_header = valid[..44].to_vec();
    oversized_header[8..12].copy_from_slice(&20552u32.to_be_bytes());
    let cases = [
        oversized_header,
        independent_snapshot_frame(CLUSTER, (1, 1, 1), (1, 1, 1), &m, &foreign_blob),
        independent_snapshot_frame(CLUSTER, (1, 1, 2), (1, 1, 1), &m, &blob),
        independent_snapshot_frame(CLUSTER, (1, 1, 1), (1, 1, 0), &m, &blob),
        independent_snapshot_frame(CLUSTER, (1, 1, 1), (1, 1, 1), &config, &blob),
        independent_snapshot_frame(CLUSTER, (1, 1, 1), (1, 1, 1), &address, &blob),
        independent_snapshot_frame([2; 16], (1, 1, 1), (1, 1, 1), &m, &blob),
        independent_snapshot_frame(CLUSTER, (1, 1, 1), (1, 1, 1), &m, &corrupt),
        independent_snapshot_frame(CLUSTER, (1, 1, 1), (1, 1, 1), &m, &digest),
        independent_snapshot_frame(CLUSTER, (1, 1, 1), (1, 1, 1), &m, &vec![0; 16385]),
    ];
    for frame in cases {
        let spy = rpc_spy(false);
        let mut owner = f.owner(2, spy.clone());
        let result = raw_request(&f, &mut owner, &frame, false).await;
        let closed = owner.close().await;
        assert!(result.is_err() && spy.calls.lock().unwrap().is_empty());
        assert!(closed.is_ok() && reclaimed(&owner));
    }
    let mut too_large = snap;
    too_large.snapshot = vec![0; 16385];
    assert!(peer_wire::encode_snapshot(route(), v, &too_large).is_err());
    // Absolute decoder envelope: even all maximum fields cannot reach 16 KiB.
    let envelope = 8 + 2 + 25 + 1 + 24 + 2 + 4096 + 34 + 2 + 16 * 40 + 32;
    assert_eq!(envelope, 4866);
    assert_eq!(envelope + 55 * 209, 16361);
    assert!(envelope + 56 * 209 > 16384);
}

#[tokio::test]
async fn rpc_supervisor_survives_observer_cancellation() {
    let (release, released) = oneshot::channel();
    let owned = Arc::new(());
    let weak = Arc::downgrade(&owned);
    let worker: LearnerWorker = Arc::new(tokio::sync::Mutex::new(tokio::spawn(async move {
        let _owned = owned;
        released.await.map_err(|_| "release lost")?;
        Ok(())
    })));
    let observer = tokio::spawn(observe_learner_worker(worker.clone()));
    tokio::task::yield_now().await;
    observer.abort();
    let cancelled = observer.await.is_err();
    let retained = weak.strong_count() == 1;
    release.send(()).unwrap();
    let result = observe_learner_worker(worker).await;
    assert!(cancelled && retained && result.is_ok());
    assert_eq!(weak.strong_count(), 0);
}

#[tokio::test]
async fn rpc_partial_attach_real_bind_failure_releases_temporary_resources() {
    let f = Fixture::new();
    let credentials = Credentials {
        root: f.root.clone(),
        leaf: f.server.cert.clone(),
        key: f.server.private_key(),
        time: Arc::new(pki::FixedTime(1001)),
    };
    let (mut owner, factory) = TransportOwner::pending(CLUSTER, node(2), credentials);
    let before = Arc::strong_count(&f.sm.0);
    // RFC5737 documentation address is not a local interface in this fixture.
    // This passes endpoint syntax, builds real TLS resources, then reaches OS bind.
    let failed = owner.attach(&f.sm, rpc_spy(false), "192.0.2.1:43182").await;
    let temporary_released = Arc::strong_count(&f.sm.0) == before;
    let no_publication = owner.shared.state.lock().unwrap().resources.is_none();
    let closed = owner.close().await;
    assert_eq!(failed, Err(Refusal::Address));
    assert!(temporary_released && no_publication && closed.is_ok() && reclaimed(&owner));
    assert_eq!(factory.shared.strong_count(), 1);
}

fn learner_image(
    f: &Fixture,
    original: u64,
    receipt: bool,
    sm: &SmAdapter,
) -> Arc<peer_registry::LearnerGate> {
    use crate::cluster::join_receipt::{ExpectedIdentity, Origin};
    peer_registry::LearnerGate::new(
        sm,
        Arc::new(ExpectedIdentity {
            cluster: CLUSTER,
            node: node(2),
            key: f.server.digest(),
            ca: crate::cluster::pki::ca::fingerprint_of(&f.root),
            origin: Origin::Redeem {
                term: 1,
                leader: 1,
                index: original,
                anchors: vec![(1, *f.client.digest().as_bytes())],
            },
        }),
        "192.168.0.2:7001".parse().unwrap(),
        Arc::new(super::super::checked_clock::HostClock::new(0)),
        receipt,
    )
}

async fn listen_learner_for_test(
    f: &Fixture,
    sm: &SmAdapter,
    learner: Arc<peer_registry::LearnerGate>,
    spy: Arc<RpcSpy>,
) -> (TransportOwner, SocketAddr) {
    let creds = Credentials {
        root: f.root.clone(),
        leaf: f.server.cert.clone(),
        key: f.server.private_key(),
        time: Arc::new(pki::FixedTime(1001)),
    };
    let resources =
        make_resources_for_learner(CLUSTER, node(2), sm, creds, spy, Some(learner)).unwrap();
    let mut owner = TransportOwner::pending_founder(CLUSTER, node(2)).0;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    owner.publish(sm, resources, listener).unwrap();
    (owner, address)
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn gated_call_observer_ignores_unrelated_handshake_during_dispatch_gate() {
    let f = Fixture::new();
    add_learner_membership(&f.sm);
    let learner = learner_image(&f, 1, true, &f.sm);
    let spy = rpc_spy(false);
    let (mut owner, address) = listen_learner_for_test(&f, &f.sm, learner, spy.clone()).await;
    let hold = gate(&owner, Stage::BeforeDispatch);
    let mut shared_events = events(&owner);
    let trace = Arc::new(GatedCallTrace::default());
    trace.entry.set(Instant::now()).unwrap();
    owner.observe_gated_inbound(&trace);

    let exchange = timeout(OBSERVE, async {
        let mut stream = connect(f.client_config(&f.client, 2), address)
            .await
            .unwrap_or_else(|_| panic!("selected TLS connection failed"));
        let source = stream
            .get_ref()
            .0
            .local_addr()
            .unwrap_or_else(|_| panic!("selected socket source was unavailable"));
        let frame = peer_wire::encode_request(route(), &heartbeat()).unwrap();
        assert!(
            send_frame(&mut stream, &frame, false).await.is_ok(),
            "selected frame write failed"
        );
        assert!(observe(&mut shared_events, Event::Stage(Stage::BeforeDispatch)).await);

        // The accept task can publish stages before connect reports the source.
        trace.set_source(source);
        let mut cursor = 0;
        for expected in [
            Stage::ServerHandshake,
            Stage::ServerAuthenticated,
            Stage::RequestRead,
            Stage::BeforeDispatch,
        ] {
            let (peer, _, event) = trace.next_server_event(&mut cursor).await.unwrap();
            assert!(
                peer == source,
                "observed stage came from a different connection"
            );
            assert_eq!(event, Event::Stage(expected));
        }

        // The second socket stays in TLS while the selected call is gated.
        let unrelated = TcpStream::connect(address)
            .await
            .unwrap_or_else(|_| panic!("unrelated connection failed"));
        let unrelated_source = unrelated
            .local_addr()
            .unwrap_or_else(|_| panic!("unrelated socket source was unavailable"));
        assert!(
            unrelated_source != source,
            "control needs two distinct connections"
        );
        assert_eq!(
            shared_events.recv().await,
            Some(Event::Stage(Stage::ServerHandshake))
        );
        assert!(spy.calls.lock().unwrap().is_empty());
        release(&owner, Stage::BeforeDispatch, &hold);

        let (peer, _, event) = trace.next_server_event(&mut cursor).await.unwrap();
        assert!(
            peer == source,
            "observed dispatch came from a different connection"
        );
        assert!(matches!(event, Event::LearnerDispatch(_)));
        for expected in [Event::Stage(Stage::BeforeReply), Event::Finished(None)] {
            let (peer, _, event) = trace.next_server_event(&mut cursor).await.unwrap();
            assert!(
                peer == source,
                "observed reply came from a different connection"
            );
            assert_eq!(event, expected);
        }
        assert!(
            trace
                .server_events
                .lock()
                .unwrap()
                .unrelated
                .iter()
                .any(|(_, event)| *event == Event::Stage(Stage::ServerHandshake))
        );
        assert!(
            read_frame(&mut stream).await.is_ok(),
            "selected reply read failed"
        );
        assert!(
            clean_eof(&mut stream).await.is_ok(),
            "selected socket did not finish cleanly"
        );
        drop(unrelated);
        frame
    })
    .await;
    release(&owner, Stage::BeforeDispatch, &hold);
    assert!(bounded_close(&mut owner).await.is_ok());
    let frame = exchange.expect("gated exchange observation elapsed");
    assert!(
        spy.calls.lock().unwrap().as_slice() == [frame],
        "selected frame must be the only dispatch"
    );
}

async fn exchange_learner_for_test(
    address: SocketAddr,
    config: Arc<ClientConfig>,
    frame: &[u8],
) -> io::Result<Vec<u8>> {
    timeout(OBSERVE, async {
        let mut stream = connect(config, address).await?;
        send_frame(&mut stream, frame, false).await?;
        let bytes = read_frame(&mut stream).await?;
        clean_eof(&mut stream).await?;
        Ok(bytes)
    })
    .await
    .map_err(|_| io::Error::other("learner exchange elapsed"))?
}

fn add_learner_membership(sm: &SmAdapter) {
    let membership = Members::new(
        vec![[1].into_iter().collect()],
        BTreeMap::from([(1, openraft::BasicNode::new("192.168.0.1:7001"))]),
    )
    .unwrap();
    sm.0.lock()
        .unwrap()
        .apply(
            LogPos {
                term: 1,
                leader: 1,
                index: 1,
            },
            &StoredEntry::Membership(super::super::encode(&membership).unwrap()),
        )
        .unwrap();
}

// Poll the actual socket future with the store held. Paused Tokio time
// places release on the chosen side of the acquisition deadline.
async fn poll_while_store_is_held<F: Future>(
    sm: &SmAdapter,
    mut exchange: Pin<&mut F>,
    release_stage: impl FnOnce(),
    expire: bool,
) -> Option<F::Output> {
    use std::task::{Context, Waker};
    let sm = sm.clone();
    let (locked, ready) = oneshot::channel();
    let (release, wait) = std::sync::mpsc::channel::<()>();
    let holder = tokio::task::spawn_blocking(move || {
        let _store = sm.0.lock().unwrap();
        locked.send(()).unwrap();
        let _ = wait.recv();
    });
    ready.await.unwrap();

    tokio::time::pause();
    release_stage();
    assert!(
        exchange
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending(),
        "contended authorization must yield before returning"
    );
    let result = if expire {
        tokio::time::advance(Duration::from_millis(20)).await;
        match exchange.poll(&mut Context::from_waker(Waker::noop())) {
            Poll::Ready(result) => Some(result),
            Poll::Pending => panic!("authorization did not refuse at its deadline"),
        }
    } else {
        None
    };
    drop(release);
    holder.await.unwrap();
    result
}

async fn strict_client_waits_for_applied_store(expire: bool) {
    let f = Fixture::new();
    let dir = private_temp();
    let mut local_store = StateStore::open(
        dir.path(),
        Init {
            cluster: CLUSTER,
            first_free: node(3),
        },
    )
    .unwrap();
    local_store
        .install(&f.sm.0.lock().unwrap().snapshot())
        .unwrap();
    let sm = SmAdapter(Arc::new(Mutex::new(local_store)));
    let mut sender = TransportOwner::new(
        CLUSTER,
        node(1),
        &sm,
        Credentials {
            root: f.root.clone(),
            leaf: f.client.cert.clone(),
            key: f.client.private_key(),
            time: Arc::new(pki::FixedTime(1001)),
        },
        rpc_spy(false),
    )
    .unwrap();
    let spy = rpc_spy(false);
    let mut receiver = f.owner(2, spy.clone());
    let mut receiver_events = events(&receiver);
    let address = listening(&mut receiver).await;
    let mut sender_events = events(&sender);
    let boundary = Stage::BeforeClientAuthorization;
    let hold = gate(&sender, boundary);
    {
        let resources = sender
            .shared
            .state
            .lock()
            .unwrap()
            .resources
            .clone()
            .unwrap();
        let handshake = sender
            .shared
            .handshakes
            .clone()
            .try_acquire_owned()
            .unwrap();
        let (caller, _reply) = oneshot::channel();
        let mut exchange = std::pin::pin!(outbound(
            &sender.shared,
            resources,
            address,
            node(2),
            Request::Append(heartbeat()),
            handshake,
            Instant::now() + OBSERVE,
            &caller,
            #[cfg(target_os = "linux")]
            None,
        ));
        tokio::select! {
            _ = &mut exchange => panic!("outbound finished before its authorization boundary"),
            reached = observe(&mut sender_events, Event::Stage(boundary)) => assert!(reached),
        }
        let expired = poll_while_store_is_held(
            &sm,
            exchange.as_mut(),
            || release(&sender, boundary, &hold),
            expire,
        )
        .await;
        let result = match expired {
            Some(result) => result,
            None => exchange.await,
        };
        if expire {
            assert!(matches!(result, Err(Refusal::Tls)));
        } else {
            assert!(matches!(result, Ok(Reply::Append(Ok(_)))));
        }
    }
    let mut request_read = false;
    let finished = timeout(OBSERVE, async {
        while let Some(event) = receiver_events.recv().await {
            request_read |= event == Event::Stage(Stage::RequestRead);
            if let Event::Finished(reason) = event {
                return reason;
            }
        }
        panic!("receiver observation closed")
    })
    .await
    .unwrap();
    assert_eq!(request_read, !expire);
    assert_eq!(finished, expire.then_some(Refusal::Frame));
    assert_eq!(spy.calls.lock().unwrap().len(), usize::from(!expire));
    assert!(bounded_close(&mut sender).await.is_ok());
    assert!(bounded_close(&mut receiver).await.is_ok());
}

#[tokio::test]
async fn strict_client_authorizes_after_store_release_within_budget() {
    strict_client_waits_for_applied_store(false).await;
}

#[tokio::test]
async fn strict_client_refuses_before_writing_when_store_outlasts_budget() {
    strict_client_waits_for_applied_store(true).await;
}

async fn dispatch_waits_for_applied_store(learner: bool, expire: bool) {
    let f = Fixture::new();
    let learner = learner.then(|| {
        add_learner_membership(&f.sm);
        learner_image(&f, 1, true, &f.sm)
    });
    let spy = rpc_spy(false);
    let credentials = Credentials {
        root: f.root.clone(),
        leaf: f.server.cert.clone(),
        key: f.server.private_key(),
        time: Arc::new(pki::FixedTime(1001)),
    };
    let resources =
        make_resources_for_learner(CLUSTER, node(2), &f.sm, credentials, spy.clone(), learner)
            .unwrap();
    let (mut owner, _) = TransportOwner::pending_founder(CLUSTER, node(2));
    let hold = gate(&owner, Stage::BeforeDispatch);
    let reply = gate(&owner, Stage::BeforeReply);
    let mut events = events(&owner);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let handshake = owner.shared.handshakes.clone().try_acquire_owned().unwrap();
    let frame = peer_wire::encode_request(route(), &heartbeat()).unwrap();

    {
        let mut server = std::pin::pin!(async {
            let (socket, _) = listener.accept().await.unwrap();
            inbound(
                &owner.shared,
                resources,
                socket,
                handshake,
                Instant::now() + INBOUND_BUDGET,
                #[cfg(target_os = "linux")]
                None,
            )
            .await
        });
        let mut client = std::pin::pin!(exchange_learner_for_test(
            address,
            f.client_config(&f.client, 2),
            &frame,
        ));
        tokio::select! {
            result = &mut server => panic!("server finished before dispatch gate: {result:?}"),
            result = &mut client => panic!("client finished before dispatch gate: {result:?}"),
            reached = observe(&mut events, Event::Stage(Stage::BeforeDispatch)) => assert!(reached),
        }

        let expired = poll_while_store_is_held(
            &f.sm,
            server.as_mut(),
            || release(&owner, Stage::BeforeDispatch, &hold),
            expire,
        )
        .await;
        if let Some(refusal) = expired {
            tokio::time::resume();
            assert_eq!(refusal, Err(Refusal::Identity));
            assert!(client.await.is_err());
        } else {
            // Finish the 4 ms authorization check on the paused clock.
            // Resume before the reply needs real socket readiness.
            tokio::select! {
                result = &mut server => panic!("server finished before reply gate: {result:?}"),
                reached = observe(&mut events, Event::Stage(Stage::BeforeReply)) => assert!(reached),
            }
            tokio::time::resume();
            release(&owner, Stage::BeforeReply, &reply);
            let (server_result, client_result) = tokio::join!(server, client);
            assert_eq!(server_result, Ok(()));
            client_result.expect("client must read a complete reply and clean EOF");
        }
    }
    if expire {
        assert!(spy.calls.lock().unwrap().is_empty());
    } else {
        assert_eq!(spy.calls.lock().unwrap().as_slice(), &[frame]);
    }
    assert!(bounded_close(&mut owner).await.is_ok());
}

#[tokio::test]
async fn learner_dispatch_yields_to_runtime_while_applied_store_is_busy() {
    dispatch_waits_for_applied_store(true, false).await;
}

#[tokio::test]
async fn strict_dispatch_authorizes_after_store_release_within_budget() {
    dispatch_waits_for_applied_store(false, false).await;
}

#[tokio::test]
async fn strict_dispatch_refuses_when_store_outlasts_budget() {
    dispatch_waits_for_applied_store(false, true).await;
}

#[tokio::test]
async fn learner_refuses_old_matching_image_after_actual_sql_persistence_failure() {
    let f = Fixture::new();
    add_learner_membership(&f.sm);
    let scratch = private_temp();
    let init = Init {
        cluster: CLUSTER,
        first_free: node(3),
    };
    {
        let mut store = StateStore::open(scratch.path(), init).unwrap();
        store.install(&f.sm.0.lock().unwrap().snapshot()).unwrap();
    }
    {
        let db = rusqlite::Connection::open(scratch.path().join("cluster-state.sqlite3")).unwrap();
        db.execute_batch("CREATE TRIGGER fail_learner_write BEFORE UPDATE ON sm BEGIN SELECT RAISE(FAIL,'fixture'); END;").unwrap();
    }
    let sm = SmAdapter(Arc::new(Mutex::new(
        StateStore::open(scratch.path(), init).unwrap(),
    )));
    let learner = learner_image(&f, 1, true, &sm);
    assert_eq!(learner.phase(), Ok(peer_registry::Phase::Strict));
    let spy = rpc_spy(false);
    let (mut owner, address) = listen_learner_for_test(&f, &sm, learner.clone(), spy.clone()).await;
    let hold = gate(&owner, Stage::BeforeDispatch);
    let mut rx = events(&owner);
    let frame = peer_wire::encode_request(route(), &heartbeat()).unwrap();
    let mut exchange = Box::pin(exchange_learner_for_test(
        address,
        f.client_config(&f.client, 2),
        &frame,
    ));
    tokio::select! {
        result = &mut exchange => panic!("learner exchange finished before gate: {result:?}"),
        reached = observe(&mut rx, Event::Stage(Stage::BeforeDispatch)) => assert!(reached),
    }
    {
        let mut store = sm.0.try_lock().unwrap();
        let old = store.snapshot();
        assert_eq!(
            store.apply(
                LogPos {
                    term: 1,
                    leader: 1,
                    index: 2
                },
                &StoredEntry::Blank
            ),
            Err(super::super::StoreError::Sql)
        );
        assert!(store.is_poisoned());
        assert_eq!(old, store.snapshot());
        assert_eq!(
            store
                .state()
                .authorize_peer(CLUSTER, node(1), f.client.digest()),
            Ok(())
        );
        assert_eq!(
            store
                .state()
                .authorize_peer(CLUSTER, node(2), f.server.digest()),
            Ok(())
        );
    }
    release(&owner, Stage::BeforeDispatch, &hold);
    assert!(exchange.await.is_err());
    assert!(observe(&mut rx, Event::Finished(Some(Refusal::Identity))).await);
    assert!(
        exchange_learner_for_test(address, f.client_config(&f.client, 2), &frame)
            .await
            .is_err()
    );
    // The cryptographic handshake starts, but a poisoned store refuses
    // before the application-authenticated stage and before RequestRead.
    assert_eq!(rx.try_recv(), Ok(Event::Stage(Stage::ServerHandshake)));
    assert_eq!(
        timeout(OBSERVE, rx.recv()).await.unwrap(),
        Some(Event::Finished(Some(Refusal::Tls)))
    );
    assert!(spy.calls.lock().unwrap().is_empty());
    assert!(bounded_close(&mut owner).await.is_ok());
}

#[tokio::test]
async fn learner_tls_receipt_is_exact_and_scoped_to_append_and_snapshot() {
    let f = Fixture::new();
    let dir = private_temp();
    let sm = SmAdapter(Arc::new(Mutex::new(
        StateStore::open(
            dir.path(),
            Init {
                cluster: CLUSTER,
                first_free: node(2),
            },
        )
        .unwrap(),
    )));
    let learner = learner_image(&f, 1, true, &sm);
    let spy = rpc_spy(false);
    let (mut owner, address) = listen_learner_for_test(&f, &sm, learner.clone(), spy.clone()).await;
    let mut rx = events(&owner);
    let config = f.client_config(&f.client, 2);
    let append = peer_wire::encode_request(route(), &heartbeat()).unwrap();
    let snapshot =
        peer_wire::encode_snapshot(route(), heartbeat().vote, &full_snapshot(&f)).unwrap();
    for frame in [&append, &snapshot] {
        assert!(
            exchange_learner_for_test(address, config.clone(), frame)
                .await
                .is_ok()
        );
        assert!(
            observe(
                &mut rx,
                Event::LearnerDispatch(peer_registry::Lane::Receipt)
            )
            .await
        );
    }
    for kind in [
        peer_wire::Kind::VoteRequest,
        peer_wire::Kind::PreVoteRequest,
    ] {
        let frame = peer_wire::encode_vote(route(), kind, &ballot_request()).unwrap();
        assert!(
            exchange_learner_for_test(address, config.clone(), &frame)
                .await
                .is_err()
        );
        assert!(observe(&mut rx, Event::Finished(Some(Refusal::Identity))).await);
    }
    for identity in [
        Fixture::issue(&f.issuer, CLUSTER, 1, 940, 10000),
        Fixture::reissue(&f.issuer, [3; 16], 1, &f.client, 940, 10000),
        Fixture::reissue(&f.issuer, CLUSTER, 3, &f.client, 940, 10000),
    ] {
        assert!(
            exchange_learner_for_test(address, f.client_config(&identity, 2), &append)
                .await
                .is_err()
        );
        assert!(observe(&mut rx, Event::Finished(Some(Refusal::Tls))).await);
    }
    let mut conflicting = heartbeat();
    conflicting.prev_log_id = Some(log_id(1, 3, 1));
    let conflicting = peer_wire::encode_request(route(), &conflicting).unwrap();
    assert!(
        exchange_learner_for_test(address, config.clone(), &conflicting)
            .await
            .is_err()
    );
    assert!(observe(&mut rx, Event::Finished(Some(Refusal::Identity))).await);
    let mut wrong_route = route();
    wrong_route.target = 3;
    let bad_route = peer_wire::encode_request(wrong_route, &heartbeat()).unwrap();
    let mut wrong_sender = heartbeat();
    wrong_sender.vote.leader_id.node_id = 3;
    let bad_sender = peer_wire::encode_request(route(), &wrong_sender).unwrap();
    let mut trailing = append;
    trailing.push(0);
    for frame in [bad_route, bad_sender, trailing] {
        assert!(
            exchange_learner_for_test(address, config.clone(), &frame)
                .await
                .is_err()
        );
        assert!(observe(&mut rx, Event::Finished(Some(Refusal::Frame))).await);
    }
    assert_eq!(spy.calls.lock().unwrap().len(), 2);
    assert!(bounded_close(&mut owner).await.is_ok());
    drop(sm);
    assert_eq!(learner.phase(), Err(verify::VerifyRefusal::Unavailable));
}

#[tokio::test]
async fn learner_removed_anchor_valid_leaf_is_denied_after_strict_cutover() {
    let f = Fixture::new();
    add_learner_membership(&f.sm);
    let learner = learner_image(&f, 1, true, &f.sm);
    let spy = rpc_spy(false);
    let (mut owner, address) =
        listen_learner_for_test(&f, &f.sm, learner.clone(), spy.clone()).await;
    let mut rx = events(&owner);
    let frame = peer_wire::encode_request(route(), &heartbeat()).unwrap();
    let config = f.client_config(&f.client, 2);
    assert!(
        exchange_learner_for_test(address, config.clone(), &frame)
            .await
            .is_ok()
    );
    assert!(observe(&mut rx, Event::LearnerDispatch(peer_registry::Lane::Strict)).await);
    let hold = gate(&owner, Stage::BeforeDispatch);
    let mut exchange = Box::pin(exchange_learner_for_test(address, config.clone(), &frame));
    tokio::select! {
        result = &mut exchange => panic!("learner exchange finished before gate: {result:?}"),
        reached = observe(&mut rx, Event::Stage(Stage::BeforeDispatch)) => assert!(reached),
    }
    f.sm.0
        .lock()
        .unwrap()
        .apply(
            LogPos {
                term: 1,
                leader: 1,
                index: 2,
            },
            &StoredEntry::Enrollment(Command::Remove {
                cluster: CLUSTER,
                node: node(1),
                proposed_at_ms: 101,
            }),
        )
        .unwrap();
    release(&owner, Stage::BeforeDispatch, &hold);
    assert!(exchange.await.is_err());
    assert!(observe(&mut rx, Event::Finished(Some(Refusal::Identity))).await);
    assert_eq!(learner.phase(), Ok(peer_registry::Phase::Strict));
    assert!(
        exchange_learner_for_test(address, config, &frame)
            .await
            .is_err()
    );
    assert!(observe(&mut rx, Event::Finished(Some(Refusal::Tls))).await);
    assert_eq!(spy.calls.lock().unwrap().len(), 1);
    assert!(bounded_close(&mut owner).await.is_ok());
}

#[tokio::test]
async fn learner_terminal_crossing_refuses_strict_remote_after_local_binding_removed() {
    let f = Fixture::new();
    add_learner_membership(&f.sm);
    let learner = learner_image(&f, 1, true, &f.sm);
    assert_eq!(learner.phase(), Ok(peer_registry::Phase::Strict));
    let store = f.sm.0.lock().unwrap();
    assert_eq!(learner.phase(), Err(verify::VerifyRefusal::Unavailable));
    drop(store);
    assert!(matches!(
        f.sm.0.lock().unwrap().apply(
            LogPos {
                term: 1,
                leader: 1,
                index: 2
            },
            &StoredEntry::Enrollment(Command::Remove {
                cluster: CLUSTER,
                node: node(2),
                proposed_at_ms: 101
            }),
        ),
        Ok(Some(Outcome::Removed { replay: false }))
    ));
    assert_eq!(learner.phase(), Ok(peer_registry::Phase::Refused));
    assert_eq!(
        learner
            .authorize(
                CLUSTER,
                node(1),
                f.client.digest(),
                true,
                Instant::now() + INBOUND_BUDGET,
            )
            .await,
        Err(verify::VerifyRefusal::Unavailable)
    );
}

#[tokio::test]
async fn learner_ballots_refuse_bound_founder_below_e_and_dispatch_after_e() {
    use crate::cluster::{
        enrollment::{BoundVoters, JoinCode},
        membership::Membership as WireMembership,
    };

    let f = Fixture::new();
    let dir = private_temp();
    let mut store = StateStore::open(
        dir.path(),
        Init {
            cluster: CLUSTER,
            first_free: node(2),
        },
    )
    .unwrap();
    let pos = |index| LogPos {
        term: 1,
        leader: 1,
        index,
    };
    let members = Members::new(
        vec![[1].into_iter().collect()],
        BTreeMap::from([(1, openraft::BasicNode::new("192.168.0.1:7001"))]),
    )
    .unwrap();
    let membership = super::super::encode(&members).unwrap();
    let voters = WireMembership::decode(membership.bytes()).unwrap();
    store
        .apply(pos(0), &StoredEntry::Membership(membership))
        .unwrap();
    assert!(matches!(
        store.apply(
            pos(1),
            &StoredEntry::Enrollment(Command::Found {
                cluster: CLUSTER,
                members: FoundMembers::new([(node(1), f.client.digest())]).unwrap(),
                proposed_at_ms: 100,
            })
        ),
        Ok(Some(Outcome::Founded { replay: false }))
    ));
    let anchor = BoundVoters::observe(store.state(), &voters)
        .unwrap()
        .anchor(CLUSTER);
    let code = JoinCode::generate(CLUSTER, [4; 32], anchor, getrandom::fill).unwrap();
    assert!(matches!(
        store.apply(
            pos(2),
            &StoredEntry::Enrollment(Command::Mint {
                cluster: CLUSTER,
                actor: [3; 16],
                request: [4; 16],
                code: code.code_id(),
                verifier: code.verifier(),
                proposed_at_ms: 101,
            })
        ),
        Ok(Some(Outcome::Minted { replay: false, .. }))
    ));
    assert_eq!(store.applied(), Some(pos(2)));
    assert_eq!(
        store
            .state()
            .authorize_peer(CLUSTER, node(1), f.client.digest()),
        Ok(())
    );
    assert_eq!(
        store
            .state()
            .authorize_peer(CLUSTER, node(2), f.server.digest()),
        Err(crate::cluster::enrollment::PeerRefusal::NotMember)
    );

    let sm = SmAdapter(Arc::new(Mutex::new(store)));
    let learner = learner_image(&f, 3, true, &sm);
    assert_eq!(learner.phase(), Ok(peer_registry::Phase::Bootstrap));
    let spy = rpc_spy(false);
    let (mut owner, address) = listen_learner_for_test(&f, &sm, learner.clone(), spy.clone()).await;
    let mut rx = events(&owner);
    let config = f.client_config(&f.client, 2);
    let ballots = [
        peer_wire::Kind::VoteRequest,
        peer_wire::Kind::PreVoteRequest,
    ]
    .map(|kind| peer_wire::encode_vote(route(), kind, &ballot_request()).unwrap());
    for frame in &ballots {
        assert!(
            exchange_learner_for_test(address, config.clone(), frame)
                .await
                .is_err()
        );
        assert!(observe(&mut rx, Event::Stage(Stage::ServerAuthenticated)).await);
        assert!(observe(&mut rx, Event::Finished(Some(Refusal::Identity))).await);
    }
    assert!(spy.calls.lock().unwrap().is_empty());

    {
        let mut store = sm.0.lock().unwrap();
        let redeem = Command::redeem(
            CLUSTER,
            [5; 16],
            code.code_id(),
            &code.proof(),
            f.server.digest(),
            anchor,
            102,
        );
        assert!(matches!(
            store.apply(pos(3), &StoredEntry::Enrollment(redeem)),
            Ok(Some(Outcome::Enrolled(_)))
        ));
        store.apply(pos(4), &StoredEntry::Blank).unwrap();
    }
    assert_eq!(learner.phase(), Ok(peer_registry::Phase::Strict));
    for frame in &ballots {
        assert!(
            exchange_learner_for_test(address, config.clone(), frame)
                .await
                .is_ok()
        );
        assert!(observe(&mut rx, Event::LearnerDispatch(peer_registry::Lane::Strict)).await);
    }
    assert_eq!(spy.calls.lock().unwrap().as_slice(), &ballots);
    assert!(bounded_close(&mut owner).await.is_ok());
}

#[tokio::test]
async fn admission_wire_requires_exact_route_method_source_and_end() {
    let advertised = "192.168.6.2:7001".parse().unwrap();
    let source = "192.168.6.2:43210".parse().unwrap();
    let original = control::request(route(), advertised);
    assert_eq!(&original[..4], b"ARXC");
    assert_eq!(
        control::read_request(&mut &original[4..], route(), source).await,
        Ok(advertised)
    );
    for (index, replacement) in [(4, 2), (5, 2), (6, 2), (29, 3), (37, 3), (38, 1)] {
        let mut malformed = original.clone();
        malformed[index] = replacement;
        assert_eq!(
            control::read_request(&mut &malformed[4..], route(), source).await,
            Err(Refusal::Frame)
        );
    }
    let mut trailing = original.clone();
    trailing.push(0);
    assert_eq!(
        control::read_request(&mut &trailing[4..], route(), source).await,
        Err(Refusal::Frame)
    );
    assert_eq!(
        control::read_request(
            &mut &original[4..],
            route(),
            "192.168.6.3:5151".parse().unwrap()
        )
        .await,
        Err(Refusal::Address)
    );
    for invalid in [
        "192.0.2.1:7001",
        "127.0.0.1:7001",
        "[::ffff:192.168.6.2]:7001",
        "192.168.6.2:0",
    ] {
        let addr = invalid.parse().unwrap();
        let frame = control::request(route(), addr);
        assert_eq!(
            control::read_request(&mut &frame[4..], route(), addr).await,
            Err(Refusal::Address)
        );
    }
}

// The new admission cases authenticate the exact test identities without
// applying a synthetic Found entry or starting an Engine.
fn admission_verifier(f: &Fixture) -> PeerVerifier {
    let bindings = [(node(1), f.client.digest()), (node(2), f.server.digest())];
    let registry = Arc::new(move |cluster, peer, key| {
        if cluster == CLUSTER && bindings.contains(&(peer, key)) {
            Ok(())
        } else {
            Err(verify::VerifyRefusal::NotMember)
        }
    });
    PeerVerifier::new(f.root.clone(), CLUSTER, registry).unwrap()
}

async fn accept_admission_peer(
    f: &Fixture,
    listener: &TcpListener,
) -> tokio_rustls::server::TlsStream<TcpStream> {
    let config = verify::server_config(
        admission_verifier(f).server(),
        f.client.cert.clone(),
        f.client.private_key(),
        Arc::new(pki::FixedTime(1001)),
    )
    .unwrap();
    timeout(OBSERVE, async {
        let (socket, _) = listener.accept().await.unwrap();
        TlsAcceptor::from(Arc::new(config))
            .accept(socket)
            .await
            .unwrap()
    })
    .await
    .unwrap()
}

pub(in crate::cluster::consensus) async fn admission_test_peer()
-> (AuthenticatedPeer, SmAdapter, tempfile::TempDir) {
    let f = Fixture::unseeded();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let config = verify::client_config(
        admission_verifier(&f).client(node(1)),
        f.server.cert.clone(),
        f.server.private_key(),
        Arc::new(pki::FixedTime(1001)),
    )
    .unwrap();
    let (client, server) = tokio::join!(
        connect(Arc::new(config), listener.local_addr().unwrap()),
        accept_admission_peer(&f, &listener)
    );
    assert!(client.is_ok());
    let peer = AuthenticatedPeer::from_server(server.get_ref().1).unwrap();
    assert_eq!(
        peer.authenticated_identity(),
        Ok((CLUSTER, node(2), f.server.digest()))
    );
    (peer, f.sm, f._dir)
}

async fn admission_tls_exchange() -> (Fixture, TransportOwner, AdmissionClient, TcpListener) {
    let f = Fixture::unseeded();
    let gate = learner_image(&f, 1, true, &f.sm);
    let (owner, _) = listen_learner_for_test(&f, &f.sm, gate, rpc_spy(false)).await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    // Dial loopback so the tests need no host-specific LAN address. Exercise
    // request() and the real owned exchange after the endpoint constructor.
    let client = AdmissionClient {
        shared: Arc::downgrade(&owner.shared),
        issuer: listener.local_addr().unwrap(),
        advertised: "192.168.6.2:7001".parse().unwrap(),
    };
    (f, owner, client, listener)
}

async fn read_admission_request(stream: &mut (impl AsyncRead + Unpin), advertised: SocketAddr) {
    timeout(OBSERVE, async {
        let mut magic = [0; 4];
        stream.read_exact(&mut magic).await.unwrap();
        assert_eq!(magic, control::MAGIC);
        // This test issuer stands in for the advertised peer on a LAN.
        assert_eq!(
            control::read_request(stream, reverse(), advertised).await,
            Ok(advertised)
        );
    })
    .await
    .unwrap();
}

fn pause_admission_request(
    client: &AdmissionClient,
    boundary: AdmissionWriteBoundary,
) -> (
    AdmissionResponse,
    oneshot::Receiver<()>,
    oneshot::Sender<()>,
) {
    let (reached, arrival) = oneshot::channel();
    let (resume, release) = oneshot::channel();
    let pause = AdmissionWritePause {
        boundary,
        reached,
        resume: release,
    };
    let response = client.exchange(client.advertised, Some(pause)).unwrap();
    (response, arrival, resume)
}

#[tokio::test]
async fn admission_close_before_first_write_is_definite_refusal() {
    let (f, mut owner, client, listener) = admission_tls_exchange().await;
    let mut observed = events(&owner);
    let (response, reached, resume) =
        pause_admission_request(&client, AdmissionWriteBoundary::BeforeWrite);
    let mut stream = accept_admission_peer(&f, &listener).await;
    timeout(OBSERVE, reached).await.unwrap().unwrap();

    let mut close = std::pin::pin!(owner.close());
    assert!(futures_util::poll!(&mut close).is_pending());
    resume.send(()).unwrap();
    assert_eq!(response.reply().await, AdmissionReply::Refused);
    assert_eq!(observed.try_recv(), Err(mpsc::error::TryRecvError::Empty));
    let mut byte = [0; 1];
    assert_eq!(
        timeout(OBSERVE, stream.read(&mut byte))
            .await
            .unwrap()
            .unwrap_err()
            .kind(),
        io::ErrorKind::UnexpectedEof
    );
    assert_eq!(timeout(OBSERVE, close).await.unwrap(), Ok(()));
}

#[tokio::test]
async fn admission_close_after_partial_write_is_unknown() {
    let (f, mut owner, client, listener) = admission_tls_exchange().await;
    let mut observed = events(&owner);
    let (response, reached, resume) =
        pause_admission_request(&client, AdmissionWriteBoundary::AfterPrefix);
    let mut stream = accept_admission_peer(&f, &listener).await;
    timeout(OBSERVE, reached).await.unwrap().unwrap();
    let mut byte = [0; 1];
    timeout(OBSERVE, stream.read_exact(&mut byte))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(byte, control::MAGIC[..1]);

    let mut close = std::pin::pin!(owner.close());
    assert!(futures_util::poll!(&mut close).is_pending());
    resume.send(()).unwrap();
    assert_eq!(response.reply().await, AdmissionReply::Unknown);
    assert_eq!(observed.try_recv(), Err(mpsc::error::TryRecvError::Empty));
    assert_eq!(
        timeout(OBSERVE, stream.read(&mut byte))
            .await
            .unwrap()
            .unwrap_err()
            .kind(),
        io::ErrorKind::UnexpectedEof
    );
    assert_eq!(timeout(OBSERVE, close).await.unwrap(), Ok(()));
}

#[tokio::test]
async fn admission_disconnect_after_request_has_unknown_disposition() {
    let (f, mut owner, client, listener) = admission_tls_exchange().await;
    let response = client.request().unwrap();
    let mut stream = accept_admission_peer(&f, &listener).await;
    read_admission_request(&mut stream, client.advertised).await;
    timeout(OBSERVE, stream.shutdown()).await.unwrap().unwrap();
    assert_eq!(response.reply().await, AdmissionReply::Unknown);
    assert_eq!(bounded_close(&mut owner).await, Ok(()));
}

#[tokio::test]
async fn admission_complete_tls_reply_returns_applied() {
    let (f, mut owner, client, listener) = admission_tls_exchange().await;
    let mut observed = events(&owner);
    let response = client.request().unwrap();
    let mut stream = accept_admission_peer(&f, &listener).await;
    read_admission_request(&mut stream, client.advertised).await;
    let answer = AdmissionReply::Applied(
        LogPos {
            term: 2,
            leader: 1,
            index: 9,
        },
        client.advertised,
    );
    timeout(
        OBSERVE,
        send_frame(&mut stream, &control::reply(route(), answer), false),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(response.reply().await, answer);
    assert_eq!(observed.try_recv(), Err(mpsc::error::TryRecvError::Empty));
    assert_eq!(bounded_close(&mut owner).await, Ok(()));
}

#[tokio::test]
async fn admission_wire_reply_rejects_address_substitution() {
    let address = "[fd7a:115c:a1e0::2]:7001".parse().unwrap();
    let position = LogPos {
        term: 2,
        leader: 1,
        index: 9,
    };
    for answer in [
        AdmissionReply::Applied(position, address),
        AdmissionReply::Revoked(position, address),
    ] {
        let frame = control::reply(reverse(), answer);
        assert_eq!(
            control::read_reply(&mut frame.as_slice(), reverse(), address).await,
            Ok(answer)
        );
        assert_eq!(
            control::read_reply(&mut frame.as_slice(), route(), address).await,
            Err(Refusal::Frame)
        );
        let mut other: SocketAddr = address;
        other.set_port(7002);
        assert_eq!(
            control::read_reply(&mut frame.as_slice(), reverse(), other).await,
            Err(Refusal::Frame)
        );
    }
}

#[tokio::test]
async fn admission_tls_refuses_claimed_sender_address_and_revoked_client() {
    let f = Fixture::new();
    let spy = rpc_spy(false);
    let mut owner = f.owner(2, spy.clone());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    owner.listen_on(listener).unwrap();
    let mut rx = events(&owner);
    let config = f.client_config(&f.client, 2);
    let advertised = "192.168.10.2:7001".parse().unwrap();
    let mut wrong_sender = route();
    wrong_sender.source = 6;

    for (frame, reason) in [
        (control::request(wrong_sender, advertised), Refusal::Frame),
        (control::request(route(), advertised), Refusal::Address),
    ] {
        assert!(
            exchange_learner_for_test(address, config.clone(), &frame)
                .await
                .is_err()
        );
        assert!(observe(&mut rx, Event::Stage(Stage::ServerAuthenticated)).await);
        assert!(observe(&mut rx, Event::Finished(Some(reason))).await);
    }

    assert!(f.remove(1));
    assert!(
        exchange_learner_for_test(address, config, &control::request(route(), advertised))
            .await
            .is_err()
    );
    assert!(observe(&mut rx, Event::Finished(Some(Refusal::Tls))).await);
    assert!(spy.calls.lock().unwrap().is_empty());
    assert!(bounded_close(&mut owner).await.is_ok());
}

#[tokio::test]
async fn admission_tls_original_anchor_is_exact_and_withdraws_at_cutover() {
    let f = Fixture::new();
    let dir = private_temp();
    let sm = SmAdapter(Arc::new(Mutex::new(
        StateStore::open(
            dir.path(),
            Init {
                cluster: CLUSTER,
                first_free: node(3),
            },
        )
        .unwrap(),
    )));
    let learner = learner_image(&f, 1, true, &sm);
    let resources = make_resources_for_learner(
        CLUSTER,
        node(2),
        &sm,
        Credentials {
            root: f.root.clone(),
            leaf: f.server.cert.clone(),
            key: f.server.private_key(),
            time: Arc::new(pki::FixedTime(1001)),
        },
        rpc_spy(false),
        Some(learner.clone()),
    )
    .unwrap();
    let ordinary = resources.client_config(node(1)).unwrap();
    let admission = resources.admission_client_config(node(1)).unwrap();
    let mut original = f.owner(1, rpc_spy(false));
    let mut substitute = f.owner_with(
        1,
        &Fixture::issue(&f.issuer, CLUSTER, 1, 940, 10000),
        rpc_spy(false),
    );
    let a = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let b = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (address, wrong) = (a.local_addr().unwrap(), b.local_addr().unwrap());
    original.listen_on(a).unwrap();
    substitute.listen_on(b).unwrap();

    assert_eq!(learner.phase(), Ok(peer_registry::Phase::Bootstrap));
    assert!(connect(ordinary.clone(), address).await.is_err());
    assert!(connect(admission.clone(), wrong).await.is_err());
    // Complete the authenticated exchange so both halves finish TLS before
    // observing the intentionally refused direct-route body on loopback.
    let frame = control::request(reverse(), "192.168.10.2:7001".parse().unwrap());
    let mut rx = events(&original);
    assert!(
        exchange_learner_for_test(address, admission.clone(), &frame)
            .await
            .is_err()
    );
    assert!(observe(&mut rx, Event::Finished(Some(Refusal::Address))).await);

    assert!(matches!(
        sm.0.lock()
            .unwrap()
            .apply(
                LogPos {
                    term: 1,
                    leader: 1,
                    index: 0
                },
                &StoredEntry::Enrollment(Command::Found {
                    cluster: CLUSTER,
                    members: FoundMembers::new([
                        (node(1), f.client.digest()),
                        (node(2), f.server.digest())
                    ])
                    .unwrap(),
                    proposed_at_ms: 100,
                }),
            )
            .unwrap(),
        Some(Outcome::Founded { replay: false })
    ));
    add_learner_membership(&sm);
    assert_eq!(learner.phase(), Ok(peer_registry::Phase::Strict));
    assert!(
        exchange_learner_for_test(address, admission.clone(), &frame)
            .await
            .is_err()
    );
    assert!(observe(&mut rx, Event::Finished(Some(Refusal::Address))).await);
    assert!(
        exchange_learner_for_test(address, ordinary, &frame)
            .await
            .is_err()
    );
    assert!(observe(&mut rx, Event::Finished(Some(Refusal::Address))).await);
    assert!(matches!(
        sm.0.lock()
            .unwrap()
            .apply(
                LogPos {
                    term: 1,
                    leader: 1,
                    index: 2
                },
                &StoredEntry::Enrollment(Command::Remove {
                    cluster: CLUSTER,
                    node: node(1),
                    proposed_at_ms: 101
                }),
            )
            .unwrap(),
        Some(Outcome::Removed { replay: false })
    ));
    assert!(connect(admission, address).await.is_err());
    assert!(bounded_close(&mut original).await.is_ok());
    assert!(bounded_close(&mut substitute).await.is_ok());
}

#[tokio::test(flavor = "current_thread")]
async fn issuer_channel_uses_mutual_tls13_exporters_bound_to_session_and_challenge() {
    use crate::cluster::consensus::issuer_transfer::Claim;
    let f = Fixture::new();
    let server = f.server_config(&f.server);
    let client = f.client_config(&f.client, 2);
    let claim = Claim {
        cluster: CLUSTER,
        root: crate::cluster::pki::ca::fingerprint_of(f.root.as_ref()),
        sender: (node(1), *f.client.digest().as_bytes()),
        receiver: (node(2), *f.server.digest().as_bytes()),
        epoch: log_id(7, 1, 6).leader_id,
        membership: LogPos {
            term: 6,
            leader: 1,
            index: 5,
        },
    };
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let mut previous = None;
    for _ in 0..2 {
        let (from_client, from_server) = tokio::join!(
            async {
                let stream = connect(client.clone(), address).await.unwrap();
                let connection = stream.get_ref().1;
                assert_eq!(
                    connection.protocol_version(),
                    Some(rustls::ProtocolVersion::TLSv1_3)
                );
                assert_eq!(
                    connection.handshake_kind(),
                    Some(rustls::HandshakeKind::Full)
                );
                let (cluster, peer, key) = AuthenticatedPeer::from_client(connection)
                    .unwrap()
                    .authenticated_identity()
                    .unwrap();
                assert!(cluster == CLUSTER && (peer, *key.as_bytes()) == claim.receiver);
                std::array::from_fn::<_, 2, _>(|i| {
                    connection
                        .export_keying_material(
                            [0; 32],
                            ISSUER_EXPORTER_LABEL,
                            Some(&claim.transcript(&[i as u8; 32])),
                        )
                        .unwrap()
                })
            },
            async {
                let (socket, _) = listener.accept().await.unwrap();
                let stream = TlsAcceptor::from(server.clone())
                    .accept(socket)
                    .await
                    .unwrap();
                let connection = stream.get_ref().1;
                assert_eq!(
                    connection.handshake_kind(),
                    Some(rustls::HandshakeKind::Full)
                );
                let (cluster, peer, key) = AuthenticatedPeer::from_server(connection)
                    .unwrap()
                    .authenticated_identity()
                    .unwrap();
                assert!(cluster == CLUSTER && (peer, *key.as_bytes()) == claim.sender);
                std::array::from_fn::<_, 2, _>(|i| {
                    connection
                        .export_keying_material(
                            [0; 32],
                            ISSUER_EXPORTER_LABEL,
                            Some(&claim.transcript(&[i as u8; 32])),
                        )
                        .unwrap()
                })
            }
        );
        assert!(from_client == from_server);
        assert!(from_client[0] != from_client[1]);
        assert!(previous != Some(from_client));
        previous = Some(from_client);
    }
}

mod issuer_refusals {
    use super::*;
    use crate::cluster::{
        consensus::issuer_transfer::{Claim, Offer},
        pki::custody::voter_transfer::{self as custody, Completion, Wipe},
    };
    use control::issuer as wire;
    use std::{
        fs::{self, OpenOptions},
        io::Write,
        path::PathBuf,
        sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    };

    #[derive(Clone)]
    struct Seal {
        peers: Arc<Fixture>,
        packages: Arc<AtomicUsize>,
    }

    impl issuer_channel::Sender for Seal {
        async fn seal<'a>(
            &'a self,
            _: Offer,
            key: &'a [u8; 32],
            transcript: &'a [u8],
            _: Instant,
        ) -> Result<Vec<u8>, Refusal> {
            self.packages.fetch_add(1, Ordering::SeqCst);
            custody::fixture_seal(
                key,
                transcript,
                self.peers.root.as_ref(),
                self.peers.issuer.key(),
            )
            .map_err(|_| Refusal::Identity)
        }
    }

    struct Receiver {
        peers: Arc<Fixture>,
        offer: Offer,
        file: PathBuf,
        revoke_before_readback: AtomicBool,
        completed: Mutex<Completion<()>>,
        writes: AtomicUsize,
    }

    impl issuer_channel::Receiver for Receiver {
        fn check(
            &self,
            claim: Claim,
            sender: AuthenticatedPeer,
            _: Instant,
        ) -> issuer_channel::ReceiveFuture<'_, Offer> {
            Box::pin(async move { self.check(claim, sender) })
        }

        fn install<'a>(
            &'a self,
            claim: Claim,
            sender: AuthenticatedPeer,
            channel: (&'a [u8; 32], &'a [u8], Vec<u8>),
            _: Instant,
        ) -> issuer_channel::ReceiveFuture<'a, ()> {
            Box::pin(async move { self.install(claim, sender, channel) })
        }
    }

    impl Receiver {
        fn check(&self, claim: Claim, _: AuthenticatedPeer) -> Result<Offer, Refusal> {
            if self.offer.claim != claim {
                return Err(Refusal::Identity);
            }
            self.peers
                .sm
                .0
                .lock()
                .unwrap()
                .state()
                .authorize_peer(CLUSTER, claim.receiver.0, self.peers.server.digest())
                .map_err(|_| Refusal::Identity)?;
            Ok(self.offer)
        }

        fn install(
            &self,
            claim: Claim,
            sender: AuthenticatedPeer,
            (key, transcript, package): (&[u8; 32], &[u8], Vec<u8>),
        ) -> Result<(), Refusal> {
            self.check(claim, sender)?;
            let mut completion = self.completed.lock().unwrap();
            completion.available().map_err(|_| Refusal::Identity)?;
            custody::fixture_authenticate(
                key,
                transcript,
                package.clone(),
                self.peers.root.as_ref(),
            )
            .map_err(|_| Refusal::Identity)?;
            completion.spend().map_err(|_| Refusal::Identity)?;
            if self.revoke_before_readback.swap(false, Ordering::SeqCst) {
                assert!(self.peers.remove(claim.receiver.0.get()));
            }
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&self.file)
                .map_err(|_| Refusal::Dispatch)?;
            output.write_all(&package).map_err(|_| Refusal::Dispatch)?;
            output.sync_all().map_err(|_| Refusal::Dispatch)?;
            assert!(fs::read(&self.file).unwrap() == package);
            self.writes.fetch_add(1, Ordering::SeqCst);
            completion
                .confirm((), || {
                    self.check(claim, sender)
                        .map(|_| ())
                        .map_err(|_| crate::cluster::pki::custody::Error::Ownership)
                })
                .map_err(|_| Refusal::Identity)
        }
    }

    struct TransferFixture {
        outbound: TransportOwner,
        inbound: TransportOwner,
        sender: Seal,
        receiver: Arc<Receiver>,
    }

    impl TransferFixture {
        async fn new() -> Self {
            let peers = Arc::new(Fixture::new());
            let outbound = peers.owner(1, Spy::exact(None));
            let mut inbound = peers.owner(2, Spy::exact(None));
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let position = LogPos {
                term: 6,
                leader: 1,
                index: 5,
            };
            let offer = Offer {
                claim: Claim {
                    cluster: CLUSTER,
                    root: crate::cluster::pki::ca::fingerprint_of(peers.root.as_ref()),
                    sender: (node(1), *peers.client.digest().as_bytes()),
                    receiver: (node(2), *peers.server.digest().as_bytes()),
                    epoch: log_id(7, 1, 6).leader_id,
                    membership: position,
                },
                sender_address: address,
                receiver_address: address,
            };
            let receiver = Arc::new(Receiver {
                offer,
                file: peers._dir.path().join("wire-install"),
                peers: peers.clone(),
                revoke_before_readback: AtomicBool::new(false),
                completed: Mutex::default(),
                writes: AtomicUsize::new(0),
            });
            inbound.shared.state.lock().unwrap().voter_receiver = Some(receiver.clone());
            inbound.listen_on(listener).unwrap();
            Self {
                outbound,
                inbound,
                sender: Seal {
                    peers,
                    packages: Arc::default(),
                },
                receiver,
            }
        }

        async fn connect(&self) -> tokio_rustls::client::TlsStream<TcpStream> {
            let peers = &self.sender.peers;
            connect(
                peers.client_config(&peers.client, 2),
                self.receiver.offer.receiver_address,
            )
            .await
            .unwrap()
        }

        async fn send(&self) -> TransferOutcome {
            self.outbound
                .send_issuer_transfer(self.sender.clone(), self.receiver.offer)
                .unwrap()
                .outcome()
                .await
        }

        async fn close(mut self) {
            self.outbound.close().await.unwrap();
            self.inbound.close().await.unwrap();
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn issuer_transfer_revocation_before_unwrap_and_during_install_never_confirms() {
        let fixture = TransferFixture::new().await;
        let mut client = fixture.connect().await;
        assert!(fixture.sender.peers.remove(2));
        wire::hello(&mut client, fixture.receiver.offer.claim)
            .await
            .unwrap();
        assert!(
            timeout(OBSERVE, wire::read_challenge(&mut client))
                .await
                .unwrap()
                .is_err()
        );
        assert!(!fixture.receiver.file.exists());
        assert!(fixture.receiver.completed.lock().unwrap().get().is_err());
        drop(client);
        fixture.close().await;

        let fixture = TransferFixture::new().await;
        fixture
            .receiver
            .revoke_before_readback
            .store(true, Ordering::SeqCst);
        assert_eq!(fixture.send().await, TransferOutcome::Unknown);
        assert_eq!(fixture.receiver.writes.load(Ordering::SeqCst), 1);
        assert!(fixture.receiver.file.is_file());
        {
            let mut completion = fixture.receiver.completed.lock().unwrap();
            assert!(completion.get().is_err());
            assert!(completion.available().is_err());
            assert!(completion.confirm((), || Ok(())).is_err());
        }
        fixture.close().await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn issuer_transfer_refuses_sealed_package_replayed_on_another_tls_session() {
        let fixture = TransferFixture::new().await;
        let claim = fixture.receiver.offer.claim;
        let mut original = fixture.connect().await;
        wire::hello(&mut original, claim).await.unwrap();
        let challenge = timeout(OBSERVE, wire::read_challenge(&mut original))
            .await
            .unwrap()
            .unwrap();
        let transcript = claim.transcript(&challenge);
        let mut key = Wipe([0; 32]);
        original
            .get_ref()
            .1
            .export_keying_material(&mut key.0[..], ISSUER_EXPORTER_LABEL, Some(&transcript))
            .unwrap();
        let package = custody::fixture_seal(
            &key.0,
            &transcript,
            fixture.sender.peers.root.as_ref(),
            fixture.sender.peers.issuer.key(),
        )
        .unwrap();
        drop(key);
        original.shutdown().await.unwrap();
        drop(original);

        let mut replay = fixture.connect().await;
        wire::hello(&mut replay, claim).await.unwrap();
        let new_challenge = timeout(OBSERVE, wire::read_challenge(&mut replay))
            .await
            .unwrap()
            .unwrap();
        assert_ne!(new_challenge, challenge);
        wire::sealed(&mut replay, &package).await.unwrap();
        assert!(
            timeout(
                OBSERVE,
                wire::read_acknowledgement(&mut replay, &new_challenge)
            )
            .await
            .unwrap()
            .is_err()
        );
        assert_eq!(fixture.receiver.writes.load(Ordering::SeqCst), 0);
        assert!(!fixture.receiver.file.exists());
        assert_eq!(fixture.send().await, TransferOutcome::Installed);
        assert_eq!(fixture.receiver.writes.load(Ordering::SeqCst), 1);
        fixture.close().await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn issuer_transfer_lost_acknowledgement_is_unknown_and_single_install() {
        let fixture = TransferFixture::new().await;
        fixture
            .inbound
            .shared
            .hooks
            .drop_issuer_ack
            .store(true, Ordering::SeqCst);
        assert_eq!(fixture.send().await, TransferOutcome::Unknown);
        assert!(fixture.receiver.completed.lock().unwrap().get().is_ok());
        let envelope = fs::read(&fixture.receiver.file).unwrap();
        assert!(matches!(
            fixture
                .outbound
                .send_issuer_transfer(fixture.sender.clone(), fixture.receiver.offer),
            Err(Refusal::Identity),
        ));
        assert_eq!(fixture.sender.packages.load(Ordering::SeqCst), 1);
        assert_eq!(fixture.receiver.writes.load(Ordering::SeqCst), 1);
        assert!(fs::read(&fixture.receiver.file).unwrap() == envelope);
        fixture.close().await;
    }

    #[cfg(target_os = "linux")]
    mod production_receiver {
        use super::*;
        use crate::cluster::{
            consensus::{
                BasicNode,
                bound_start::Job,
                checked_clock::{CheckedClock, HostClock},
                encode,
                membership_observation::Sample,
            },
            enrollment::durable_join::PublishedJoin,
            issuance_publication as publication,
            join_receipt::{ExpectedIdentity, Origin},
            pki::{ca::ClusterCa, csr::ProvenCsr, custody::LearnerCustody},
            startup_plan::{Owner, tests::Layout},
        };
        use custody::ReceiveStage::{self, *};
        use std::{net::UdpSocket, os::unix::fs::PermissionsExt, sync::TryLockError};

        const ORIGINAL: LogPos = LogPos {
            term: 1,
            leader: 1,
            index: 1,
        };
        const PROMOTED: LogPos = LogPos {
            term: 1,
            leader: 1,
            index: 2,
        };

        fn promoted_store(
            folder: &std::path::Path,
            bindings: [(NodeId, KeyDigest); 3],
            address: SocketAddr,
            fail_writes: bool,
        ) -> (SmAdapter, Members) {
            let init = Init {
                cluster: CLUSTER,
                first_free: node(5),
            };
            let mut store = StateStore::open(folder, init).unwrap();
            let seed = StoredEntry::Enrollment(Command::Found {
                cluster: CLUSTER,
                members: FoundMembers::new(bindings).unwrap(),
                proposed_at_ms: 100,
            });
            assert!(matches!(
                store.apply(
                    LogPos {
                        index: 0,
                        ..ORIGINAL
                    },
                    &seed
                ),
                Ok(Some(Outcome::Founded { replay: false }))
            ));
            store.apply(ORIGINAL, &StoredEntry::Blank).unwrap();
            let voters = bindings.map(|(node, _)| node.get());
            let members = Members::new(
                vec![voters.into()],
                voters
                    .map(|node| (node, BasicNode::new(address.to_string())))
                    .into_iter()
                    .collect::<BTreeMap<_, _>>(),
            )
            .unwrap();
            store
                .apply(
                    PROMOTED,
                    &StoredEntry::Membership(encode(&members).unwrap()),
                )
                .unwrap();
            if fail_writes {
                // Reopen the same image with a real SQL write failure armed.
                drop(store);
                let db = rusqlite::Connection::open(folder.join("cluster-state.sqlite3")).unwrap();
                db.execute_batch("CREATE TRIGGER fail_issuer_write BEFORE UPDATE ON sm BEGIN SELECT RAISE(FAIL,'fixture'); END;").unwrap();
                drop(db);
                store = StateStore::open(folder, init).unwrap();
            }
            (SmAdapter(Arc::new(Mutex::new(store))), members)
        }

        async fn run(stop: Option<ReceiveStage>, expected_stages: &[ReceiveStage]) {
            run_with(stop, expected_stages, false).await;
        }

        async fn run_with(
            stop: Option<ReceiveStage>,
            expected_stages: &[ReceiveStage],
            fail_before_final_proof: bool,
        ) {
            let route = UdpSocket::bind("0.0.0.0:0").unwrap();
            route.connect("192.0.2.1:9").unwrap(); // Select a local interface without sending data.
            let ip = route.local_addr().unwrap().ip();
            let listener = TcpListener::bind((ip, 0)).await.unwrap();
            let address = listener.local_addr().unwrap();
            assert!(control::qualified(address));

            let receiver = Layout::new();
            let mut plan = receiver.plan("join");
            plan.peer = address.to_string();
            plan.enrollment = SocketAddr::new(ip, 7002).to_string();
            let mut owner = Owner::acquire(plan).unwrap();
            let [wrap, staging] = ["wrap", "staging"].map(|name| {
                let path = receiver.root.path().join(name);
                fs::create_dir(&path).unwrap();
                fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
                path
            });
            let clock = Arc::new(HostClock::new(0));
            let now = clock.sample_ms().unwrap();
            let ca = ClusterCa::ephemeral(CLUSTER, now);
            let root = ca.cert_der();
            let issuer = Issuer::from_ca_cert_der(
                &CertificateDer::from(root),
                KeyPair::try_from(custody::Wipe(ca.private_marker()).0.as_slice()).unwrap(),
            )
            .unwrap();
            let seconds = (now / 1000) as i64;
            let client = Fixture::issue(&issuer, CLUSTER, 1, seconds - 60, seconds + 3600);
            let mut learner = LearnerCustody::create(&owner, &wrap, &staging).unwrap();
            let csr = ProvenCsr::parse(&learner.seal(CLUSTER, ca.fingerprint()).unwrap()).unwrap();
            let ticket = crate::cluster::consensus::issue::test_ticket(csr.digest());
            let target = ticket.node();
            let leaf = ca.sign_leaf(&ticket, &csr, now).unwrap();
            let expected = ExpectedIdentity {
                cluster: CLUSTER,
                node: target,
                key: csr.digest(),
                ca: ca.fingerprint(),
                origin: Origin::Redeem {
                    term: ORIGINAL.term,
                    leader: ORIGINAL.leader,
                    index: ORIGINAL.index,
                    anchors: vec![(1, *client.digest().as_bytes())],
                },
            };
            publication::reserve(&mut owner).unwrap();
            publication::store(
                &mut owner,
                Some(&expected),
                learner.installation(),
                root,
                &leaf,
                &learner.member().unwrap(),
                Some(now),
            )
            .unwrap();
            let applied = private_temp();
            let (sm, members) = promoted_store(
                applied.path(),
                [
                    (node(1), client.digest()),
                    (node(3), KeyDigest::of_spki(&[3]).unwrap()),
                    (target, csr.digest()),
                ],
                address,
                fail_before_final_proof,
            );
            let epoch = log_id(2, 1, PROMOTED.index + 1).leader_id;
            let sample = Sample::issuer_fixture(epoch, id(PROMOTED), members.clone());
            let events = Arc::new(Mutex::new(Vec::new()));
            let envelope = receiver.root.path().join("voter-custody/ca-key");
            learner.observe_voter_receive({
                let sm = sm.clone();
                let events = events.clone();
                let envelope = envelope.clone();
                move |stage| {
                    events.lock().unwrap().push(stage);
                    if matches!(stage, ChannelOpen | ReadbackOpen | Confirmed) {
                        assert!(matches!(sm.0.try_lock(), Err(TryLockError::WouldBlock)));
                        return;
                    }
                    let mut store =
                        sm.0.try_lock()
                            .expect("apply can proceed at a disk boundary");
                    assert_eq!(envelope.is_file(), stage == BeforeReadback);
                    if Some(stage) == stop {
                        assert!(matches!(
                            store.apply(
                                LogPos {
                                    term: 2,
                                    index: PROMOTED.index + 1,
                                    ..PROMOTED
                                },
                                &StoredEntry::Enrollment(Command::Remove {
                                    cluster: CLUSTER,
                                    node: target,
                                    proposed_at_ms: 101,
                                }),
                            ),
                            Ok(Some(Outcome::Removed { replay: false }))
                        ));
                    }
                }
            });
            let (job, credentials, gate) = Job::issuer_receiver_fixture(
                PublishedJoin {
                    owner,
                    custody: learner,
                    expected,
                    leaf,
                    time: Arc::new(pki::FixedTime(seconds as u64)),
                },
                &sm,
                sample,
                clock,
            );
            let claim = Claim {
                cluster: CLUSTER,
                root: ca.fingerprint(),
                sender: (node(1), *client.digest().as_bytes()),
                receiver: (target, *csr.digest().as_bytes()),
                membership: PROMOTED,
                epoch,
            };
            let client_config = verify::client_config(
                verify::PeerVerifier::new(
                    credentials.root.clone(),
                    CLUSTER,
                    peer_registry::registry(&sm),
                )
                .unwrap()
                .client(target),
                client.cert.clone(),
                client.private_key(),
                credentials.time.clone(),
            )
            .unwrap();
            let dispatch = Spy::exact(None);
            let (mut transport, _) = TransportOwner::pending_founder(CLUSTER, target);
            let resources = make_resources_for_learner(
                CLUSTER,
                target,
                &sm,
                credentials,
                dispatch.clone(),
                Some(gate),
            )
            .unwrap();
            transport.publish(&sm, resources, listener).unwrap();
            transport.receive_issuer_transfer(job).unwrap();

            let mut stream = timeout(OBSERVE, connect(Arc::new(client_config), address))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                stream.get_ref().1.handshake_kind(),
                Some(rustls::HandshakeKind::Full)
            );
            assert_eq!(
                stream.get_ref().1.protocol_version(),
                Some(rustls::ProtocolVersion::TLSv1_3)
            );
            wire::hello(&mut stream, claim).await.unwrap();
            let challenge = timeout(OBSERVE, wire::read_challenge(&mut stream))
                .await
                .unwrap()
                .unwrap();
            let transcript = claim.transcript(&challenge);
            let mut key = Wipe([0; 32]);
            stream
                .get_ref()
                .1
                .export_keying_material(&mut key.0[..], ISSUER_EXPORTER_LABEL, Some(&transcript))
                .unwrap();
            let sealed = custody::fixture_seal(&key.0, &transcript, root, issuer.key()).unwrap();
            wire::sealed(&mut stream, &sealed).await.unwrap();
            let ack = timeout(OBSERVE, wire::read_acknowledgement(&mut stream, &challenge))
                .await
                .unwrap();
            assert_eq!(ack.is_ok(), stop.is_none());
            drop(stream);
            transport.close().await.unwrap();
            assert_eq!(dispatch.count(), 0);
            assert_eq!(*events.lock().unwrap(), expected_stages);
            assert_eq!(envelope.exists(), stop != Some(BeforeUnwrap));
            assert_eq!(
                envelope.with_file_name("reservation").exists(),
                stop != Some(BeforeUnwrap)
            );
            let survivor = log_id(3, target.get(), PROMOTED.index + 2).leader_id;
            let survivor = (
                Sample::issuer_fixture(survivor, id(PROMOTED), members),
                survivor,
            );
            if !fail_before_final_proof {
                job.assert_fixture_custody(root, stop.is_none().then_some(PROMOTED), survivor);
                return;
            }
            let store = sm.clone();
            job.assert_poisoned_final_proof_refuses(
                root,
                survivor,
                Arc::new(move || {
                    let mut store = store
                        .0
                        .try_lock()
                        .expect("the final proof has not acquired the store");
                    let old = store.snapshot();
                    assert_eq!(
                        store.apply(
                            LogPos {
                                index: PROMOTED.index + 1,
                                ..PROMOTED
                            },
                            &StoredEntry::Blank
                        ),
                        Err(crate::cluster::state_store::StoreError::Sql)
                    );
                    assert!(store.is_poisoned());
                    assert_eq!(old, store.snapshot());
                }),
            );
        }

        #[tokio::test(flavor = "current_thread")]
        async fn applied_revocation_before_channel_open_refuses_production_receiver() {
            run(Some(BeforeUnwrap), &[BeforeUnwrap]).await;
        }

        #[tokio::test(flavor = "current_thread")]
        async fn applied_revocation_before_readback_refuses_production_receiver() {
            run(
                Some(BeforeReadback),
                &[BeforeUnwrap, ChannelOpen, BeforeReadback],
            )
            .await;
        }

        #[tokio::test(flavor = "current_thread")]
        async fn persistence_failure_before_final_proof_refuses_promoted_readiness() {
            run_with(
                None,
                &[
                    BeforeUnwrap,
                    ChannelOpen,
                    BeforeReadback,
                    ReadbackOpen,
                    Confirmed,
                ],
                true,
            )
            .await;
        }

        #[tokio::test(flavor = "current_thread")]
        async fn production_receiver_holds_apply_guard_through_confirmation() {
            run(
                None,
                &[
                    BeforeUnwrap,
                    ChannelOpen,
                    BeforeReadback,
                    ReadbackOpen,
                    Confirmed,
                ],
            )
            .await;
        }
        use crate::cluster::{
            consensus::startup_recovery,
            restart_history::{TestFault, tests as history_fixture},
            startup_plan::Mode,
        };

        #[derive(Clone)]
        struct HistorySender {
            ca: Arc<ClusterCa>,
            sealed: Arc<AtomicUsize>,
        }

        impl issuer_channel::Sender for HistorySender {
            async fn seal<'a>(
                &'a self,
                _: Offer,
                key: &'a [u8; 32],
                transcript: &'a [u8],
                _: Instant,
            ) -> Result<Vec<u8>, Refusal> {
                self.sealed.fetch_add(1, Ordering::SeqCst);
                let private = Wipe(self.ca.private_marker());
                let keypair =
                    KeyPair::try_from(private.0.as_slice()).map_err(|_| Refusal::Identity)?;
                custody::fixture_seal(key, transcript, self.ca.cert_der(), &keypair)
                    .map_err(|_| Refusal::Identity)
            }
        }

        async fn history_receiver_child(base: &std::path::Path, case: &str) {
            // The connection is to this process's own listener. Use the
            // host-local private address so the unchanged production route
            // check also authorizes the sender.
            let route = UdpSocket::bind("0.0.0.0:0").unwrap();
            route.connect("192.0.2.1:9").unwrap();
            let ip = route.local_addr().unwrap().ip();
            let listener = TcpListener::bind((ip, 0)).await.unwrap();
            let address = listener.local_addr().unwrap();
            assert!(control::qualified(address));
            let enrollment = SocketAddr::new(ip, if address.port() == 7002 { 7003 } else { 7002 });
            let mut plan = history_fixture::plan(base, Mode::Join);
            plan.peer = address.to_string();
            plan.enrollment = enrollment.to_string();
            fs::write(&plan.config, format!("{address}\n{enrollment}\n")).unwrap();
            let wrap = history_fixture::private_dir(base, "wrap");
            let stage = history_fixture::private_dir(base, "custody");
            history_fixture::private_dir(base, "history");
            let mut owner = Owner::acquire(plan).unwrap();
            let mut slot = history_fixture::slot(base)
                .check_fresh(&owner, &wrap, &stage)
                .unwrap();
            let clock = Arc::new(HostClock::new(0));
            let now = clock.sample_ms().unwrap();
            let ca = Arc::new(ClusterCa::ephemeral(CLUSTER, now));
            let root = ca.cert_der();
            let ca_key = Wipe(ca.private_marker());
            let issuer = Issuer::from_ca_cert_der(
                &CertificateDer::from(root),
                KeyPair::try_from(ca_key.0.as_slice()).unwrap(),
            )
            .unwrap();
            let seconds = (now / 1000) as i64;
            let client = Fixture::issue(&issuer, CLUSTER, 1, seconds - 60, seconds + 3600);
            let mut learner = LearnerCustody::create(&owner, &wrap, &stage).unwrap();
            let csr = ProvenCsr::parse(&learner.seal(CLUSTER, ca.fingerprint()).unwrap()).unwrap();
            slot.bind(learner.history_auth()).unwrap();
            let ticket = crate::cluster::consensus::issue::test_ticket(csr.digest());
            let target = ticket.node();
            let leaf = ca.sign_leaf(&ticket, &csr, now).unwrap();
            let expected = ExpectedIdentity {
                cluster: CLUSTER,
                node: target,
                key: csr.digest(),
                ca: ca.fingerprint(),
                origin: Origin::Redeem {
                    term: ORIGINAL.term,
                    leader: ORIGINAL.leader,
                    index: ORIGINAL.index,
                    anchors: vec![(1, *client.digest().as_bytes())],
                },
            };
            publication::reserve(&mut owner).unwrap();
            publication::store(
                &mut owner,
                Some(&expected),
                learner.installation(),
                root,
                &leaf,
                &learner.member().unwrap(),
                Some(now),
            )
            .unwrap();
            let live = history_fixture::learner_record(
                &mut slot,
                &owner,
                &learner,
                &expected,
                &leaf,
                (node(1), client.digest()),
            );
            let (mut applied, mut log) = history_fixture::learner_stores(&mut owner);
            let bindings = [
                (node(1), client.digest()),
                (node(3), KeyDigest::of_spki(&[3]).unwrap()),
                (target, csr.digest()),
            ];
            let voters = bindings.map(|(id, _)| id.get());
            let members = Members::new(
                vec![voters.into()],
                voters
                    .into_iter()
                    .map(|id| (id, BasicNode::new(address.to_string())))
                    .collect::<BTreeMap<_, _>>(),
            )
            .unwrap();
            let entries = [
                (
                    LogPos {
                        index: 0,
                        ..ORIGINAL
                    },
                    StoredEntry::Enrollment(Command::Found {
                        cluster: CLUSTER,
                        members: FoundMembers::new(bindings).unwrap(),
                        proposed_at_ms: 100,
                    }),
                ),
                (ORIGINAL, StoredEntry::Blank),
                (PROMOTED, StoredEntry::Membership(encode(&members).unwrap())),
            ];
            for (where_, entry) in &entries {
                applied.apply(*where_, entry).unwrap();
            }
            log.append(&entries).unwrap();
            let sm = SmAdapter(Arc::new(Mutex::new(applied)));
            let epoch = log_id(2, 1, PROMOTED.index + 1).leader_id;
            let sample = Sample::issuer_fixture(epoch, id(PROMOTED), members);
            let events = Arc::new(Mutex::new(Vec::new()));
            let voter_file = base.join("history/voter");
            let revoke = case == "revoke";
            learner.observe_voter_receive({
                let events = events.clone();
                let sm = sm.clone();
                let voter_file = voter_file.clone();
                move |stage| {
                    events.lock().unwrap().push(stage);
                    if matches!(stage, ChannelOpen | ReadbackOpen | HistoryCandidate) {
                        assert!(matches!(sm.0.try_lock(), Err(TryLockError::WouldBlock)));
                        assert!(!voter_file.exists());
                        return;
                    }
                    let mut store =
                        sm.0.try_lock()
                            .expect("disk and final latches are outside the apply guard");
                    assert_eq!(
                        voter_file.is_file(),
                        matches!(stage, HistoryCommitted | Confirmed)
                    );
                    if stage == BeforeReadback && revoke {
                        let next = LogPos {
                            index: PROMOTED.index + 1,
                            ..PROMOTED
                        };
                        assert_eq!(
                            store
                                .apply(
                                    next,
                                    &StoredEntry::Enrollment(Command::Remove {
                                        cluster: CLUSTER,
                                        node: target,
                                        proposed_at_ms: 101,
                                    })
                                )
                                .unwrap(),
                            Some(Outcome::Removed { replay: false })
                        );
                    }
                }
            });
            let (job, credentials, gate) = Job::issuer_receiver_fixture(
                PublishedJoin {
                    owner,
                    custody: learner,
                    expected,
                    leaf,
                    time: Arc::new(pki::FixedTime(seconds as u64)),
                },
                &sm,
                sample,
                clock,
            );
            job.set_receiver_fixture_history(slot, live, &sm).await;
            assert!(base.join("history/strict").is_file());
            let fault = match case {
                "temp-sync" => Some(TestFault::FileSync),
                "final-sync" => Some(TestFault::DirectorySync),
                "substitute" => Some(TestFault::VoterSubstitution),
                _ => None,
            };
            if let Some(fault) = fault {
                job.fault_receiver_fixture_history(fault);
            }
            let claim = Claim {
                cluster: CLUSTER,
                root: ca.fingerprint(),
                membership: PROMOTED,
                epoch,
                sender: (node(1), *client.digest().as_bytes()),
                receiver: (target, *csr.digest().as_bytes()),
            };
            let offer = Offer {
                claim,
                sender_address: address,
                receiver_address: address,
            };
            let dispatch = Spy::exact(None);
            let mut outbound = TransportOwner::new(
                CLUSTER,
                node(1),
                &sm,
                Credentials {
                    root: credentials.root.clone(),
                    leaf: client.cert.clone(),
                    key: client.private_key(),
                    time: credentials.time.clone(),
                },
                dispatch.clone(),
            )
            .unwrap();
            let (mut inbound, _) = TransportOwner::pending_founder(CLUSTER, target);
            let res = make_resources_for_learner(
                CLUSTER,
                target,
                &sm,
                credentials,
                dispatch.clone(),
                Some(gate),
            )
            .unwrap();
            inbound.publish(&sm, res, listener).unwrap();
            inbound.receive_issuer_transfer(job).unwrap();
            if case == "lost-ack" {
                inbound
                    .shared
                    .hooks
                    .drop_issuer_ack
                    .store(true, Ordering::SeqCst);
            }
            let sender = HistorySender {
                ca,
                sealed: Arc::default(),
            };
            let outcome = outbound
                .send_issuer_transfer(sender.clone(), offer)
                .unwrap()
                .outcome()
                .await;
            assert_eq!(
                outcome,
                if matches!(case, "healthy" | "tamper") {
                    TransferOutcome::Installed
                } else {
                    TransferOutcome::Unknown
                }
            );
            assert_eq!(sender.sealed.load(Ordering::SeqCst), 1);
            assert!(matches!(
                outbound.send_issuer_transfer(sender, offer),
                Err(Refusal::Identity)
            ));
            inbound.close().await.unwrap();
            outbound.close().await.unwrap();
            assert_eq!(dispatch.count(), 0);

            let completed = matches!(case, "healthy" | "lost-ack" | "tamper");
            let mut expected_stages = vec![BeforeUnwrap, ChannelOpen, BeforeReadback];
            if case != "revoke" {
                expected_stages.extend([ReadbackOpen, HistoryCandidate]);
            }
            if completed {
                expected_stages.extend([HistoryCommitted, Confirmed]);
            }
            assert_eq!(*events.lock().unwrap(), expected_stages);
            assert_eq!(
                job.receiver_fixture_completion(),
                (completed.then_some(PROMOTED), completed, false)
            );
            assert_eq!(voter_file.is_file(), completed || case == "final-sync");
            assert_eq!(
                base.join("history/.voter.tmp").is_file(),
                case == "temp-sync"
            );
            assert!(base.join("voter-custody/ca-key").is_file());
            // Keep the job/custody and stores owned until the subprocess exits.
            std::mem::forget(log);
        }

        async fn history_receiver_case(selector: &'static str, case: &'static str) {
            if let Some((base, observed)) = history_fixture::probe() {
                assert_eq!(observed, case);
                return history_receiver_child(&base, case).await;
            }
            let layout = Layout::new();
            let base = layout.root.path();
            history_fixture::subprocess(selector, base, case);
            let mut plan = history_fixture::plan(base, Mode::RecoverJoiner);
            let config = fs::read_to_string(&plan.config).unwrap();
            let [peer, enrollment] = config.lines().collect::<Vec<_>>().try_into().unwrap();
            plan.peer = peer.into();
            plan.enrollment = enrollment.into();
            if case == "tamper" {
                let envelope = base.join("voter-custody/ca-key");
                let mut ciphertext = fs::read(&envelope).unwrap();
                let last = ciphertext.last_mut().unwrap();
                *last ^= 1;
                fs::write(&envelope, &ciphertext).unwrap();
            }
            let owner = Owner::acquire(plan).unwrap();
            let recovery = startup_recovery::recover_process(
                owner,
                history_fixture::slot(base),
                &base.join("wrap"),
                &base.join("custody"),
                true,
            );
            if case == "tamper" {
                assert_eq!(
                    recovery.sqlite_opens(),
                    2,
                    "only the read-only pair may open"
                );
            }
            let recovery = recovery.into_ready();
            if matches!(case, "healthy" | "lost-ack" | "final-sync") {
                let Ok(recovery) = recovery else {
                    panic!("complete authentic voter history must reopen")
                };
                assert_eq!(
                    recovery.history.verified().unwrap().voter_position(),
                    Some(PROMOTED)
                );
            } else {
                assert!(
                    recovery.is_err(),
                    "partial or substituted voter history refuses"
                );
            }
        }

        macro_rules! history_receiver_test {
            ($test:ident, $case:literal) => {
                #[tokio::test(flavor = "current_thread")]
                async fn $test() {
                    let (_, selector) = concat!(module_path!(), "::", stringify!($test))
                        .split_once("::")
                        .unwrap();
                    history_receiver_case(selector, $case).await;
                }
            };
        }

        history_receiver_test!(receiver_history_healthy_ack_after_durable_pair, "healthy");
        history_receiver_test!(receiver_history_revocation_before_readback, "revoke");
        history_receiver_test!(
            receiver_history_temp_sync_failure_preserves_partial,
            "temp-sync"
        );
        history_receiver_test!(
            receiver_history_final_sync_failure_qualifies_only_after_exit,
            "final-sync"
        );
        history_receiver_test!(
            receiver_history_postproof_substitution_never_confirms,
            "substitute"
        );
        history_receiver_test!(
            receiver_history_lost_ack_spends_sender_and_recovers,
            "lost-ack"
        );
        history_receiver_test!(receiver_history_ciphertext_tamper_refuses_reopen, "tamper");
    }
}
