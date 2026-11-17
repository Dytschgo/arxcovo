use super::*;
use csr::{CsrError, ProvenCsr};
use rcgen::{CertificateParams, KeyPair, PublicKeyData};
use x509_parser::prelude::*;
fn request(key: &KeyPair) -> Vec<u8> {
    CertificateParams::default()
        .serialize_request(key)
        .unwrap()
        .der()
        .to_vec()
}
#[test]
fn possession_algorithms_and_bounded_shape() {
    for alg in [&rcgen::PKCS_ECDSA_P256_SHA256, &rcgen::PKCS_ED25519] {
        let key = KeyPair::generate_for(alg).unwrap();
        let der = request(&key);
        let csr = ProvenCsr::parse(&der).unwrap();
        assert_eq!(
            csr.digest(),
            crate::cluster::enrollment::KeyDigest::of_spki(&key.subject_public_key_info()).unwrap()
        );
        for end in 1..der.len() {
            assert!(
                matches!(ProvenCsr::parse(&der[..end]), Err(CsrError::Malformed)),
                "prefix {end}"
            );
        }
        let mut trailing = der.clone();
        trailing.push(0);
        assert!(matches!(
            ProvenCsr::parse(&trailing),
            Err(CsrError::Malformed)
        ));
        let mut signature = der.clone();
        *signature.last_mut().unwrap() ^= 1;
        assert!(matches!(
            ProvenCsr::parse(&signature),
            Err(CsrError::BadSignature)
        ));
        let (_, parsed) = X509CertificationRequest::from_der(&der).unwrap();
        let spki = parsed.certification_request_info.subject_pki.raw;
        let offset = spki.as_ptr() as usize - der.as_ptr() as usize;
        let mut tampered = der.clone();
        tampered[offset + spki.len() - 1] ^= 1;
        assert!(matches!(
            ProvenCsr::parse(&tampered),
            Err(CsrError::BadSignature)
        ));
    }
    for der in [
        include_bytes!("fixtures/p384_sha256.csr.der").as_slice(),
        include_bytes!("fixtures/rsa2048_sha256.csr.der").as_slice(),
    ] {
        assert!(matches!(
            ProvenCsr::parse(der),
            Err(CsrError::UnsupportedKey)
        ));
    }
    let mut mismatch = request(&KeyPair::generate().unwrap());
    let oid = [0x06, 0x08, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x02];
    let offset = mismatch.windows(oid.len()).position(|w| w == oid).unwrap();
    mismatch[offset + 9] = 3; // ecdsa-with-SHA384 over prime256v1
    assert!(matches!(
        ProvenCsr::parse(&mismatch),
        Err(CsrError::UnsupportedKey)
    ));
    let key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P384_SHA384).unwrap();
    assert!(matches!(
        ProvenCsr::parse(&request(&key)),
        Err(CsrError::UnsupportedKey)
    ));
    for bytes in [&[][..], &[0; 4097][..]] {
        assert!(matches!(ProvenCsr::parse(bytes), Err(CsrError::Oversized)));
    }
    let key = KeyPair::generate().unwrap();
    let mut p = CertificateParams::default();
    p.custom_extensions
        .push(rcgen::CustomExtension::from_oid_content(
            &[1, 2, 3, 4],
            vec![5, 0],
        ));
    assert!(matches!(
        ProvenCsr::parse(p.serialize_request(&key).unwrap().der()),
        Err(CsrError::Malformed)
    ));
}
#[test]
fn fresh_leaf_discards_requested_authority_and_contains_exact_identity() {
    use rcgen::*;
    let key = KeyPair::generate().unwrap();
    let mut p = CertificateParams::default();
    p.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    p.key_usages = vec![KeyUsagePurpose::KeyCertSign];
    p.extended_key_usages = vec![
        ExtendedKeyUsagePurpose::Any,
        ExtendedKeyUsagePurpose::CodeSigning,
    ];
    p.subject_alt_names = vec![
        SanType::DnsName("untrusted.invalid".try_into().unwrap()),
        SanType::URI(
            node_uri([9; 16], std::num::NonZeroU64::new(99).unwrap())
                .try_into()
                .unwrap(),
        ),
    ];
    p.distinguished_name
        .push(DnType::CommonName, "untrusted-marker");
    let csr = ProvenCsr::parse(p.serialize_request(&key).unwrap().der()).unwrap();
    let ticket = crate::cluster::consensus::issue::test_ticket(csr.digest());
    let ca = ca::ClusterCa::ephemeral([1; 16], 1000);
    let other = ca::ClusterCa::ephemeral([1; 16], 1000);
    let der = ca.sign_leaf(&ticket, &csr, 1000).unwrap();
    let der2 = ca.sign_leaf(&ticket, &csr, 1000).unwrap();
    let (_, leaf) = X509Certificate::from_der(&der).unwrap();
    let (_, leaf2) = X509Certificate::from_der(&der2).unwrap();
    let (_, root) = X509Certificate::from_der(ca.cert_der()).unwrap();
    let (_, wrong) = X509Certificate::from_der(other.cert_der()).unwrap();
    leaf.verify_signature(Some(root.public_key())).unwrap();
    assert!(leaf.verify_signature(Some(wrong.public_key())).is_err());
    assert!(der.len() <= 4096);
    assert_eq!(leaf.public_key().raw, key.subject_public_key_info());
    assert!(leaf.subject().iter().next().is_none());
    assert!(!leaf.is_ca());
    assert_eq!(leaf.key_usage().unwrap().unwrap().value.flags, 1);
    let eku = leaf.extended_key_usage().unwrap().unwrap().value;
    assert!(
        eku.server_auth
            && eku.client_auth
            && !eku.any
            && !eku.code_signing
            && !eku.email_protection
            && !eku.time_stamping
            && !eku.ocsp_signing
            && eku.other.is_empty()
    );
    let san = leaf.subject_alternative_name().unwrap().unwrap().value;
    assert_eq!(
        san.general_names,
        vec![GeneralName::URI(
            "arxcovo://cluster/01010101010101010101010101010101/node/4"
        )]
    );
    assert_eq!(leaf.validity().not_before.timestamp(), -59);
    assert_eq!(leaf.validity().not_after.timestamp(), -59 + 30 * 86400);
    assert_eq!(leaf.raw_serial().len(), 16);
    assert_eq!(leaf.raw_serial()[0] & 0xc0, 0x40);
    assert_ne!(leaf.raw_serial(), leaf2.raw_serial());
}
#[test]
fn ca_validity_boundaries() {
    use crate::cluster::consensus::issue::{IssueError, test_ticket};
    let key = KeyPair::generate().unwrap();
    let csr = ProvenCsr::parse(&request(&key)).unwrap();
    let ticket = test_ticket(csr.digest());
    for signer in [1000, 1000 + (86400 - 60) * 1000] {
        assert!(
            ca::ClusterCa::ephemeral([1; 16], 1000)
                .sign_leaf(&ticket, &csr, signer)
                .is_ok()
        );
        assert_eq!(
            ca::ClusterCa::ephemeral_with_validity([1; 16], 1000, 86400)
                .sign_leaf(&ticket, &csr, signer),
            Err(IssueError::CaExpiring)
        );
    }
    for (extra, pass) in [(59, false), (60, true)] {
        let ca = ca::ClusterCa::ephemeral_with_validity([1; 16], 1000, 30 * 86400 + extra);
        assert_eq!(ca.sign_leaf(&ticket, &csr, 1000).is_ok(), pass);
        assert_eq!(
            ca.signs.load(std::sync::atomic::Ordering::SeqCst),
            usize::from(pass)
        );
    }
}
#[test]
fn strict_uri_and_static_errors() {
    let uri = node_uri([0xab; 16], std::num::NonZeroU64::new(u64::MAX).unwrap());
    assert_eq!(
        parse_node_uri(&uri),
        Some(([0xab; 16], std::num::NonZeroU64::new(u64::MAX).unwrap()))
    );
    for bad in [
        uri.to_uppercase(),
        uri.replace("/node/", "/node/0"),
        uri.replace(&u64::MAX.to_string(), "0"),
        format!("{uri}/"),
        "x".repeat(4097),
        uri.replace("arxcovo:", "https:"),
    ] {
        assert!(parse_node_uri(&bad).is_none());
    }
    for e in [
        CsrError::Oversized,
        CsrError::Malformed,
        CsrError::UnsupportedKey,
        CsrError::BadSignature,
    ] {
        assert!(!format!("{e} {e:?}").contains("untrusted-marker"));
    }
}

