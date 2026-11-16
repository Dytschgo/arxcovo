use super::MAX_CSR;
use crate::cluster::enrollment::KeyDigest;
use rcgen::{CertificateSigningRequestParams, PublicKey, PublicKeyData};
use x509_parser::prelude::*;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CsrError {
    Oversized,
    Malformed,
    UnsupportedKey,
    BadSignature,
}
impl std::fmt::Display for CsrError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Oversized => "CSR size refused",
            Self::Malformed => "malformed CSR",
            Self::UnsupportedKey => "unsupported CSR key",
            Self::BadSignature => "invalid CSR signature",
        })
    }
}
impl std::error::Error for CsrError {}
pub struct ProvenCsr {
    pub(super) public_key: PublicKey,
    digest: KeyDigest,
}
impl ProvenCsr {
    pub fn parse(bytes: &[u8]) -> Result<Self, CsrError> {
        if bytes.is_empty() || bytes.len() > MAX_CSR {
            return Err(CsrError::Oversized);
        }
        let (rest, csr) =
            X509CertificationRequest::from_der(bytes).map_err(|_| CsrError::Malformed)?;
        if !rest.is_empty() {
            return Err(CsrError::Malformed);
        }
        let spki = &csr.certification_request_info.subject_pki;
        let key_alg = &spki.algorithm;
        let sig_alg = &csr.signature_algorithm;
        let ed = key_alg.algorithm.to_id_string() == "1.3.101.112"
            && key_alg.parameters.is_none()
            && sig_alg.algorithm.to_id_string() == "1.3.101.112";
        let ec = key_alg.algorithm.to_id_string() == "1.2.840.10045.2.1"
            && key_alg
                .parameters
                .as_ref()
                .and_then(|p| p.as_oid().ok())
                .is_some_and(|o| o.to_id_string() == "1.2.840.10045.3.1.7")
            && sig_alg.algorithm.to_id_string() == "1.2.840.10045.4.3.2";
        if !(ed || ec) || sig_alg.parameters.is_some() {
            return Err(CsrError::UnsupportedKey);
        }
        let parsed =
            CertificateSigningRequestParams::from_der(&bytes.into()).map_err(|e| match e {
                rcgen::Error::InvalidCertificationRequestSignature => CsrError::BadSignature,
                rcgen::Error::UnsupportedSignatureAlgorithm => CsrError::UnsupportedKey,
                _ => CsrError::Malformed,
            })?;
        let public_key = parsed.public_key;
        drop(parsed.params);
        if public_key.subject_public_key_info() != spki.raw {
            return Err(CsrError::UnsupportedKey);
        }
        let digest = KeyDigest::of_spki(spki.raw).map_err(|_| CsrError::Malformed)?;
        Ok(Self { public_key, digest })
    }
    pub fn digest(&self) -> KeyDigest {
        self.digest
    }
}
