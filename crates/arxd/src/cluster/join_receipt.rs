//! Public consistency only. The caller supplies trusted history separately;
//! certificate validation neither authenticates that history nor unseals a key.
#![allow(dead_code)]
use super::{
    enrollment::{ClusterId, KeyDigest, NodeId},
    pki::verify::{PeerVerifier, VerifyRefusal},
    state_store::LogPos,
};
use rustls::{
    pki_types::{CertificateDer, UnixTime},
    server::danger::ClientCertVerifier,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "role", deny_unknown_fields)]
pub(super) enum Origin {
    #[serde(rename = "founder")]
    Found { term: u64, leader: u64, index: u64 },
    #[serde(rename = "joiner")]
    Redeem {
        term: u64,
        leader: u64,
        index: u64,
        anchors: Vec<(u64, [u8; 32])>,
    },
}
impl Origin {
    pub(super) fn position(&self) -> LogPos {
        let (term, leader, index) = match self {
            Self::Found {
                term,
                leader,
                index,
            }
            | Self::Redeem {
                term,
                leader,
                index,
                ..
            } => (*term, *leader, *index),
        };
        LogPos {
            term,
            leader,
            index,
        }
    }
    pub(super) fn founder(&self) -> bool {
        matches!(self, Self::Found { .. })
    }
    fn valid(&self, node: u64) -> bool {
        if self.position().index > i64::MAX as u64 {
            return false;
        }
        match self {
            Self::Found { .. } => node == 1,
            Self::Redeem { anchors, .. } => {
                node >= 2
                    && (1..=16).contains(&anchors.len())
                    && anchors.iter().all(|(n, _)| *n != 0 && *n != node)
                    && anchors.windows(2).all(|w| w[0].0 < w[1].0)
            }
        }
    }
}
/// Constructed by the trusted caller, never by decoding an identity record.
pub(super) struct ExpectedIdentity {
    pub cluster: ClusterId,
    pub node: NodeId,
    pub key: KeyDigest,
    pub ca: [u8; 32],
    pub origin: Origin,
}
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PublicIdentity {
    version: u8,
    cluster: ClusterId,
    node: u64,
    key: [u8; 32],
    ca: [u8; 32],
    origin: Origin,
    leaf: Vec<u8>,
    artifact: String,
}
#[derive(Debug, PartialEq, Eq)]
pub(super) struct InvalidIdentity;
pub(super) fn basename(s: &str) -> bool {
    (1..=64).contains(&s.len())
        && s != "."
        && s != ".."
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        && !matches!(
            s.to_ascii_uppercase().as_str(),
            "CON"
                | "PRN"
                | "AUX"
                | "NUL"
                | "COM1"
                | "COM2"
                | "COM3"
                | "COM4"
                | "COM5"
                | "COM6"
                | "COM7"
                | "COM8"
                | "COM9"
                | "LPT1"
                | "LPT2"
                | "LPT3"
                | "LPT4"
                | "LPT5"
                | "LPT6"
                | "LPT7"
                | "LPT8"
                | "LPT9"
        )
}
impl PublicIdentity {
    pub(super) fn decode(bytes: &[u8]) -> Result<Self, InvalidIdentity> {
        if bytes.len() > 8192 {
            return Err(InvalidIdentity);
        }
        let record: Self = serde_json::from_slice(bytes).map_err(|_| InvalidIdentity)?;
        if record.version != 1
            || !record.origin.valid(record.node)
            || !basename(&record.artifact)
            || record.leaf.is_empty()
            || record.leaf.len() > 4096
        {
            return Err(InvalidIdentity);
        }
        Ok(record)
    }
    pub(super) fn artifact(&self) -> &str {
        &self.artifact
    }
    pub(super) fn verify(
        &self,
        expected: &ExpectedIdentity,
        root: &[u8],
        now: UnixTime,
    ) -> Result<(), InvalidIdentity> {
        if root.is_empty()
            || root.len() > 4096
            || self.cluster != expected.cluster
            || self.node != expected.node.get()
            || self.key != *expected.key.as_bytes()
            || self.ca != expected.ca
            || self.origin != expected.origin
            || !expected.origin.valid(expected.node.get())
            || Sha256::digest(root).as_slice() != expected.ca
        {
            return Err(InvalidIdentity);
        }
        let cluster = expected.cluster;
        let node = expected.node;
        let key = expected.key;
        let verifier = PeerVerifier::new(
            CertificateDer::from(root.to_vec()),
            cluster,
            Arc::new(move |c, n, k| {
                if (c, n, k) == (cluster, node, key) {
                    Ok(())
                } else {
                    Err(VerifyRefusal::KeyMismatch)
                }
            }),
        )
        .map_err(|_| InvalidIdentity)?
        .server();
        verifier
            .verify_client_cert(&CertificateDer::from(self.leaf.as_slice()), &[], now)
            .map_err(|_| InvalidIdentity)?;
        Ok(())
    }
}

