use super::*;
use crate::cluster::{
    enrollment::JoinCode,
    pki::{
        csr::ProvenCsr,
        preproof::{ExchangeError, exchange},
        preproof_tests::{Fixture, ProbeTime, fixture, fixture_identity, wrong_signer_config},
        verify::SERVER_NAME,
    },
};
use rcgen::{CertificateParams, KeyPair, PKCS_ED25519, SanType};
use rustls::pki_types::{PrivatePkcs8KeyDer, ServerName};
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst};
use tokio_rustls::{TlsConnector, client::TlsStream as ClientTls};

const WATCHDOG: Duration = Duration::from_secs(2);
const REFUSAL_DEADLINE: Duration = Duration::from_secs(5);
const REQUEST: RequestId = [17; 16];

async fn within<T>(future: impl Future<Output = T>) -> T {
    timeout(WATCHDOG, future).await.expect("loopback watchdog")
}

struct Gate {
    arrived: Semaphore,
    release: Semaphore,
    resource: Arc<Semaphore>,
}

impl Gate {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            arrived: Semaphore::new(0),
            release: Semaphore::new(0),
            resource: Arc::new(Semaphore::new(1)),
        })
    }

    async fn hold(&self) {
        let _resource = self.resource.clone().try_acquire_owned().unwrap();
        self.arrived.add_permits(1);
        self.release.acquire().await.unwrap().forget();
    }
}

#[derive(Default)]
struct Observed {
    anchors: AtomicUsize,
    redeems: Mutex<Vec<Redeem>>,
}

struct Synthetic {
    query: Query,
    list: Vec<u8>,
    observation: Arc<Observed>,
    anchor_work: Option<Arc<Gate>>,
    redeem_work: Option<Arc<Gate>>,
    refuse_redeem: bool,
}

fn reply() -> Reply {
    Reply {
        node: NodeId::new(23).unwrap(),
        original: LogPos {
            term: 8,
            leader: 3,
            index: 91,
        },
        // Opaque test payload, never an issuance artifact.
        leaf: vec![0x30, 0x01, 0x00],
    }
}

impl Backend for Synthetic {
    async fn anchor(&self, query: Query, deadline: Instant) -> Result<BoundVoters, Refused> {
        assert!(deadline <= Instant::now() + WORK_BUDGET);
        self.observation.anchors.fetch_add(1, SeqCst);
        if let Some(gate) = &self.anchor_work {
            gate.hold().await;
        }
        if query != self.query {
            return Err(Refused);
        }
        Ok(BoundVoters::decode(&self.list).unwrap())
    }

    async fn redeem(&self, request: Redeem, deadline: Instant) -> Result<Reply, Refused> {
        assert!(deadline <= Instant::now() + WORK_BUDGET);
        let accepted = !self.refuse_redeem && ProvenCsr::parse(&request.csr).is_ok();
        self.observation.redeems.lock().unwrap().push(request);
        if let Some(gate) = &self.redeem_work {
            gate.hold().await;
        }
        accepted.then(reply).ok_or(Refused)
    }
}

fn synthetic(f: &Fixture) -> Synthetic {
    Synthetic {
        query: Query {
            cluster: f.code.cluster(),
            code: f.code.code_id(),
        },
        list: f.list.clone(),
        observation: Arc::new(Observed::default()),
        anchor_work: None,
        redeem_work: None,
        refuse_redeem: false,
    }
}

fn private_key(key: &KeyPair) -> PrivateKeyDer<'static> {
    PrivatePkcs8KeyDer::from(key.serialize_der()).into()
}

async fn listener() -> (TcpListener, std::net::SocketAddr) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    (listener, addr)
}

async fn start(f: &Fixture, backend: Synthetic) -> (ListenerOwner, std::net::SocketAddr) {
    let (listener, addr) = listener().await;
    (
        ListenerOwner::start(
            listener,
            f.good.clone().into(),
            private_key(&f.good_key),
            Arc::new(ProbeTime),
            backend,
        )
        .unwrap(),
        addr,
    )
}

fn signed_csr(maximum: bool) -> Vec<u8> {
    let key = KeyPair::generate_for(&PKCS_ED25519).unwrap();
    let encode = |padding| {
        let mut params = CertificateParams::default();
        let uri = format!("https://example.invalid/{}", "a".repeat(padding));
        params
            .subject_alt_names
            .push(SanType::URI(uri.try_into().unwrap()));
        params.serialize_request(&key).unwrap().der().to_vec()
    };
    let csr = if maximum {
        let start = 3500;
        let overhead = encode(start).len() - start;
        let csr = encode(4096 - overhead);
        assert_eq!(csr.len(), 4096);
        csr
    } else {
        encode(4)
    };
    ProvenCsr::parse(&csr).unwrap();
    csr
}