#[test]
fn signature_algorithm_parameters_are_refused() {
    let der = request(&KeyPair::generate().unwrap());
    let alg = [
        0x30u8, 0x0a, 0x06, 0x08, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x02,
    ];
    let pos = der.windows(alg.len()).rposition(|w| w == alg).unwrap();
    assert_eq!(der[0], 0x30);
    let (hl, len) = match der[1] {
        0x81 => (3, der[2] as usize),
        0x82 => (4, ((der[2] as usize) << 8) | der[3] as usize),
        n if n < 0x80 => (2, n as usize),
        _ => panic!("length form"),
    };
    assert_eq!(hl + len, der.len());
    let mut body = der[hl..pos].to_vec();
    body.extend_from_slice(&[
        0x30, 0x0c, 0x06, 0x08, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x02, 0x05, 0x00,
    ]);
    body.extend_from_slice(&der[pos + alg.len()..]);
    let n = body.len();
    let mut out = vec![0x30u8];
    if n < 0x80 {
        out.push(n as u8)
    } else if n < 0x100 {
        out.extend([0x81, n as u8])
    } else {
        out.extend([0x82, (n >> 8) as u8, n as u8])
    }
    out.extend(body);
    let r = ProvenCsr::parse(&out);
    assert!(matches!(r, Err(CsrError::UnsupportedKey)));
}