#[cfg(test)]
pub(in crate::cluster) mod tests {
    use super::*;
    use rcgen::{
        CertificateParams, ExtendedKeyUsagePurpose, IsCa, KeyPair, KeyUsagePurpose, SanType,
    };
    pub(in crate::cluster) fn fixture(
        node: u64,
        origin: Origin,
    ) -> (Vec<u8>, Vec<u8>, ExpectedIdentity) {
        let now = time::OffsetDateTime::now_utc();
        let mut p = CertificateParams::default();
        p.not_before = now - time::Duration::days(1);
        p.not_after = now + time::Duration::days(2);
        p.is_ca = IsCa::Ca(rcgen::BasicConstraints::Constrained(0));
        p.key_usages = vec![KeyUsagePurpose::KeyCertSign];
        let ca_key = KeyPair::generate().unwrap();
        let ca = p.self_signed(&ca_key).unwrap();
        let issuer = rcgen::Issuer::new(p, ca_key);
        let key = KeyPair::generate().unwrap();
        use rcgen::PublicKeyData;
        let digest = KeyDigest::of_spki(&key.subject_public_key_info()).unwrap();
        let n = NodeId::new(node).unwrap();
        let mut p = CertificateParams::default();
        p.not_before = now - time::Duration::days(1);
        p.not_after = now + time::Duration::days(1);
        p.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        p.extended_key_usages = vec![
            ExtendedKeyUsagePurpose::ClientAuth,
            ExtendedKeyUsagePurpose::ServerAuth,
        ];
        p.subject_alt_names = vec![SanType::URI(
            crate::cluster::pki::node_uri([1; 16], n)
                .try_into()
                .unwrap(),
        )];
        let leaf = p.signed_by(&key, &issuer).unwrap();
        let root = ca.der().to_vec();
        let fingerprint = Sha256::digest(&root).into();
        let record = PublicIdentity {
            version: 1,
            cluster: [1; 16],
            node,
            key: *digest.as_bytes(),
            ca: fingerprint,
            origin: origin.clone(),
            leaf: leaf.der().to_vec(),
            artifact: "sealed-key".into(),
        };
        (
            serde_json::to_vec(&record).unwrap(),
            root,
            ExpectedIdentity {
                cluster: [1; 16],
                node: n,
                key: digest,
                ca: fingerprint,
                origin,
            },
        )
    }
    fn now() -> UnixTime {
        UnixTime::now()
    }
    #[test]
    fn public_roles_crypto_and_trusted_history() {
        for (n, origin) in [
            (
                1,
                Origin::Found {
                    term: 1,
                    leader: 1,
                    index: 1,
                },
            ),
            (
                2,
                Origin::Redeem {
                    term: 1,
                    leader: 1,
                    index: 3,
                    anchors: vec![(1, [9; 32])],
                },
            ),
        ] {
            let (bytes, root, mut expected) = fixture(n, origin);
            let record = PublicIdentity::decode(&bytes).unwrap();
            record.verify(&expected, &root, now()).unwrap();
            expected.cluster = [2; 16];
            assert!(record.verify(&expected, &root, now()).is_err());
            expected.cluster = [1; 16];
            let key = expected.key;
            expected.key = KeyDigest::of_spki(b"wrong-key").unwrap();
            assert!(record.verify(&expected, &root, now()).is_err());
            expected.key = key;
            expected.ca = [0; 32];
            assert!(record.verify(&expected, &root, now()).is_err());
            expected.ca = record.ca;
            if n == 2 {
                let old = expected.origin.clone();
                expected.origin = Origin::Redeem {
                    term: 1,
                    leader: 1,
                    index: 3,
                    anchors: vec![(1, [8; 32])],
                };
                assert!(record.verify(&expected, &root, now()).is_err());
                expected.origin = old;
                // Shape and trusted tuple agree; the actual sole SAN is still N2.
                let mut wrong_san = PublicIdentity::decode(&bytes).unwrap();
                wrong_san.node = 3;
                expected.node = NodeId::new(3).unwrap();
                assert!(wrong_san.verify(&expected, &root, now()).is_err());
                expected.node = NodeId::new(2).unwrap();
            }
            let mut bad = PublicIdentity::decode(&bytes).unwrap();
            bad.leaf[30] ^= 1;
            assert!(bad.verify(&expected, &root, now()).is_err());
            expected.origin = if n == 1 {
                Origin::Redeem {
                    term: 1,
                    leader: 1,
                    index: 1,
                    anchors: vec![(2, [8; 32])],
                }
            } else {
                Origin::Redeem {
                    term: 1,
                    leader: 1,
                    index: 4,
                    anchors: vec![(1, [9; 32])],
                }
            };
            assert!(record.verify(&expected, &root, now()).is_err());
        }
    }
    #[test]
    fn closed_codec_and_basename_refusals() {
        let (bytes, _, _) = fixture(
            2,
            Origin::Redeem {
                term: 1,
                leader: 1,
                index: 3,
                anchors: vec![(1, [9; 32])],
            },
        );
        for mutation in ["version", "artifact", "node", "origin", "unknown"] {
            let mut v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            match mutation {
                "version" => v[mutation] = 2.into(),
                "artifact" => v[mutation] = "../key".into(),
                "node" => v[mutation] = 1.into(),
                "origin" => v["origin"]["anchors"] = serde_json::json!([[2, vec![0; 32]]]),
                _ => v[mutation] = true.into(),
            }
            assert!(
                PublicIdentity::decode(&serde_json::to_vec(&v).unwrap()).is_err(),
                "{mutation}"
            );
        }
        let mut trailing = bytes.clone();
        trailing.extend(b"x");
        assert!(PublicIdentity::decode(&trailing).is_err());
        assert!(PublicIdentity::decode(&vec![b' '; 8193]).is_err());
        for s in ["", ".", "..", "x:y", "x/y", "x\\y", "CON", "LPT9"] {
            assert!(!basename(s));
        }
        assert!(basename("sealed-key_01"));
    }
}
