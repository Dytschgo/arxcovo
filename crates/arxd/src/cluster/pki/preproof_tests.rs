//! Self-owned adversary CA; no production issuance authority or registry stubs.
use super::preproof::{PreproofClient, Refused};
use crate::cluster::enrollment::{BoundVoters, JoinCode, KeyDigest, NodeId};
use rcgen::{CertificateParams, KeyPair, PublicKeyData};
use rustls::{
    ServerConfig, ServerConnection,
    crypto::ring,
    pki_types::{PrivatePkcs8KeyDer, UnixTime},
    server::{ClientHello, ResolvesServerCert},
    sign::CertifiedKey,
    time_provider::TimeProvider,
};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    sync::Arc,
    time::Duration,
};

#[derive(Debug)]
pub(in crate::cluster) struct ProbeTime;
impl TimeProvider for ProbeTime {
    fn current_time(&self) -> Option<UnixTime> {
        Some(UnixTime::since_unix_epoch(Duration::from_secs(1000)))
    }
}
pub(in crate::cluster) fn server(der: Vec<u8>, key: &KeyPair) -> ServerConnection {
    let mut config =
        ServerConfig::builder_with_details(Arc::new(ring::default_provider()), Arc::new(ProbeTime))
            .with_protocol_versions(&[&rustls::version::TLS13])
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                vec![der.into()],
                PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
            )
            .unwrap();
    config.send_tls13_tickets = 0;
    ServerConnection::new(Arc::new(config)).unwrap()
}
/// Whole-session counters; fixed buffers and at most32 rounds per exchange.
pub(in crate::cluster) struct Driver {
    incoming: usize,
    outgoing: usize,
}
impl Driver {
    pub(in crate::cluster) fn new() -> Self {
        Self {
            incoming: 0,
            outgoing: 0,
        }
    }
    pub(in crate::cluster) fn pump(
        &mut self,
        c: &mut PreproofClient,
        s: &mut ServerConnection,
    ) -> Result<(), Refused> {
        for _ in 0..32 {
            let mut bytes = [0; 4096];
            let n = c.drain(&mut bytes)?;
            let client_sent = n;
            self.outgoing = self.outgoing.checked_add(n).ok_or(Refused)?;
            if self.outgoing > 128 * 1024 {
                return Err(Refused);
            }
            let mut input = &bytes[..n];
            while !input.is_empty() {
                let n = s.read_tls(&mut input).map_err(|_| Refused)?;
                if n == 0 {
                    return Err(Refused);
                }
                s.process_new_packets().map_err(|_| Refused)?;
            }
            let n = s.write_tls(&mut &mut bytes[..]).map_err(|_| Refused)?;
            self.incoming = self.incoming.checked_add(n).ok_or(Refused)?;
            if self.incoming > 128 * 1024 {
                return Err(Refused);
            }
            let mut input = &bytes[..n];
            while !input.is_empty() {
                let n = c.feed(input)?;
                if n == 0 {
                    return Err(Refused);
                }
                input = &input[n..];
            }
            if client_sent == 0 && n == 0 && !s.wants_write() {
                return Ok(());
            }
        }
        Err(Refused)
    }
}
fn plaintext(s: &mut ServerConnection) -> Vec<u8> {
    let mut b = [0; 65];
    let n = match s.reader().read(&mut b) {
        Ok(n) => n,
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => 0,
        Err(_) => 0,
    };
    b[..n].to_vec()
}
pub(in crate::cluster) fn exchange(
    c: &mut PreproofClient,
    s: &mut ServerConnection,
    code: &JoinCode,
    list: &[u8],
) -> Result<[u8; 32], Refused> {
    let mut driver = Driver::new();
    driver.pump(c, s)?;
    assert!(s.peer_certificates().is_none());
    c.query()?;
    driver.pump(c, s)?;
    let query = plaintext(s);
    assert_eq!(query.len(), 32);
    assert_eq!(&query[..16], &code.cluster());
    assert_eq!(&query[16..], &code.code_id());
    s.writer().write_all(list).unwrap();
    driver.pump(c, s)?;
    let mut received = [0; 641];
    let n = c.read_anchor(&mut received)?;
    c.accept_anchor(&received[..n])?;
    c.release_proof(code)?;
    driver.pump(c, s)?;
    let proof = plaintext(s);
    assert_eq!(proof.len(), 32);
    Ok(proof.try_into().unwrap())
}
struct Fixture {
    root: Vec<u8>,
    good: Vec<u8>,
    attacker: Vec<u8>,
    good_key: KeyPair,
    bad_key: KeyPair,
    list: Vec<u8>,
    bad_list: Vec<u8>,
    code: JoinCode,
}
fn list(key: &KeyPair) -> Vec<u8> {
    let mut b = vec![1];
    b.extend(1u64.to_be_bytes());
    b.extend(
        KeyDigest::of_spki(&key.subject_public_key_info())
            .unwrap()
            .as_bytes(),
    );
    b
}
pub(super) fn test_client() -> PreproofClient {
    let f = fixture();
    PreproofClient::new(&f.code, &f.root, Arc::new(ProbeTime)).unwrap()
}
fn fixture() -> Fixture {
    fixture_identity([1; 16], 1)
}
fn fixture_identity(cluster: [u8; 16], node: u64) -> Fixture {
    let key = KeyPair::generate().unwrap();
    let mut ca = CertificateParams::default();
    ca.not_before = time::OffsetDateTime::from_unix_timestamp(0).unwrap();
    ca.not_after = time::OffsetDateTime::from_unix_timestamp(100000).unwrap();
    ca.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Constrained(0));
    ca.key_usages = vec![rcgen::KeyUsagePurpose::KeyCertSign];
    let root = ca.self_signed(&key).unwrap().der().to_vec();
    let issuer = rcgen::Issuer::new(ca, key);
    let good_key = KeyPair::generate().unwrap();
    let bad_key = KeyPair::generate().unwrap();
    let sign = |key: &KeyPair| {
        let mut p = CertificateParams::default();
        p.not_before = time::OffsetDateTime::from_unix_timestamp(0).unwrap();
        p.not_after = time::OffsetDateTime::from_unix_timestamp(100000).unwrap();
        p.key_usages = vec![rcgen::KeyUsagePurpose::DigitalSignature];
        p.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ServerAuth];
        p.subject_alt_names = vec![rcgen::SanType::URI(
            super::node_uri(cluster, NodeId::new(node).unwrap())
                .try_into()
                .unwrap(),
        )];
        p.signed_by(key, &issuer).unwrap().der().to_vec()
    };
    let good = sign(&good_key);
    let attacker = sign(&bad_key);
    let bad_list = list(&bad_key);
    let list = list(&good_key);
    let code = JoinCode::generate(
        [1; 16],
        Sha256::digest(&root).into(),
        BoundVoters::decode(&list).unwrap().anchor([1; 16]),
        |b| {
            b.fill(91);
            Ok::<_, ()>(())
        },
    )
    .unwrap();
    Fixture {
        root,
        good,
        attacker,
        good_key,
        bad_key,
        list,
        bad_list,
        code,
    }
}
#[test]
fn genuine_release_and_terminal_gate() {
    let f = fixture();
    let mut c = PreproofClient::new(&f.code, &f.root, Arc::new(ProbeTime)).unwrap();
    let mut s = server(f.good, &f.good_key);
    assert_eq!(
        &exchange(&mut c, &mut s, &f.code, &f.list).unwrap(),
        f.code.proof().bytes()
    );
    assert!(c.release_proof(&f.code).is_err());
    assert!(c.drain(&mut [0; 4096]).is_err());
    assert!(PreproofClient::new(&f.code, &[0; 4097], Arc::new(ProbeTime)).is_err());
    assert!(PreproofClient::new(&f.code, &fixture().root, Arc::new(ProbeTime)).is_err());
}
fn attack(f: Fixture, mut s: ServerConnection, list: Vec<u8>) {
    let mut c = PreproofClient::new(&f.code, &f.root, Arc::new(ProbeTime)).unwrap();
    let result = exchange(&mut c, &mut s, &f.code, &list);
    // A mutant must deliver actual proof to this server to break this assertion.
    let received_proof = if result.is_ok() {
        32
    } else {
        plaintext(&mut s).len()
    };
    assert_eq!(received_proof, 0, "server received proof bytes");
    assert!(result.is_err());
}
#[test]
fn adversary_anchor_digest_zero_proof() {
    let f = fixture();
    let s = server(f.attacker.clone(), &f.bad_key);
    let list = f.bad_list.clone();
    attack(f, s, list);
}
#[test]
fn adversary_endpoint_key_zero_proof() {
    let f = fixture();
    let s = server(f.attacker.clone(), &f.bad_key);
    let list = f.list.clone();
    attack(f, s, list);
}
struct WrongSigner(Arc<CertifiedKey>);
impl std::fmt::Debug for WrongSigner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WrongSigner")
    }
}
impl ResolvesServerCert for WrongSigner {
    fn resolve(&self, _: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        Some(self.0.clone())
    }
}
#[test]
fn adversary_certificate_verify_zero_proof() {
    super::preproof::SIGNATURE_CALLS.with(|n| n.set(0));
    let f = fixture();
    let provider = Arc::new(ring::default_provider());
    let key = provider
        .key_provider
        .load_private_key(PrivatePkcs8KeyDer::from(f.bad_key.serialize_der()).into())
        .unwrap();
    let certified = CertifiedKey::new(vec![f.good.clone().into()], key);
    let mut config = ServerConfig::builder_with_details(provider, Arc::new(ProbeTime))
        .with_protocol_versions(&[&rustls::version::TLS13])
        .unwrap()
        .with_no_client_auth()
        .with_cert_resolver(Arc::new(WrongSigner(Arc::new(certified))));
    config.send_tls13_tickets = 0;
    let s = ServerConnection::new(Arc::new(config)).unwrap();
    let list = f.list.clone();
    attack(f, s, list);
    super::preproof::SIGNATURE_CALLS
        .with(|n| assert_eq!(n.get(), 1, "reached actual possession verification"));
}

