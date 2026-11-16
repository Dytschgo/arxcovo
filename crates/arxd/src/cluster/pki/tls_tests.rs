use super::{ca::ClusterCa, csr::ProvenCsr, verify::*};
use crate::cluster::{
    consensus::issue::test_ticket,
    enrollment::{KeyDigest, NodeId},
};
use rcgen::{CertificateParams, KeyPair, PublicKeyData, SigningKey};
use rustls::{
    CertificateError, ClientConnection, Error, ProtocolVersion, ServerConnection,
    client::danger::ServerCertVerifier,
    pki_types::{CertificateDer, PrivatePkcs8KeyDer, ServerName, UnixTime},
    server::danger::ClientCertVerifier,
    time_provider::TimeProvider,
};
use std::{sync::Arc, time::Duration};
pub(in crate::cluster) const NOW: u64 = 1_000_000;
#[derive(Debug)]
pub(in crate::cluster) struct FixedTime(pub u64);
impl TimeProvider for FixedTime {
    fn current_time(&self) -> Option<UnixTime> {
        Some(UnixTime::since_unix_epoch(Duration::from_secs(self.0)))
    }
}
pub(in crate::cluster) fn node(n: u64) -> NodeId {
    NodeId::new(n).unwrap()
}
pub(in crate::cluster) fn leaf(ca: &ClusterCa, key: &KeyPair) -> Vec<u8> {
    let csr = CertificateParams::default().serialize_request(key).unwrap();
    let csr = ProvenCsr::parse(csr.der()).unwrap();
    ca.sign_leaf(&test_ticket(csr.digest()), &csr, NOW).unwrap()
}
pub(in crate::cluster) fn verifier(ca: &ClusterCa, registry: Registry) -> PeerVerifier {
    PeerVerifier::new(ca.cert_der().to_vec().into(), [1; 16], registry).unwrap()
}
pub(in crate::cluster) fn registry(key: &KeyPair) -> Registry {
    let digest = KeyDigest::of_spki(&key.subject_public_key_info()).unwrap();
    Arc::new(move |cluster, n, key| {
        if cluster != [1; 16] {
            Err(VerifyRefusal::WrongCluster)
        } else if n != node(4) {
            Err(VerifyRefusal::NotMember)
        } else if key != digest {
            Err(VerifyRefusal::KeyMismatch)
        } else {
            Ok(())
        }
    })
}
pub(in crate::cluster) fn connections(
    ca: &ClusterCa,
    der: &[u8],
    key: &KeyPair,
    registry: Registry,
    expected: NodeId,
    seconds: u64,
) -> (ClientConnection, ServerConnection) {
    let client = client_config(
        verifier(ca, registry.clone()).client(expected),
        der.to_vec().into(),
        PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
        Arc::new(FixedTime(seconds)),
    )
    .unwrap();
    assert!(!client.enable_sni);
    let server = server_config(
        verifier(ca, registry).server(),
        der.to_vec().into(),
        PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
        Arc::new(FixedTime(seconds)),
    )
    .unwrap();
    (
        ClientConnection::new(Arc::new(client), ServerName::try_from(SERVER_NAME).unwrap())
            .unwrap(),
        ServerConnection::new(Arc::new(server)).unwrap(),
    )
}
/// Call only on std threads / blocking pool. No sockets, at most 32 rounds.
fn complete(
    mut c: ClientConnection,
    mut s: ServerConnection,
) -> Result<(ClientConnection, ServerConnection), Error> {
    for _ in 0..32 {
        let mut bytes = Vec::new();
        c.write_tls(&mut bytes).unwrap();
        s.read_tls(&mut bytes.as_slice()).unwrap();
        s.process_new_packets()?;
        let mut bytes = Vec::new();
        s.write_tls(&mut bytes).unwrap();
        c.read_tls(&mut bytes.as_slice()).unwrap();
        c.process_new_packets()?;
        if !c.is_handshaking() && !s.is_handshaking() {
            assert_eq!(c.protocol_version(), Some(ProtocolVersion::TLSv1_3));
            assert_eq!(s.protocol_version(), Some(ProtocolVersion::TLSv1_3));
            return Ok((c, s));
        }
    }
    panic!("handshake exceeded 32 rounds")
}
pub(in crate::cluster) fn pump(
    c: ClientConnection,
    s: ServerConnection,
) -> Result<(AuthenticatedPeer, AuthenticatedPeer), Error> {
    let (c, s) = complete(c, s)?;
    Ok((
        AuthenticatedPeer::from_client(&c)?,
        AuthenticatedPeer::from_server(&s)?,
    ))
}
pub(in crate::cluster) fn refusal(e: Error) -> VerifyRefusal {
    let Error::InvalidCertificate(CertificateError::Other(e)) = e else {
        panic!("expected static refusal")
    };
    *e.0.downcast_ref::<VerifyRefusal>().unwrap()
}
fn direct(v: &PeerVerifier, der: &[u8], seconds: u64) -> Result<(), Error> {
    v.verify_server_cert(
        &CertificateDer::from(der),
        &[],
        &ServerName::try_from(SERVER_NAME).unwrap(),
        &[],
        UnixTime::since_unix_epoch(Duration::from_secs(seconds)),
    )
    .map(|_| ())
}
#[test]
fn mutual_tls_target_and_incomplete_identity() {
    std::thread::spawn(|| {
        let ca = ClusterCa::ephemeral([1; 16], NOW);
        let key = KeyPair::generate().unwrap();
        let der = leaf(&ca, &key);
        let (c, s) = connections(&ca, &der, &key, registry(&key), node(4), 1001);
        assert!(matches!(
            AuthenticatedPeer::from_client(&c),
            Err(VerifyRefusal::Incomplete)
        ));
        assert!(matches!(
            AuthenticatedPeer::from_server(&s),
            Err(VerifyRefusal::Incomplete)
        ));
        let (a, b) = pump(c, s).unwrap();
        assert_eq!(a.node(), node(4));
        assert_eq!(b.node(), node(4));
        let (c, s) = connections(&ca, &der, &key, registry(&key), node(5), 1001);
        assert_eq!(
            refusal(pump(c, s).err().unwrap()),
            VerifyRefusal::TargetMismatch
        );
    })
    .join()
    .unwrap();
}
#[test]
fn bounds_root_key_time_and_static_errors() {
    std::thread::spawn(|| {
        let ca = ClusterCa::ephemeral([1; 16], NOW);
        let key = KeyPair::generate().unwrap();
        let der = leaf(&ca, &key);
        let v = verifier(&ca, registry(&key)).client(node(4));
        assert_eq!(format!("{v:?}"), "PeerVerifier");
        let now = UnixTime::since_unix_epoch(Duration::from_secs(1001));
        let invalid = CertificateDer::from(vec![0; 4097]);
        assert_eq!(
            refusal(
                v.verify_client_cert(&invalid, &[der.clone().into()], now)
                    .err()
                    .unwrap()
            ),
            VerifyRefusal::Intermediates
        );
        assert_eq!(
            refusal(v.verify_client_cert(&invalid, &[], now).err().unwrap()),
            VerifyRefusal::Oversized
        );
        assert!(direct(&v, &[0], 1001).is_err());
        let foreign = ClusterCa::ephemeral([1; 16], NOW);
        assert!(matches!(
            direct(&v, &leaf(&foreign, &key), 1001),
            Err(Error::InvalidCertificate(
                CertificateError::UnknownIssuer | CertificateError::BadSignature
            ))
        ));
        let wrong = KeyPair::generate().unwrap();
        assert_eq!(
            refusal(direct(&v, &leaf(&ca, &wrong), 1001).unwrap_err()),
            VerifyRefusal::KeyMismatch
        );
        for seconds in [940, 940 + 30 * 86400] {
            direct(&v, &der, seconds).unwrap();
            let (c, s) = connections(&ca, &der, &key, registry(&key), node(4), seconds);
            pump(c, s).unwrap();
        }
        for seconds in [939, 941 + 30 * 86400] {
            assert_time_error(direct(&v, &der, seconds).unwrap_err(), seconds == 939);
            let (c, s) = connections(&ca, &der, &key, registry(&key), node(4), seconds);
            assert_time_error(pump(c, s).err().unwrap(), seconds == 939);
        }

        let server = server_config(
            verifier(&ca, registry(&key)).server(),
            der.clone().into(),
            PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
            Arc::new(FixedTime(1001)),
        )
        .unwrap();

        // Build a client without a certificate while retaining the real verifier.
        let client = rustls::ClientConfig::builder_with_details(
            Arc::new(rustls::crypto::ring::default_provider()),
            Arc::new(FixedTime(1001)),
        )
        .with_protocol_versions(&[&rustls::version::TLS13])
        .unwrap()
        .dangerous()
        .with_custom_certificate_verifier(v.clone())
        .with_no_client_auth();
        let c = ClientConnection::new(Arc::new(client), ServerName::try_from(SERVER_NAME).unwrap())
            .unwrap();
        assert!(matches!(
            pump(c, ServerConnection::new(Arc::new(server)).unwrap()),
            Err(Error::NoCertificatesPresented)
        ));
        assert!(v.offer_client_auth() && v.client_auth_mandatory());
        for r in [
            VerifyRefusal::Unavailable,
            VerifyRefusal::Malformed,
            VerifyRefusal::TargetMismatch,
        ] {
            assert!(!format!("{r:?} {r}").contains("arxcovo://"));
        }
    })
    .join()
    .unwrap();
}
#[test]
fn tls_signature_checks_both_sides_and_tls12_refusal() {
    use rustls::internal::msgs::codec::{Codec, Reader};
    let ca = ClusterCa::ephemeral([1; 16], NOW);
    let key = KeyPair::generate().unwrap();
    let der = CertificateDer::from(leaf(&ca, &key));
    let v = verifier(&ca, registry(&key));
    let message = b"TLS handshake signature test";
    let signature = key.sign(message).unwrap();
    let mut encoded = vec![4, 3];
    encoded.extend_from_slice(&(signature.len() as u16).to_be_bytes());
    encoded.extend_from_slice(&signature);
    let good = rustls::DigitallySignedStruct::read(&mut Reader::init(&encoded)).unwrap();
    ServerCertVerifier::verify_tls13_signature(&v, message, &der, &good).unwrap();
    ClientCertVerifier::verify_tls13_signature(&v, message, &der, &good).unwrap();
    assert!(ServerCertVerifier::verify_tls13_signature(&v, b"tampered", &der, &good).is_err());
    assert!(ClientCertVerifier::verify_tls13_signature(&v, b"tampered", &der, &good).is_err());
    assert_eq!(
        refusal(
            ServerCertVerifier::verify_tls12_signature(&v, message, &der, &good)
                .err()
                .unwrap()
        ),
        VerifyRefusal::Tls12
    );
    assert_eq!(
        refusal(
            ClientCertVerifier::verify_tls12_signature(&v, message, &der, &good)
                .err()
                .unwrap()
        ),
        VerifyRefusal::Tls12
    );
}
#[test]
fn strict_san_der_and_self_signed() {
    use rcgen::{BasicConstraints, IsCa, Issuer, SanType};
    let rootkey = KeyPair::generate().unwrap();
    let mut root = CertificateParams::default();
    root.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
    let rootcert = root.self_signed(&rootkey).unwrap();
    let issuer = Issuer::new(root, rootkey);
    let key = KeyPair::generate().unwrap();
    let uri = super::node_uri([1; 16], node(4));
    let cases = vec![
        vec![],
        vec![
            SanType::URI(uri.clone().try_into().unwrap()),
            SanType::URI(uri.clone().try_into().unwrap()),
        ],
        vec![SanType::DnsName("private.invalid".try_into().unwrap())],
        vec![SanType::URI(
            "arxcovo://cluster/01010101010101010101010101010101/node/04"
                .try_into()
                .unwrap(),
        )],
        vec![SanType::URI(
            super::node_uri([2; 16], node(4)).try_into().unwrap(),
        )],
    ];
    for (index, sans) in cases.into_iter().enumerate() {
        let mut p = CertificateParams::default();
        p.subject_alt_names = sans;
        p.extended_key_usages = vec![
            rcgen::ExtendedKeyUsagePurpose::ClientAuth,
            rcgen::ExtendedKeyUsagePurpose::ServerAuth,
        ];
        let der = p.signed_by(&key, &issuer).unwrap();
        let v = PeerVerifier::new(rootcert.der().clone(), [1; 16], registry(&key))
            .unwrap()
            .client(node(4));
        assert_eq!(
            refusal(direct(&v, der.der(), 1_800_000_000).unwrap_err()),
            if index == 4 {
                VerifyRefusal::WrongCluster
            } else {
                VerifyRefusal::Malformed
            }
        );
    }
    let ca = ClusterCa::ephemeral([1; 16], NOW);
    let der = leaf(&ca, &key);
    let mut trailing = der.clone();
    trailing.push(0);
    let v = verifier(&ca, registry(&key)).client(node(4));
    assert!(direct(&v, &trailing, 1001).is_err());
    for end in 0..der.len() {
        assert!(direct(&v, &der[..end], 1001).is_err());
    }
    let selfsigned = CertificateParams::default().self_signed(&key).unwrap();
    assert!(direct(&v, selfsigned.der(), 1001).is_err());
}

