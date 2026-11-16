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
pub(in crate::cluster) fn pump(
    mut c: ClientConnection,
    mut s: ServerConnection,
) -> Result<(AuthenticatedPeer, AuthenticatedPeer), Error> {
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
            return Ok((
                AuthenticatedPeer::from_client(&c)?,
                AuthenticatedPeer::from_server(&s)?,
            ));
        }
    }
    panic!("handshake exceeded 32 rounds")
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
            assert!(matches!(
                direct(&v, &der, seconds),
                Err(Error::InvalidCertificate(_))
            ));
            let (c, s) = connections(&ca, &der, &key, registry(&key), node(4), seconds);
            assert!(matches!(pump(c, s), Err(Error::InvalidCertificate(_))));
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