fn test_join_code(f: &Fixture) -> JoinCode {
    JoinCode::generate(
        f.code.cluster(),
        Sha256::digest(&f.root).into(),
        BoundVoters::decode(&f.list)
            .unwrap()
            .anchor(f.code.cluster()),
        |bytes| {
            bytes.fill(91);
            Ok::<_, ()>(())
        },
    )
    .unwrap()
}

// These hand-authored frames intentionally do not use the production codec.
fn wire(kind: u8, body: &[u8]) -> Vec<u8> {
    let mut bytes = b"ARXE\x01".to_vec();
    bytes.push(kind);
    bytes.extend((body.len() as u16).to_be_bytes());
    bytes.extend(body);
    bytes
}

fn query_wire(code: &JoinCode) -> Vec<u8> {
    wire(1, &[code.cluster(), code.code_id()].concat())
}

fn anchor_wire(list: &[u8]) -> Vec<u8> {
    wire(2, &[&[0], &[9; 16][..], list].concat())
}

fn redeem_wire(code: &JoinCode, challenge: &[u8], csr: &[u8]) -> Vec<u8> {
    wire(
        3,
        &[
            &REQUEST,
            &code.cluster()[..],
            &code.code_id(),
            code.anchor().as_bytes(),
            challenge,
            code.proof().bytes(),
            &(csr.len() as u16).to_be_bytes(),
            csr,
        ]
        .concat(),
    )
}

fn result_wire() -> Vec<u8> {
    // Status, node, original term, leader, index, DER length and payload.
    wire(
        4,
        &[
            0, 0, 0, 0, 0, 0, 0, 0, 23, 0, 0, 0, 0, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 0,
            0, 0, 0, 91, 0, 3, 0x30, 0x01, 0x00,
        ],
    )
}

fn raw_client(f: &Fixture) -> TlsConnector {
    // The raw peer speaks the wire independently. The fixture leaf also needs
    // the bootstrap URI check, so use the real verifier for its TLS handshake.
    TlsConnector::from(Arc::new(crate::cluster::pki::preproof::test_config(
        &f.code,
        &f.root,
        Arc::new(ProbeTime),
    )))
}

async fn raw_connect(connector: &TlsConnector, addr: std::net::SocketAddr) -> ClientTls<TcpStream> {
    let tcp = TcpStream::connect(addr).await.unwrap();
    tcp.set_nodelay(true).unwrap();
    let tls = connector
        .connect(ServerName::try_from(SERVER_NAME).unwrap(), tcp)
        .await
        .unwrap();
    assert_eq!(tls.get_ref().1.handshake_kind(), Some(HandshakeKind::Full));
    assert_eq!(
        tls.get_ref().1.protocol_version(),
        Some(rustls::ProtocolVersion::TLSv1_3)
    );
    tls
}

async fn query_raw(tls: &mut ClientTls<TcpStream>, f: &Fixture) -> [u8; 16] {
    tls.write_all(&query_wire(&f.code)).await.unwrap();
    tls.flush().await.unwrap();
    let mut anchor = vec![0; 25 + f.list.len()];
    tls.read_exact(&mut anchor).await.unwrap();
    assert_eq!(&anchor[..9], &anchor_wire(&f.list)[..9]);
    assert_eq!(&anchor[25..], f.list);
    anchor[9..25].try_into().unwrap()
}

async fn assert_no_reply(tls: &mut (impl AsyncRead + Unpin)) {
    match tls.read(&mut [0]).await {
        Ok(0) | Err(_) => {}
        Ok(_) => panic!("refused exchange returned plaintext"),
    }
}

async fn assert_released(owner: &ListenerOwner) {
    while owner.control.sessions.available_permits() != SESSIONS
        || owner.control.handshakes.available_permits() != HANDSHAKES
    {
        tokio::task::yield_now().await;
    }
}

async fn assert_listener_refused(addr: std::net::SocketAddr) {
    // Windows can take about two seconds to report a closed-port refusal.
    let error = timeout(REFUSAL_DEADLINE, TcpStream::connect(addr))
        .await
        .expect("closed listener connection deadline")
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::ConnectionRefused);
}

