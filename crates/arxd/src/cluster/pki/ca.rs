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
    #[cfg(test)]
    conversion_attempts: std::sync::atomic::AtomicUsize,
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
            conversion_attempts: std::sync::atomic::AtomicUsize::new(0),
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
        #[cfg(test)]
        self.conversion_attempts
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cluster::consensus::issue::test_ticket;
    use std::sync::atomic::Ordering;
    use x509_parser::parse_x509_certificate;

    const NOW: u64 = 1_000_000;

    #[test]
    fn sentinel_finite_ca_refuses_before_signing() {
        let key = KeyPair::generate().unwrap();
        let request = CertificateParams::default()
            .serialize_request(&key)
            .unwrap();
        let csr = ProvenCsr::parse(request.der()).unwrap();
        let ticket = test_ticket(csr.digest());
        let ca = ClusterCa::ephemeral([1; 16], NOW);
        assert_eq!(ca.signs.load(Ordering::SeqCst), 0);
        assert_eq!(ca.conversion_attempts.load(Ordering::SeqCst), 0);
        assert!(!ca.leaf_fits(u64::MAX));
        assert_eq!(
            ca.sign_leaf(&ticket, &csr, u64::MAX),
            Err(IssueError::CaExpiring)
        );
        assert_eq!(ca.signs.load(Ordering::SeqCst), 0);
        assert_eq!(ca.conversion_attempts.load(Ordering::SeqCst), 0);

        let der = ca.sign_leaf(&ticket, &csr, NOW).unwrap();
        assert!(!der.is_empty());
        let (remaining, _) = parse_x509_certificate(&der).unwrap();
        assert!(remaining.is_empty());
        assert_eq!(ca.signs.load(Ordering::SeqCst), 1);
        assert_eq!(ca.conversion_attempts.load(Ordering::SeqCst), 1);
        assert_eq!(
            ca.sign_leaf(&ticket, &csr, u64::MAX),
            Err(IssueError::CaExpiring)
        );
        assert_eq!(ca.signs.load(Ordering::SeqCst), 1);
        assert_eq!(ca.conversion_attempts.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn sentinel_conversion_refuses_before_signing() {
        let key = KeyPair::generate().unwrap();
        let request = CertificateParams::default()
            .serialize_request(&key)
            .unwrap();
        let csr = ProvenCsr::parse(request.der()).unwrap();
        let ticket = test_ticket(csr.digest());
        let mut ca = ClusterCa::ephemeral([1; 16], NOW);
        // Private, deliberately inconsistent branch fixture: its real issuer
        // and certificate remain finite. This is never a production CA policy.
        ca.not_after_s = i64::MAX;
        assert!(ca.leaf_fits(u64::MAX));
        let seconds = i64::try_from(u64::MAX / 1000).unwrap();
        assert_eq!(seconds, 18_446_744_073_709_551);
        let not_before = seconds.checked_sub(60).unwrap();
        let not_after = not_before.checked_add(2_592_000).unwrap();
        assert_eq!(not_before, 18_446_744_073_709_491);
        assert_eq!(not_after, 18_446_744_076_301_491);
        assert!(OffsetDateTime::from_unix_timestamp(not_before).is_err());
        assert!(OffsetDateTime::from_unix_timestamp(not_after).is_err());
        assert_eq!(ca.signs.load(Ordering::SeqCst), 0);
        assert_eq!(ca.conversion_attempts.load(Ordering::SeqCst), 0);
        assert_eq!(ca.sign_leaf(&ticket, &csr, u64::MAX), Err(IssueError::Sign));
        assert_eq!(ca.conversion_attempts.load(Ordering::SeqCst), 1);
        assert_eq!(ca.signs.load(Ordering::SeqCst), 0);

        let der = ca.sign_leaf(&ticket, &csr, NOW).unwrap();
        assert!(!der.is_empty());
        let (remaining, _) = parse_x509_certificate(&der).unwrap();
        assert!(remaining.is_empty());
        assert_eq!(ca.conversion_attempts.load(Ordering::SeqCst), 2);
        assert_eq!(ca.signs.load(Ordering::SeqCst), 1);
        assert_eq!(ca.sign_leaf(&ticket, &csr, u64::MAX), Err(IssueError::Sign));
        assert_eq!(ca.conversion_attempts.load(Ordering::SeqCst), 3);
        assert_eq!(ca.signs.load(Ordering::SeqCst), 1);
    }
}
