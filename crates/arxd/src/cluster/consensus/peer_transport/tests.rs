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
        let mut store = StateStore::open(
            dir.path(),
            Init {
                cluster: CLUSTER,
                first_free: node(3),
            },
        )
        .unwrap();
        let applied = store
            .apply(
                LogPos {
                    term: 1,
                    leader: 1,
                    index: 0,
                },
                &StoredEntry::Enrollment(Command::Found {
                    cluster: CLUSTER,
                    members: FoundMembers::new([
                        (node(1), client.digest()),
                        (node(2), server.digest()),
                    ])
                    .unwrap(),
                    proposed_at_ms: 100,
                }),
            )
            .unwrap();
        assert!(matches!(applied, Some(Outcome::Founded { replay: false })));
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
async fn learner_once(case: u8) -> Result<(), &'static str> {
    use super::super::{ConsensusNode, MembershipBlob, Params, VoteLogStore, WireMembership};
    use futures_util::FutureExt;
    use openraft::network::{NetSnapshot, NetVote, RaftNetworkFactory};
    let mut f = Fixture::new();
    let port = 43181 + u16::from(case) * 2;
    let sa: SocketAddr = format!("127.0.0.1:{port}").parse().map_err(|_| "address")?;
    let ra: SocketAddr = format!("127.0.0.1:{}", port + 1)
        .parse()
        .map_err(|_| "address")?;
    let sl = TcpListener::bind(sa)
        .await
        .map_err(|_| "sender reservation")?;
    let rl = TcpListener::bind(ra)
        .await
        .map_err(|_| "receiver reservation")?;
    if sl.local_addr().map_err(|_| "sender address")? != sa
        || rl.local_addr().map_err(|_| "receiver address")? != ra
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
    let original = seed_learner(&rd, &f, &m, None)?;
    seed_learner(&sd, &f, &m, None)?;
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
    let sender_release = f.sm.0.lock().map_err(|_| "sender lock")?.released();
    let sender_weak = Arc::downgrade(&f.sm.0);
    let credentials = |identity: &Identity| Credentials {
        root: f.root.clone(),
        leaf: identity.cert.clone(),
        key: identity.private_key(),
        time: Arc::new(pki::FixedTime(1001)),
    };
    let (mut sender, mut sf) = TransportOwner::pending(CLUSTER, node(1), credentials(&f.client));
    sf.loopback = true;
    let (mut receiver, mut rf) = TransportOwner::pending(CLUSTER, node(2), credentials(&f.server));
    rf.loopback = true;
    let mut pending = sf
        .new_client(2, &openraft::BasicNode::new(ra.to_string()))
        .await;
    let pending_refused = pending
        .pre_vote(ballot_request(), RPCOption::new(Duration::from_secs(4)))
        .await
        .is_err();
    let rdir = rd.clone();
    let mut startup =
        tokio::spawn(
            async move { ConsensusNode::start(2, &rdir, init, Params::default(), rf).await },
        );
    // Preserve sole startup handle on observation expiry. Never abort/drop/retry it.
    let (startup_observed, started) = match timeout(OBSERVE, &mut startup).await {
        Ok(r) => (true, r),
        Err(_) => (false, startup.await),
    };
    let mut receiver_node = match started {
        Ok(Ok(n)) => n,
        _ => {
            let _ = sender.close().await;
            let _ = receiver.close().await;
            return Err("startup failed; Engine cleanup unconfirmed");
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
        if initial.state != openraft::ServerState::Learner
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
    let sc = sender.close().await;
    let rc = receiver.close().await;
    if !sender.joined() || !receiver.joined() {
        return Err("transport cleanup unconfirmed; node shutdown withheld");
    }
    let shutdown = receiver_node.shutdown().await;
    let owners_released = log_release.is_set()
        && sm_release.is_set()
        && log_weak.strong_count() == 0
        && sm_weak.strong_count() == 0;
    drop(receiver_node);
    drop(f.sm);
    let sender_released = sender_release.is_set() && sender_weak.strong_count() == 0;
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
    let mut readback = tokio::task::spawn_blocking(move || -> Result<(), &'static str> {
        let mut log = VoteLogStore::open(&rd, CLUSTER).map_err(|_| "receiver log reopen")?;
        let sm = StateStore::open(&rd, init).map_err(|_| "receiver state reopen")?;
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
        let lr = log.released();
        let sr = sm.released();
        drop(log);
        drop(sm);
        let sender = StateStore::open(&sd, init).map_err(|_| "sender state reopen")?;
        let ss = sender.released();
        drop(sender);
        if !lr.is_set() || !sr.is_set() || !ss.is_set() {
            return Err("reopen release");
        };
        Ok(())
    });
    let (readback_observed, reopened) = match timeout(OBSERVE, &mut readback).await {
        Ok(r) => (true, r),
        Err(_) => (false, readback.await),
    };
    let reopened = reopened.map_err(|_| "readback worker join")?;
    if !readback_observed {
        return Err("readback observation expired; worker joined");
    }

    let weak_refused = pending
        .pre_vote(ballot_request(), RPCOption::new(Duration::from_secs(4)))
        .await
        .is_err();
    drop((sf, pending));
    if sc.is_err()
        || rc.is_err()
        || shutdown.is_err()
        || !owners_released
        || !sender_released
        || !weak_refused
    {
        return Err("joined teardown/release");
    }
    reopened?;
    outcome.map_err(|_| "fixture body panic after cleanup")??;
    Ok(())
}
#[tokio::test]
async fn rpc_learner_p_once() {
    assert!(learner_once(0).await.is_ok())
}
#[tokio::test]
async fn rpc_learner_v_once() {
    assert!(learner_once(1).await.is_ok())
}
#[tokio::test]
async fn rpc_learner_a_once() {
    assert!(learner_once(2).await.is_ok())
}
#[tokio::test]
async fn rpc_learner_s_once() {
    assert!(learner_once(3).await.is_ok())
}
#[tokio::test]
async fn rpc_snapshot_cancel_owned_future_at_tls_and_reply_phases() {
    use openraft::network::NetSnapshot;
    for phase in [
        Stage::ClientHandshake,
        Stage::ClientAuthenticated,
        Stage::ReplyRead,
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
        let charged = sender.shared.connections.available_permits() == 31;
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