fn assert_time_error(error: Error, early: bool) {
    match (early, error) {
        (true, Error::InvalidCertificate(CertificateError::NotValidYetContext { .. }))
        | (false, Error::InvalidCertificate(CertificateError::ExpiredContext { .. })) => (),
        (_, other) => panic!("unexpected certificate time refusal: {other:?}"),
    }
}

#[test]
fn client_certificate_chain_time_and_registry_refusals() {
    let ca = ClusterCa::ephemeral([1; 16], NOW);
    let key = KeyPair::generate().unwrap();
    let der = leaf(&ca, &key);
    let v = verifier(&ca, registry(&key));
    let check = |v: &PeerVerifier, der: &[u8], seconds| {
        v.verify_client_cert(
            &CertificateDer::from(der),
            &[],
            UnixTime::since_unix_epoch(Duration::from_secs(seconds)),
        )
        .map(|_| ())
    };
    check(&v, &der, 1001).unwrap();
    let foreign = ClusterCa::ephemeral([1; 16], NOW);
    for bad in [
        leaf(&foreign, &key),
        CertificateParams::default()
            .self_signed(&key)
            .unwrap()
            .der()
            .to_vec(),
    ] {
        assert!(matches!(
            check(&v, &bad, 1001),
            Err(Error::InvalidCertificate(
                CertificateError::UnknownIssuer
                    | CertificateError::BadSignature
                    | CertificateError::NotValidYetContext { .. }
            ))
        ));
    }
    for seconds in [939, 941 + 30 * 86400] {
        assert_time_error(check(&v, &der, seconds).unwrap_err(), seconds == 939);
    }
    let wrong = KeyPair::generate().unwrap();
    assert_eq!(
        refusal(check(&v, &leaf(&ca, &wrong), 1001).unwrap_err()),
        VerifyRefusal::KeyMismatch
    );
    let absent = verifier(&ca, Arc::new(|_, _, _| Err(VerifyRefusal::NotMember)));
    assert_eq!(
        refusal(check(&absent, &der, 1001).unwrap_err()),
        VerifyRefusal::NotMember
    );
    let other_cluster = PeerVerifier::new(
        ca.cert_der().to_vec().into(),
        [2; 16],
        Arc::new(|_, _, _| Ok(())),
    )
    .unwrap();
    assert_eq!(
        refusal(check(&other_cluster, &der, 1001).unwrap_err()),
        VerifyRefusal::WrongCluster
    );
}