pub(super) fn prepared() -> (PreproofClient, ServerConnection, JoinCode) {
    let f = fixture();
    let mut c = PreproofClient::new(&f.code, &f.root, Arc::new(ProbeTime)).unwrap();
    let mut s = server(f.good, &f.good_key);
    let mut d = Driver::new();
    d.pump(&mut c, &mut s).unwrap();
    c.query().unwrap();
    d.pump(&mut c, &mut s).unwrap();
    assert_eq!(plaintext(&mut s).len(), 32);
    s.writer().write_all(&f.list).unwrap();
    d.pump(&mut c, &mut s).unwrap();
    let mut b = [0; 641];
    let n = c.read_anchor(&mut b).unwrap();
    c.accept_anchor(&b[..n]).unwrap();
    (c, s, f.code)
}
#[test]
fn gate_errors_are_terminal_and_wrong_identity_releases_zero() {
    for f in [fixture_identity([2; 16], 1), fixture_identity([1; 16], 2)] {
        let s = server(f.good.clone(), &f.good_key);
        let list = f.list.clone();
        attack(f, s, list);
    }
    let f = fixture();
    let mut c = test_client();
    assert!(c.release_proof(&f.code).is_err());
    assert!(c.drain(&mut [0; 4096]).is_err());
    let mut c = test_client();
    assert!(c.query().is_err());
    assert!(c.feed(&[]).is_err());
    let mut c = test_client();
    assert!(c.accept_anchor(&f.list).is_err());
    assert!(c.read_anchor(&mut [0; 641]).is_err());
    let (mut c, mut s, _) = prepared();
    assert!(c.accept_anchor(&[0; 642]).is_err());
    assert!(c.drain(&mut [0; 4096]).is_err());
    assert!(plaintext(&mut s).is_empty());
    let (mut c, mut s, _) = prepared();
    assert!(c.query().is_err());
    assert!(plaintext(&mut s).is_empty());
}