#[tokio::test]
async fn one_connection_retains_canonical_list_and_dispatches_max_csr_once() {
    within(async {
        let mut f = fixture();
        f.list[0] = 16;
        for node in 2u64..=16 {
            f.list.extend(node.to_be_bytes());
            f.list.extend([node as u8; 32]);
        }
        assert_eq!(f.list.len(), 641);
        f.code = test_join_code(&f);
        let csr = signed_csr(true);
        let backend = synthetic(&f);
        let observed = backend.observation.clone();
        let (mut owner, addr) = start(&f, backend).await;
        let limits = SocketLimits {
            chunk: 31,
            ..SocketLimits::default()
        };
        let completed = exchange(
            addr,
            &f.code,
            &f.root,
            Arc::new(ProbeTime),
            REQUEST,
            &csr,
            limits,
        )
        .await
        .unwrap();
        assert_eq!(completed.voters().encode(), f.list);
        let expected = reply();
        assert_eq!(completed.reply().node, expected.node);
        assert_eq!(completed.reply().original, expected.original);
        assert_eq!(completed.reply().leaf, expected.leaf);
        assert_eq!(owner.close(WATCHDOG).await, Close::Joined);
        assert_eq!(observed.anchors.load(SeqCst), 1);
        let reading = observed.redeems.lock().unwrap();
        let [seen] = &reading[..] else {
            panic!("expected exactly one dispatch")
        };
        assert_eq!(seen.request, REQUEST);
        assert!(
            seen.query
                == Query {
                    cluster: f.code.cluster(),
                    code: f.code.code_id()
                }
        );
        assert_eq!(seen.anchor, f.code.anchor());
        assert!(seen.proof == *f.code.proof().bytes());
        assert!(seen.csr == csr);
    })
    .await;
}

#[tokio::test]
async fn same_endpoint_reconnect_cannot_reuse_captured_redeem() {
    within(async {
        let f = fixture();
        let backend = synthetic(&f);
        let observed = backend.observation.clone();
        let (mut owner, addr) = start(&f, backend).await;
        let connector = raw_client(&f);
        let mut first = raw_connect(&connector, addr).await;
        let original = query_raw(&mut first, &f).await;
        let csr = signed_csr(false);
        let captured = redeem_wire(&f.code, &original, &csr);
        first.write_all(&captured).await.unwrap();
        first.shutdown().await.unwrap();
        let mut received = Vec::new();
        first.read_to_end(&mut received).await.unwrap();
        assert_eq!(received, result_wire());

        let mut no_query = raw_connect(&connector, addr).await;
        no_query.write_all(&captured).await.unwrap();
        no_query.flush().await.unwrap();
        assert_no_reply(&mut no_query).await;

        let mut new_query = raw_connect(&connector, addr).await;
        let fresh = query_raw(&mut new_query, &f).await;
        assert_ne!(original, fresh);
        new_query.write_all(&captured).await.unwrap();
        new_query.shutdown().await.unwrap();
        assert_no_reply(&mut new_query).await;
        assert_eq!(owner.close(WATCHDOG).await, Close::Joined);
        assert_eq!(observed.anchors.load(SeqCst), 2);
        assert_eq!(observed.redeems.lock().unwrap().len(), 1);
    })
    .await;
}

#[tokio::test]
async fn ingress_refuses_invalid_headers_and_bodies_before_dispatch() {
    within(async {
        let f = fixture();
        let backend = synthetic(&f);
        let observed = backend.observation.clone();
        let (mut owner, addr) = start(&f, backend).await;
        let connector = raw_client(&f);
        let csr = signed_csr(false);
        let query = query_wire(&f.code);
        for bytes in [
            wire(255, &query[8..]),
            wire(2, &query[8..]),
            b"ARXE\x02\x01\x00\x20".to_vec(),
            b"ARXE\x01\x01\xff\xff".to_vec(),
            query[..7].to_vec(),
            query[..39].to_vec(),
        ] {
            let mut tls = raw_connect(&connector, addr).await;
            tls.write_all(&bytes).await.unwrap();
            let _ = tls.shutdown().await;
            assert_no_reply(&mut tls).await;
        }
        assert_eq!(observed.anchors.load(SeqCst), 0);

        enum Mutation {
            Oversize,
            Truncated,
            Cluster,
            Code,
            Anchor,
            Challenge,
            ZeroRequest,
            WrongCsrLength,
            TrailingBody,
            DuplicateQuery,
            DuplicateRedeem,
            Trailing,
            Unclean,
        }
        use Mutation::*;
        for mutation in [
            Oversize,
            Truncated,
            Cluster,
            Code,
            Anchor,
            Challenge,
            ZeroRequest,
            WrongCsrLength,
            TrailingBody,
            DuplicateQuery,
            DuplicateRedeem,
            Trailing,
            Unclean,
        ] {
            let mut tls = raw_connect(&connector, addr).await;
            let challenge = query_raw(&mut tls, &f).await;
            let mut redeem = redeem_wire(&f.code, &challenge, &csr);
            match mutation {
                Oversize => redeem = b"ARXE\x01\x03\x10\x83".to_vec(), // 4227
                Truncated => {
                    redeem.pop();
                }
                Cluster => redeem[24] ^= 1,
                Code => redeem[40] ^= 1,
                Anchor => redeem[56] ^= 1,
                Challenge => redeem[88] ^= 1,
                ZeroRequest => redeem[8..24].fill(0),
                WrongCsrLength => redeem[136..138].fill(0),
                TrailingBody => {
                    let new_len = (redeem.len() - 7) as u16;
                    redeem[6..8].copy_from_slice(&new_len.to_be_bytes());
                    redeem.push(0);
                }
                DuplicateQuery => redeem = query.clone(),
                DuplicateRedeem => redeem.extend_from_within(..),
                Trailing => redeem.push(0),
                Unclean => {}
            }
            tls.write_all(&redeem).await.unwrap();
            if matches!(mutation, Unclean) {
                tls.flush().await.unwrap();
                tls.get_mut().0.shutdown().await.unwrap();
            } else {
                let _ = tls.shutdown().await;
            }
            assert_no_reply(&mut tls).await;
        }
        assert_eq!(owner.close(WATCHDOG).await, Close::Joined);
        assert!(observed.redeems.lock().unwrap().is_empty());
    })
    .await;
}