#[test]
fn founder_pure_identity_and_validity() {
    use crate::cluster::consensus::issue::found::{FoundError, founder_init, generate_cluster_id};
    assert_eq!(
        generate_cluster_id(|b| {
            b.fill(17);
            Ok::<_, ()>(())
        }),
        Ok([17; 16])
    );
    assert_eq!(
        generate_cluster_id(|_| Ok::<_, ()>(())),
        Err(FoundError::Rng)
    );
    assert_eq!(generate_cluster_id(|_| Err(())), Err(FoundError::Rng));
    assert_eq!(founder_init([17; 16]).first_free.get(), 2);
    let now = 1_000_000;
    let key = KeyPair::generate().unwrap();
    let csr = ProvenCsr::parse(&request(&key)).unwrap();
    let ticket = crate::cluster::consensus::issue::test_ticket(csr.digest());
    for (seconds, fits) in [(30 * 86400 + 59, false), (30 * 86400 + 60, true)] {
        let ca = ca::ClusterCa::ephemeral_with_validity([1; 16], now, seconds);
        assert_eq!(ca.leaf_fits(now), fits);
        let signed = ca.sign_leaf(&ticket, &csr, now);
        if fits {
            assert!(signed.is_ok());
        } else {
            assert_eq!(
                signed,
                Err(crate::cluster::consensus::issue::IssueError::CaExpiring)
            );
        }
        assert_eq!(ca.fingerprint(), ca::fingerprint_of(ca.cert_der()));
        assert_eq!(ca.fingerprint(), ca.fingerprint());
        assert_ne!(
            ca.fingerprint(),
            ca::ClusterCa::ephemeral([1; 16], now).fingerprint()
        );
    }
    for error in [
        FoundError::Busy,
        FoundError::Unavailable,
        FoundError::NotFounder,
        FoundError::AlreadyInitialized,
        FoundError::FounderAllocation,
        FoundError::Rng,
        FoundError::BadCsr(CsrError::Malformed),
        FoundError::Rejected(crate::cluster::enrollment::Reject::UnknownCode),
        FoundError::WrongNode,
        FoundError::StaleIssuer,
        FoundError::Revoked,
        FoundError::ClockRollback,
        FoundError::CaExpiring,
        FoundError::Sign,
    ] {
        assert!(!error.to_string().contains("private-marker"));
        assert!(!error.to_string().is_empty());
    }
}
