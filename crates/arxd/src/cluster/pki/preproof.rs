//! Private bootstrap probe. Chain trust alone grants no membership authority.
use super::verify::{SERVER_NAME, peer_identity};
use crate::cluster::enrollment::{BoundVoters, ClusterId, CodeId, JoinAnchor, JoinCode};
use rustls::{
    ClientConfig, ClientConnection, DigitallySignedStruct, Error, ProtocolVersion, RootCertStore,
    SignatureScheme,
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    crypto::{CryptoProvider, ring, verify_tls13_signature},
    pki_types::{CertificateDer, ServerName, UnixTime},
    server::ParsedCertificate,
    time_provider::TimeProvider,
};
use sha2::{Digest, Sha256};
use std::{
    io::{self, Read, Write},
    sync::Arc,
};

const QUOTA: usize = 128 * 1024;
#[cfg(test)]
thread_local! { pub(super) static SIGNATURE_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::cluster) struct Refused;
impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("bootstrap refused")
    }
}
impl std::error::Error for Refused {}
struct BootstrapVerifier {
    cluster: ClusterId,
    roots: RootCertStore,
    provider: Arc<CryptoProvider>,
}
impl std::fmt::Debug for BootstrapVerifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BootstrapVerifier")
    }
}
impl ServerCertVerifier for BootstrapVerifier {
    fn verify_server_cert(
        &self,
        ee: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        if ee.len() > 4096 || !intermediates.is_empty() {
            return Err(Error::General("bootstrap refused".into()));
        }
        rustls::client::verify_server_cert_signed_by_trust_anchor(
            &ParsedCertificate::try_from(ee)?,
            &self.roots,
            &[],
            now,
            self.provider.signature_verification_algorithms.all,
        )?;
        if peer_identity(ee)?.cluster != self.cluster {
            return Err(Error::General("bootstrap refused".into()));
        }
        Ok(ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        _: &[u8],
        _: &CertificateDer<'_>,
        _: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        Err(Error::General("bootstrap refused".into()))
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        #[cfg(test)]
        SIGNATURE_CALLS.with(|n| n.set(n.get() + 1));
        verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Initial,
    Queried,
    Anchored,
    Released,
    Fatal,
}
pub(in crate::cluster) struct PreproofClient {
    connection: ClientConnection,
    cluster: ClusterId,
    code: CodeId,
    anchor: JoinAnchor,
    fingerprint: [u8; 32],
    phase: Phase,
    incoming: usize,
    outgoing: usize,
}
impl PreproofClient {
    pub(in crate::cluster) fn new(
        code: &JoinCode,
        root: &[u8],
        time: Arc<dyn TimeProvider>,
    ) -> Result<Self, Refused> {
        if root.is_empty()
            || root.len() > 4096
            || <[u8; 32]>::from(Sha256::digest(root)) != code.ca_fingerprint()
        {
            return Err(Refused);
        }
        let mut roots = RootCertStore::empty();
        roots
            .add(CertificateDer::from(root.to_vec()))
            .map_err(|_| Refused)?;
        let provider = Arc::new(ring::default_provider());
        let verifier = Arc::new(BootstrapVerifier {
            cluster: code.cluster(),
            roots,
            provider: provider.clone(),
        });
        let mut config = ClientConfig::builder_with_details(provider, time)
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(|_| Refused)?
            .dangerous()
            .with_custom_certificate_verifier(verifier)
            .with_no_client_auth();
        config.enable_sni = false;
        config.resumption = rustls::client::Resumption::disabled();
        config.enable_early_data = false;
        let mut connection = ClientConnection::new(
            Arc::new(config),
            ServerName::try_from(SERVER_NAME).map_err(|_| Refused)?,
        )
        .map_err(|_| Refused)?;
        connection.set_buffer_limit(Some(4096));
        Ok(Self {
            connection,
            cluster: code.cluster(),
            code: code.code_id(),
            anchor: code.anchor(),
            fingerprint: code.ca_fingerprint(),
            phase: Phase::Initial,
            incoming: 0,
            outgoing: 0,
        })
    }
    fn fail<T>(&mut self) -> Result<T, Refused> {
        self.phase = Phase::Fatal;
        Err(Refused)
    }
    fn active(&self) -> bool {
        !matches!(self.phase, Phase::Fatal | Phase::Released)
    }
    fn complete(&self) -> bool {
        !self.connection.is_handshaking()
            && self.connection.protocol_version() == Some(ProtocolVersion::TLSv1_3)
    }
    /// Full offered length is preflighted; only consumed bytes are charged.
    /// Caller retains and resubmits any unconsumed suffix.
    pub(in crate::cluster) fn feed(&mut self, bytes: &[u8]) -> Result<usize, Refused> {
        if !self.active()
            || self
                .incoming
                .checked_add(bytes.len())
                .is_none_or(|n| n > QUOTA)
        {
            return self.fail();
        }
        let mut input = bytes;
        let Ok(n) = self.connection.read_tls(&mut input) else {
            return self.fail();
        };
        self.incoming += n;
        let Ok(state) = self.connection.process_new_packets() else {
            return self.fail();
        };
        if self
            .outgoing
            .checked_add(state.tls_bytes_to_write())
            .is_none_or(|n| n > QUOTA)
        {
            return self.fail();
        }
        Ok(n)
    }
    pub(in crate::cluster) fn drain(&mut self, buffer: &mut [u8]) -> Result<usize, Refused> {
        if self.phase == Phase::Fatal {
            return Err(Refused);
        }
        if self.outgoing > QUOTA || (self.outgoing == QUOTA && self.connection.wants_write()) {
            return self.fail();
        }
        let limit = buffer.len().min(QUOTA - self.outgoing);
        let mut writer = SliceWriter {
            buffer: &mut buffer[..limit],
            copied: 0,
        };
        let result = self.connection.write_tls(&mut writer);
        let n = writer.copied;
        self.outgoing += n;
        if result.is_err() {
            return self.fail();
        }
        Ok(n)
    }
    pub(in crate::cluster) fn query(&mut self) -> Result<(), Refused> {
        if self.phase != Phase::Initial || !self.complete() {
            return self.fail();
        }
        let mut query = [0; 32];
        query[..16].copy_from_slice(&self.cluster);
        query[16..].copy_from_slice(&self.code);
        if self.connection.writer().write(&query).ok() != Some(32) {
            return self.fail();
        }
        self.phase = Phase::Queried;
        Ok(())
    }
    /// Read into caller storage; plaintext is untrusted until accept_anchor.
    pub(in crate::cluster) fn read_anchor(&mut self, buffer: &mut [u8]) -> Result<usize, Refused> {
        if self.phase != Phase::Queried || buffer.len() > 641 {
            return self.fail();
        }
        match self.connection.reader().read(buffer) {
            Ok(n) => Ok(n),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => Ok(0),
            Err(_) => self.fail(),
        }
    }
    pub(in crate::cluster) fn accept_anchor(&mut self, bytes: &[u8]) -> Result<(), Refused> {
        if self.phase != Phase::Queried || !self.complete() || bytes.len() > 641 {
            return self.fail();
        }
        let Ok(voters) = BoundVoters::decode(bytes) else {
            return self.fail();
        };
        if voters.anchor(self.cluster) != self.anchor {
            return self.fail();
        }
        let Some([cert]) = self.connection.peer_certificates() else {
            return self.fail();
        };
        let Ok(id) = peer_identity(cert) else {
            return self.fail();
        };
        if id.cluster != self.cluster || !voters.contains(id.node, id.key) {
            return self.fail();
        }
        self.phase = Phase::Anchored;
        Ok(())
    }
    pub(in crate::cluster) fn release_proof(&mut self, code: &JoinCode) -> Result<(), Refused> {
        if self.phase != Phase::Anchored
            || !self.complete()
            || code.cluster() != self.cluster
            || code.code_id() != self.code
            || code.anchor() != self.anchor
            || code.ca_fingerprint() != self.fingerprint
        {
            return self.fail();
        }
        self.phase = Phase::Fatal;
        if self.connection.writer().write(code.proof().bytes()).ok() != Some(32) {
            return Err(Refused);
        }
        self.phase = Phase::Released;
        Ok(())
    }
}
struct SliceWriter<'a> {
    buffer: &'a mut [u8],
    copied: usize,
}
impl Write for SliceWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let n = bytes.len().min(self.buffer.len() - self.copied);
        self.buffer[self.copied..self.copied + n].copy_from_slice(&bytes[..n]);
        self.copied += n;
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod quota_tests {
    use super::super::preproof_tests::{ProbeTime, prepared, test_client};
    use super::*;
    #[test]
    fn partial_proof_write_is_fatal_and_cannot_drain() {
        let (mut c, mut s, code) = prepared();
        c.connection.set_buffer_limit(Some(1));
        assert!(c.release_proof(&code).is_err());
        assert!(c.release_proof(&code).is_err());
        assert!(c.drain(&mut [0; 4096]).is_err());
        assert!(s.reader().read(&mut [0; 32]).is_err());
    }
    #[test]
    fn whole_session_admission_and_fatal_drain() {
        let mut c = test_client();
        assert!(c.feed(&[0; QUOTA + 1]).is_err());
        assert_eq!(c.incoming, 0);
        assert!(c.drain(&mut [0; 4096]).is_err());
        let mut c = test_client();
        c.incoming = QUOTA;
        assert_eq!(c.feed(&[]), Ok(0));
        assert!(c.feed(&[0]).is_err());
        let mut c = test_client();
        c.incoming = usize::MAX;
        assert!(c.feed(&[0]).is_err());
        let mut c = test_client();
        c.outgoing = usize::MAX;
        assert!(c.drain(&mut [0; 1]).is_err());
        let mut c = test_client();
        c.outgoing = QUOTA - 1;
        assert_eq!(c.drain(&mut [0; 1]), Ok(1));
        assert!(c.drain(&mut [0; 1]).is_err());
        let mut c = test_client();
        let mut total = 0;
        while c.connection.wants_write() {
            let n = c.drain(&mut [0; 1]).unwrap();
            assert_eq!(n, 1);
            total += n;
        }
        assert_eq!(c.outgoing, total);
        // Repeated partial TLS record input accumulates across calls.
        let mut c = test_client();
        c.incoming = QUOTA - 5;
        for b in [22, 3, 3, 0, 100] {
            assert_eq!(c.feed(&[b]), Ok(1));
        }
        assert_eq!(c.incoming, QUOTA);
        assert!(c.feed(&[0]).is_err());
        let _ = ProbeTime;
    }
}