fn test_server_config(f: &Fixture) -> ServerConfig {
    server_config(
        f.good.clone().into(),
        private_key(&f.good_key),
        Arc::new(ProbeTime),
    )
    .unwrap()
}

async fn raw_server<F>(
    config: ServerConfig,
    serve: impl FnOnce(TlsStream<TcpStream>) -> F + Send + 'static,
) -> (std::net::SocketAddr, JoinHandle<()>)
where
    F: Future<Output = ()> + Send + 'static,
{
    let (listener, addr) = listener().await;
    let handle = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        socket.set_nodelay(true).unwrap();
        if let Ok(tls) = TlsAcceptor::from(Arc::new(config)).accept(socket).await {
            assert_eq!(tls.get_ref().1.handshake_kind(), Some(HandshakeKind::Full));
            assert!(tls.get_ref().1.peer_certificates().is_none());
            serve(tls).await;
        }
    });
    (addr, handle)
}

async fn read_plaintext(tls: &mut TlsStream<TcpStream>, buffer: &mut Vec<u8>) {
    // Observation remains accurate even when the production client drops TLS.
    let _ = tls.read_to_end(buffer).await;
}

#[tokio::test]
async fn client_refuses_wrong_identity_and_wrong_method_with_zero_proof() {
    within(async {
        let csr = signed_csr(false);
        let f = fixture();
        let wrong_root = fixture().root;
        let (unused, addr) = listener().await;
        let run = |addr, f: &Fixture, root: Vec<u8>| {
            let csr = csr.clone();
            let code = test_join_code(f);
            async move {
                exchange(
                    addr,
                    &code,
                    &root,
                    Arc::new(ProbeTime),
                    REQUEST,
                    &csr,
                    SocketLimits::default(),
                )
                .await
            }
        };
        assert!(matches!(
            run(addr, &f, wrong_root).await,
            Err(ExchangeError::Refused)
        ));
        assert!(timeout(Duration::ZERO, unused.accept()).await.is_err());
        let wrong_cluster = fixture_identity([5; 16], 1);
        let attacker = || {
            server_config(
                f.attacker.clone().into(),
                private_key(&f.bad_key),
                Arc::new(ProbeTime),
            )
            .unwrap()
        };
        let good_anchor = anchor_wire(&f.list);
        for (fixture, config, message, expected_query) in [
            (
                &wrong_cluster,
                test_server_config(&wrong_cluster),
                good_anchor.clone(),
                false,
            ),
            (&f, wrong_signer_config(&f), good_anchor.clone(), false),
            (&f, attacker(), good_anchor, true),
            (&f, attacker(), anchor_wire(&f.bad_list), true),
            (&f, test_server_config(&f), result_wire(), true),
            (
                &f,
                test_server_config(&f),
                b"ARXE\x01\x02\xff\xff".to_vec(),
                true,
            ),
            (&f, test_server_config(&f), wire(2, &[1]), true),
        ] {
            let observed = Arc::new(AtomicUsize::new(0));
            let peer = observed.clone();
            let query = query_wire(&fixture.code);
            let (addr, done) = raw_server(config, move |mut tls| async move {
                let mut seen = Vec::new();
                if expected_query {
                    let mut body = [0; 40];
                    tls.read_exact(&mut body).await.unwrap();
                    assert_eq!(body, query.as_slice());
                    tls.write_all(&message).await.unwrap();
                    tls.flush().await.unwrap();
                }
                read_plaintext(&mut tls, &mut seen).await;
                peer.fetch_add(seen.len(), SeqCst);
            })
            .await;
            assert!(matches!(
                run(addr, fixture, fixture.root.clone()).await,
                Err(ExchangeError::Refused)
            ));
            done.await.unwrap();
            assert_eq!(observed.load(SeqCst), 0, "unauthorized plaintext");
        }
    })
    .await;
}

