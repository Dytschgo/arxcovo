use super::{csr::ProvenCsr, node_uri};
use crate::cluster::consensus::issue::{IssueError, LeafSubject};
use rcgen::{
    CertificateParams, DistinguishedName, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose, SanType, SerialNumber,
};
use time::OffsetDateTime;
pub struct ClusterCa {
    issuer: Issuer<'static, KeyPair>,
    cert_der: Vec<u8>,
    not_after_s: i64,
    #[cfg(test)]
    pub(crate) signs: std::sync::atomic::AtomicUsize,
}
impl ClusterCa {
    #[cfg(test)]
    pub fn ephemeral(cluster: crate::cluster::enrollment::ClusterId, now_ms: u64) -> Self {
        Self::ephemeral_with_validity(cluster, now_ms, 31 * 86400)
    }
    #[cfg(test)]
    pub fn ephemeral_with_validity(
        _cluster: crate::cluster::enrollment::ClusterId,
        now_ms: u64,
        validity_s: i64,
    ) -> Self {
        let nb = (now_ms / 1000) as i64 - 120;
        let mut params = CertificateParams::default();
        params.not_before = OffsetDateTime::from_unix_timestamp(nb).unwrap();
        params.not_after = OffsetDateTime::from_unix_timestamp(nb + validity_s).unwrap();
        params.is_ca = IsCa::Ca(rcgen::BasicConstraints::Constrained(0));
        params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        params.distinguished_name = DistinguishedName::new();
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, "arxcovo test cluster CA");
        let key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
        let cert_der = params.self_signed(&key).unwrap().der().to_vec();
        Self {
            issuer: Issuer::new(params, key),
            cert_der,
            not_after_s: nb + validity_s,
            signs: std::sync::atomic::AtomicUsize::new(0),
        }
    }
    fn leaf_validity(now_ms: u64) -> Result<(i64, i64), IssueError> {
        let nb = i64::try_from(now_ms / 1000)
            .map_err(|_| IssueError::Sign)?
            .checked_sub(60)
            .ok_or(IssueError::Sign)?;
        let na = nb.checked_add(30 * 86400).ok_or(IssueError::Sign)?;
        Ok((nb, na))
    }
    pub fn leaf_fits(&self, now_ms: u64) -> bool {
        Self::leaf_validity(now_ms).is_ok_and(|(_, na)| na <= self.not_after_s)
    }
    pub fn fingerprint(&self) -> crate::cluster::enrollment::CaFingerprint {
        fingerprint_of(&self.cert_der)
    }
    pub fn cert_der(&self) -> &[u8] {
        &self.cert_der
    }
    #[cfg(test)]
    pub fn private_marker(&self) -> Vec<u8> {
        self.issuer.key().serialize_der()
    }
    pub fn sign_leaf<S: LeafSubject>(
        &self,
        ticket: &S,
        csr: &ProvenCsr,
        now_ms: u64,
    ) -> Result<Vec<u8>, IssueError> {
        if ticket.key() != csr.digest() {
            return Err(IssueError::Revoked);
        }
        let (nb, na) = Self::leaf_validity(now_ms)?;
        if !self.leaf_fits(now_ms) {
            return Err(IssueError::CaExpiring);
        }
        let mut serial = [0; 16];
        getrandom::fill(&mut serial).map_err(|_| IssueError::Sign)?;
        serial[0] = (serial[0] & 0x7f) | 0x40;
        let mut params = CertificateParams::default();
        params.not_before =
            OffsetDateTime::from_unix_timestamp(nb).map_err(|_| IssueError::Sign)?;
        params.not_after = OffsetDateTime::from_unix_timestamp(na).map_err(|_| IssueError::Sign)?;
        params.distinguished_name = DistinguishedName::new();
        params.is_ca = IsCa::NoCa;
        params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        params.extended_key_usages = vec![
            ExtendedKeyUsagePurpose::ServerAuth,
            ExtendedKeyUsagePurpose::ClientAuth,
        ];
        params.subject_alt_names = vec![SanType::URI(
            node_uri(ticket.cluster(), ticket.node())
                .try_into()
                .map_err(|_| IssueError::Sign)?,
        )];
        params.serial_number = Some(SerialNumber::from_slice(&serial));
        #[cfg(test)]
        self.signs.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let der = params
            .signed_by(&csr.public_key, &self.issuer)
            .map_err(|_| IssueError::Sign)?
            .der()
            .to_vec();
        if der.len() > super::MAX_CSR {
            return Err(IssueError::Sign);
        }
        Ok(der)
    }
}

pub fn fingerprint_of(der: &[u8]) -> crate::cluster::enrollment::CaFingerprint {
    use sha2::{Digest, Sha256};
    Sha256::digest(der).into()
}