#[derive(Debug)]
struct MutableTime(std::sync::atomic::AtomicU64);
impl TimeProvider for MutableTime {
    fn current_time(&self) -> Option<UnixTime> {
        Some(UnixTime::since_unix_epoch(Duration::from_secs(
            self.0.load(std::sync::atomic::Ordering::SeqCst),
        )))
    }
}
fn reused_connections(
    c: &Arc<rustls::ClientConfig>,
    s: &Arc<rustls::ServerConfig>,
) -> (ClientConnection, ServerConnection) {
    (
        ClientConnection::new(c.clone(), ServerName::try_from(SERVER_NAME).unwrap()).unwrap(),
        ServerConnection::new(s.clone()).unwrap(),
    )
}
#[test]
fn reused_configs_always_perform_full_handshakes_and_recheck_revocation() {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    let ca = ClusterCa::ephemeral([1; 16], NOW);
    let key = KeyPair::generate().unwrap();
    let der = leaf(&ca, &key);
    let member = Arc::new(AtomicBool::new(true));
    let calls = Arc::new(AtomicUsize::new(0));
    let reg: Registry = {
        let member = member.clone();
        let calls = calls.clone();
        Arc::new(move |_, _, _| {
            calls.fetch_add(1, Ordering::SeqCst);
            if member.load(Ordering::SeqCst) {
                Ok(())
            } else {
                Err(VerifyRefusal::NotMember)
            }
        })
    };
    let c = Arc::new(
        client_config(
            verifier(&ca, reg.clone()).client(node(4)),
            der.clone().into(),
            PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
            Arc::new(FixedTime(1001)),
        )
        .unwrap(),
    );
    let s = Arc::new(
        server_config(
            verifier(&ca, reg).server(),
            der.into(),
            PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
            Arc::new(FixedTime(1001)),
        )
        .unwrap(),
    );
    for expected_calls in [2, 4] {
        let (client, server) = reused_connections(&c, &s);
        let (client, server) = complete(client, server).unwrap();
        assert_eq!(client.handshake_kind(), Some(rustls::HandshakeKind::Full));
        assert_eq!(server.handshake_kind(), Some(rustls::HandshakeKind::Full));
        assert_eq!(calls.load(Ordering::SeqCst), expected_calls);
    }
    member.store(false, Ordering::SeqCst);
    let (client, server) = reused_connections(&c, &s);
    assert_eq!(
        refusal(pump(client, server).err().unwrap()),
        VerifyRefusal::NotMember
    );
}
#[test]
fn reused_configs_recheck_expiry() {
    use std::sync::atomic::{AtomicU64, Ordering};
    let ca = ClusterCa::ephemeral([1; 16], NOW);
    let key = KeyPair::generate().unwrap();
    let der = leaf(&ca, &key);
    let time = Arc::new(MutableTime(AtomicU64::new(940 + 30 * 86400 - 1800)));
    let c = Arc::new(
        client_config(
            verifier(&ca, registry(&key)).client(node(4)),
            der.clone().into(),
            PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
            time.clone(),
        )
        .unwrap(),
    );
    let s = Arc::new(
        server_config(
            verifier(&ca, registry(&key)).server(),
            der.into(),
            PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
            time.clone(),
        )
        .unwrap(),
    );
    let (client, server) = reused_connections(&c, &s);
    pump(client, server).unwrap();
    time.0.store(940 + 30 * 86400 + 3600, Ordering::SeqCst);
    let (client, server) = reused_connections(&c, &s);
    assert_time_error(pump(client, server).err().unwrap(), false);
}