#[tokio::test]
async fn client_refuses_frame_buffered_after_anchor_without_releasing_redeem() {
    within(async {
        let f = fixture();
        let csr = signed_csr(false);
        let anchor = anchor_wire(&f.list);
        // One extra frame per run: a duplicate Anchor, then a wrong-kind Result.
        for extra in [anchor.clone(), result_wire()] {
            let query = query_wire(&f.code);
            // A single write puts both frames in one TLS record, so the extra
            // plaintext is decrypted alongside the Anchor the client consumes.
            let message = [anchor.as_slice(), &extra].concat();
            let observed = Arc::new(AtomicUsize::new(usize::MAX));
            let peer = observed.clone();
            let (addr, done) = raw_server(test_server_config(&f), move |mut tls| async move {
                let mut bytes = [0; 40];
                tls.read_exact(&mut bytes).await.unwrap();
                assert_eq!(bytes, query.as_slice());
                tls.write_all(&message).await.unwrap();
                tls.flush().await.unwrap();
                let mut seen = Vec::new();
                read_plaintext(&mut tls, &mut seen).await;
                peer.store(seen.len(), SeqCst);
            })
            .await;
            let result = exchange(
                addr,
                &f.code,
                &f.root,
                Arc::new(ProbeTime),
                REQUEST,
                &csr,
                SocketLimits::default(),
            )
            .await;
            done.await.unwrap();
            assert!(matches!(result, Err(ExchangeError::Refused)));
            assert_eq!(observed.load(SeqCst), 0, "Redeem released past extra frame");
        }
    })
    .await;
}

#[tokio::test]
async fn client_accepts_only_the_exact_result_and_authenticated_end() {
    within(async {
        let f = fixture();
        let csr = signed_csr(false);
        let good = result_wire();
        let mut max_body = good[8..43].to_vec();
        max_body[33..35].copy_from_slice(&4096u16.to_be_bytes());
        max_body.resize(4131, 0x5a);
        let mut invalid_node = good.clone();
        invalid_node[16] = 1;
        let mut invalid_index = good.clone();
        invalid_index[33] = 0x80;
        let mut invalid_der = good.clone();
        invalid_der[42] = 4;
        let mut trailing = good.clone();
        trailing.extend_from_slice(&good);
        for (message, clean, accepted) in [
            (good.clone(), true, true),
            (wire(4, &max_body), true, true),
            (good.clone(), false, false),
            (trailing, true, false),
            (invalid_node, true, false),
            (invalid_index, true, false),
            (invalid_der, true, false),
            (b"ARXE\x01\x04\xff\xff".to_vec(), true, false),
            (good[..7].to_vec(), true, false),
            (good[..good.len() - 1].to_vec(), true, false),
            (wire(4, &[1]), true, false),
            (Vec::new(), false, false),
        ] {
            let query = query_wire(&f.code);
            let anchor = anchor_wire(&f.list);
            let redeem = redeem_wire(&f.code, &[9; 16], &csr);
            let (addr, done) = raw_server(test_server_config(&f), move |mut tls| async move {
                let mut bytes = [0; 40];
                tls.read_exact(&mut bytes).await.unwrap();
                assert_eq!(bytes, query.as_slice());
                tls.write_all(&anchor).await.unwrap();
                tls.flush().await.unwrap();
                let mut observed = Vec::new();
                tls.read_to_end(&mut observed).await.unwrap();
                assert!(
                    observed == redeem,
                    "peer must receive one exact proof and CSR"
                );
                tls.write_all(&message).await.unwrap();
                tls.flush().await.unwrap();
                if clean {
                    let _ = tls.shutdown().await;
                }
            })
            .await;
            let result = exchange(
                addr,
                &f.code,
                &f.root,
                Arc::new(ProbeTime),
                REQUEST,
                &csr,
                SocketLimits::default(),
            )
            .await;
            done.await.unwrap();
            if accepted {
                assert_eq!(result.unwrap().reply().original, reply().original);
            } else {
                assert!(matches!(result, Err(ExchangeError::PossibleEffect)));
            }
        }

        let gate = Gate::new();
        let mut backend = synthetic(&f);
        backend.redeem_work = Some(gate.clone());
        let observed = backend.observation.clone();
        let (mut owner, destination) = start(&f, backend).await;
        let (relay, addr) = listener().await;
        let disconnect_on_admission = async {
            let (mut client, _) = relay.accept().await.unwrap();
            let mut server = TcpStream::connect(destination).await.unwrap();
            client.set_nodelay(true).unwrap();
            server.set_nodelay(true).unwrap();
            // Forward opaque TLS until Redeem is admitted. Dropping the relay
            // sockets while work is held prevents any Result from arriving.
            tokio::select! {
                admitted = gate.arrived.acquire() => admitted.unwrap().forget(),
                _ = tokio::io::copy_bidirectional(&mut client, &mut server) => {
                    panic!("relay closed before Redeem admission");
                }
            }
        };
        let client = exchange(
            addr,
            &f.code,
            &f.root,
            Arc::new(ProbeTime),
            REQUEST,
            &csr,
            SocketLimits::default(),
        );
        let (result, ()) = tokio::join!(client, disconnect_on_admission);
        assert!(matches!(result, Err(ExchangeError::PossibleEffect)));
        let retry = relay.accept();
        tokio::pin!(retry);
        assert!(
            futures_util::poll!(retry).is_pending(),
            "enrollment reconnected"
        );
        assert_eq!(gate.resource.available_permits(), 0);

        gate.release.add_permits(1);
        assert_eq!(owner.close(WATCHDOG).await, Close::Joined);
        assert_eq!(gate.resource.available_permits(), 1);
        assert_released(&owner).await;
        assert_eq!(observed.anchors.load(SeqCst), 1);
        let requests = observed.redeems.lock().unwrap();
        let [request] = &requests[..] else {
            panic!("expected exactly one admitted Redeem");
        };
        assert_eq!(request.request, REQUEST);
        assert!(request.csr == csr);
    })
    .await;
}

pub(super) struct WriteFault {
    armed: AtomicBool,
    delivered: AtomicUsize,
    maximum: usize,
    failed: AtomicBool,
}

impl WriteFault {
    pub(super) fn limit(&self) -> io::Result<usize> {
        if !self.armed.load(SeqCst) {
            return Ok(usize::MAX);
        }
        let remaining = self.maximum - self.delivered.load(SeqCst);
        if remaining == 0 {
            self.failed.store(true, SeqCst);
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "injected write failure",
            ));
        }
        Ok(remaining)
    }

    pub(super) fn wrote(&self, bytes: usize) {
        if self.armed.load(SeqCst) {
            self.delivered.fetch_add(bytes, SeqCst);
        }
    }
}

#[tokio::test]
async fn failed_write_after_real_ciphertext_prefix_never_dispatches() {
    within(async {
        let f = fixture();
        let gate = Gate::new();
        let mut backend = synthetic(&f);
        backend.anchor_work = Some(gate.clone());
        let observed = backend.observation.clone();
        let (mut owner, destination) = start(&f, backend).await;

        // The relay forwards opaque TLS and counts bytes independently at the
        // peer socket after the backend has received Query.
        let (relay, addr) = listener().await;
        let fault = Arc::new(WriteFault {
            armed: AtomicBool::new(false),
            delivered: AtomicUsize::new(0),
            maximum: 37,
            failed: AtomicBool::new(false),
        });
        let peer_fault = fault.clone();
        let done = tokio::spawn(async move {
            let (client, _) = relay.accept().await.unwrap();
            let server = TcpStream::connect(destination).await.unwrap();
            client.set_nodelay(true).unwrap();
            server.set_nodelay(true).unwrap();
            let (mut client_read, mut client_write) = client.into_split();
            let (mut server_read, mut server_write) = server.into_split();
            let down = tokio::io::copy(&mut server_read, &mut client_write);
            let up = async {
                let mut seen = 0;
                let mut bytes = [0; 4096];
                loop {
                    let n = client_read.read(&mut bytes).await.unwrap();
                    if n == 0 {
                        break;
                    }
                    if peer_fault.armed.load(SeqCst) {
                        seen += n;
                    }
                    server_write.write_all(&bytes[..n]).await.unwrap();
                }
                server_write.shutdown().await.unwrap();
                seen
            };
            let (_, prefix) = tokio::join!(down, up);
            prefix
        });
        let limits = SocketLimits {
            write_fault: Some(fault.clone()),
            ..SocketLimits::default()
        };
        let csr = signed_csr(true);
        let release = async {
            gate.arrived.acquire().await.unwrap().forget();
            fault.armed.store(true, SeqCst);
            gate.release.add_permits(1);
        };
        let client = exchange(
            addr,
            &f.code,
            &f.root,
            Arc::new(ProbeTime),
            REQUEST,
            &csr,
            limits,
        );
        let (result, ()) = tokio::join!(client, release);
        assert!(matches!(result, Err(ExchangeError::PossibleEffect)));
        assert!(fault.failed.load(SeqCst));
        assert_eq!(done.await.unwrap(), 37);
        assert_released(&owner).await;
        assert_eq!(owner.close(WATCHDOG).await, Close::Joined);
        assert!(observed.redeems.lock().unwrap().is_empty());
        assert_eq!(observed.anchors.load(SeqCst), 1);
    })
    .await;
}

#[tokio::test]
async fn ciphertext_caps_count_cumulative_reads_and_writes_on_real_sockets() {
    within(async {
        // The loopback peer sees every admitted write and supplies enough real
        // ciphertext bytes to exhaust the read cap, independent of frame sizes.
        let (listener, addr) = listener().await;
        let sent = vec![5; CIPHERTEXT + 1];
        let expected = sent.clone();
        let peer = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut read = Vec::new();
            let (mut rx, mut tx) = socket.split();
            let (incoming, outgoing) =
                tokio::join!(rx.read_to_end(&mut read), tx.write_all(&sent),);
            incoming.unwrap();
            outgoing.unwrap();
            assert_eq!(read, sent[..CIPHERTEXT]);
        });
        let mut client = QuotaSocket::new(
            TcpStream::connect(addr).await.unwrap(),
            SocketLimits {
                chunk: 1021,
                ..SocketLimits::default()
            },
            Instant::now() + SOCKET_BUDGET,
        );
        let (mut rx, mut tx) = tokio::io::split(&mut client);
        let mut received = Vec::new();
        let read = async { rx.read_to_end(&mut received).await };
        let write = async {
            let err = tx.write_all(&expected).await.unwrap_err();
            for _ in 0..2 {
                assert!(tx.write_all(&[1]).await.is_err());
            }
            tx.shutdown().await.unwrap();
            err
        };
        let (err_read, err_write) = tokio::join!(read, write);
        assert_eq!(
            err_read.unwrap_err().to_string(),
            "enrollment ciphertext read cap"
        );
        assert_eq!(err_write.to_string(), "enrollment ciphertext write cap");
        assert!(rx.read(&mut [0]).await.is_err());
        assert_eq!(received, expected[..CIPHERTEXT]);
        peer.await.unwrap();

        // Small residual caps force the same boundary to fail inside real TLS.
        let f = fixture();
        let backend = synthetic(&f);
        let observed = backend.observation.clone();
        let (mut owner, addr) = start(&f, backend).await;
        for limits in [
            SocketLimits {
                incoming: 100,
                ..SocketLimits::default()
            },
            SocketLimits {
                outgoing: 50,
                ..SocketLimits::default()
            },
        ] {
            assert!(matches!(
                exchange(
                    addr,
                    &f.code,
                    &f.root,
                    Arc::new(ProbeTime),
                    REQUEST,
                    &[1],
                    limits
                )
                .await,
                Err(ExchangeError::Refused),
            ));
            assert_released(&owner).await;
        }
        assert_eq!(owner.close(WATCHDOG).await, Close::Joined);
        assert_eq!(observed.anchors.load(SeqCst), 0);
        assert!(observed.redeems.lock().unwrap().is_empty());
    })
    .await;
}

#[tokio::test]
async fn saturation_refuses_without_queue_and_joins_release_capacity() {
    let addr = within(async {
        let f = fixture();
        let (mut owner, addr) = start(&f, synthetic(&f)).await;
        let held_handshakes = [
            TcpStream::connect(addr).await.unwrap(),
            TcpStream::connect(addr).await.unwrap(),
        ];
        while owner.control.handshakes.available_permits() != 0 {
            tokio::task::yield_now().await;
        }
        let mut refused = TcpStream::connect(addr).await.unwrap();
        assert_no_reply(&mut refused).await;
        drop(held_handshakes);
        assert_released(&owner).await;

        let connector = raw_client(&f);
        let mut held_sessions = Vec::new();
        for _ in 0..SESSIONS {
            held_sessions.push(raw_connect(&connector, addr).await);
            while owner.control.handshakes.available_permits() != HANDSHAKES {
                tokio::task::yield_now().await;
            }
        }
        assert_eq!(owner.control.sessions.available_permits(), 0);
        let mut refused = TcpStream::connect(addr).await.unwrap();
        assert_no_reply(&mut refused).await;
        drop(held_sessions.pop());
        while owner.control.sessions.available_permits() != 1 {
            tokio::task::yield_now().await;
        }
        held_sessions.push(raw_connect(&connector, addr).await);
        assert_eq!(owner.close(WATCHDOG).await, Close::Joined);
        assert_released(&owner).await;
        addr
    })
    .await;
    assert_listener_refused(addr).await;
}

#[tokio::test]
async fn close_and_socket_expiry_retain_each_admitted_operation() {
    // Advance only after observing admission. Resuming before socket inspection
    // keeps network scheduling independent of the virtual deadline.
    for redeem in [false, true] {
        let f = fixture();
        let gate = Gate::new();
        let mut backend = synthetic(&f);
        let observed = backend.observation.clone();
        if redeem {
            backend.redeem_work = Some(gate.clone());
        } else {
            backend.anchor_work = Some(gate.clone());
        }
        let (mut owner, addr) = start(&f, backend).await;
        let connector = raw_client(&f);
        let mut tls = within(raw_connect(&connector, addr)).await;
        if redeem {
            let challenge = within(query_raw(&mut tls, &f)).await;
            let request = redeem_wire(&f.code, &challenge, &signed_csr(false));
            within(async {
                tls.write_all(&request).await.unwrap();
                tls.shutdown().await.unwrap();
            })
            .await;
        } else {
            within(async {
                tls.write_all(&query_wire(&f.code)).await.unwrap();
                tls.flush().await.unwrap();
            })
            .await;
        }
        within(gate.arrived.acquire()).await.unwrap().forget();
        {
            let abandoned = owner.close(WATCHDOG);
            tokio::pin!(abandoned);
            assert!(futures_util::poll!(&mut abandoned).is_pending());
        }
        assert_eq!(
            owner.close(Duration::from_millis(5)).await,
            Close::Unconfirmed
        );
        assert_eq!(gate.resource.available_permits(), 0);
        assert_eq!(owner.control.sessions.available_permits(), SESSIONS - 1);
        assert!(!owner.supervisor.is_finished());

        tokio::time::pause();
        tokio::time::advance(SOCKET_BUDGET).await;
        tokio::time::resume();
        within(assert_no_reply(&mut tls)).await;
        assert_eq!(gate.resource.available_permits(), 0);
        assert_eq!(owner.control.sessions.available_permits(), SESSIONS - 1);
        gate.release.add_permits(1);
        assert_eq!(owner.close(WATCHDOG).await, Close::Joined);
        assert_eq!(owner.close(Duration::ZERO).await, Close::Joined);
        assert_eq!(gate.resource.available_permits(), 1);
        within(assert_released(&owner)).await;
        assert_eq!(observed.anchors.load(SeqCst), 1);
        assert_eq!(observed.redeems.lock().unwrap().len(), usize::from(redeem));
    }
}

#[tokio::test]
async fn successful_close_joins_idle_connections_and_releases_listener() {
    let addr = within(async {
        let f = fixture();
        let backend = synthetic(&f);
        let observed = backend.observation.clone();
        let (mut owner, addr) = start(&f, backend).await;
        let mut tls = raw_connect(&raw_client(&f), addr).await;
        assert_eq!(owner.close(WATCHDOG).await, Close::Joined);
        assert_no_reply(&mut tls).await;
        assert_released(&owner).await;
        assert_eq!(observed.anchors.load(SeqCst), 0);
        assert!(observed.redeems.lock().unwrap().is_empty());
        addr
    })
    .await;
    assert_listener_refused(addr).await;
}

#[tokio::test]
async fn late_redeem_and_close_before_clean_end_do_not_admit_dispatch() {
    for close in [false, true] {
        let f = fixture();
        let backend = synthetic(&f);
        let observed = backend.observation.clone();
        let (mut owner, addr) = start(&f, backend).await;
        let mut tls = within(raw_connect(&raw_client(&f), addr)).await;
        let challenge = within(query_raw(&mut tls, &f)).await;
        let redeem = redeem_wire(&f.code, &challenge, &signed_csr(false));
        if close {
            within(async {
                // A complete TLS frame is still insufficient before close_notify.
                tls.write_all(&redeem).await.unwrap();
                tls.flush().await.unwrap();
            })
            .await;
            assert_eq!(owner.close(WATCHDOG).await, Close::Joined);
        } else {
            // The socket still has time to read, but the work budget no longer fits.
            tokio::time::pause();
            tokio::time::advance(SOCKET_BUDGET - WORK_BUDGET + Duration::from_secs(1)).await;
            tokio::time::resume();
            within(async {
                tls.write_all(&redeem).await.unwrap();
                tls.shutdown().await.unwrap();
                assert_no_reply(&mut tls).await;
            })
            .await;
            assert_eq!(owner.close(WATCHDOG).await, Close::Joined);
        }
        assert_eq!(observed.anchors.load(SeqCst), 1);
        assert!(observed.redeems.lock().unwrap().is_empty());
        within(assert_released(&owner)).await;
    }
}
