//! Cluster-root TLS adapters. Registry authorization is local applied state and may
//! be stale without bound. Callers must move handshakes off runtime workers: the
//! store mutex wait is unbounded. No sockets or production transport live here.
use super::{MAX_CSR, parse_node_uri};
use crate::cluster::enrollment::{ClusterId, KeyDigest, NodeId};
use rustls::{
    CertificateError, ClientConfig, ClientConnection, DigitallySignedStruct, DistinguishedName,
    Error, OtherError, ProtocolVersion, RootCertStore, ServerConfig, ServerConnection,
    SignatureScheme,
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    crypto::{CryptoProvider, ring, verify_tls13_signature},
    pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime},
    server::{
        ParsedCertificate, WebPkiClientVerifier,
        danger::{ClientCertVerified, ClientCertVerifier},
    },
    time_provider::TimeProvider,
};
use std::{fmt, sync::Arc};
use x509_parser::{
    extensions::{GeneralName, ParsedExtension},
    prelude::*,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerifyRefusal {
    Oversized,
    Intermediates,
    Malformed,
    WrongCluster,
    NotMember,
    KeyMismatch,
    TargetMismatch,
    Unavailable,
    Incomplete,
    Tls12,
}
impl fmt::Display for VerifyRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for VerifyRefusal {}
impl From<VerifyRefusal> for Error {
    fn from(r: VerifyRefusal) -> Self {
        Error::InvalidCertificate(CertificateError::Other(OtherError(Arc::new(r))))
    }
}
pub type Registry =
    Arc<dyn Fn(ClusterId, NodeId, KeyDigest) -> Result<(), VerifyRefusal> + Send + Sync>;
#[derive(Clone, Copy)]
struct Identity {
    cluster: ClusterId,
    node: NodeId,
    key: KeyDigest,
}
fn peer_identity(der: &[u8]) -> Result<Identity, VerifyRefusal> {
    if der.len() > MAX_CSR {
        return Err(VerifyRefusal::Oversized);
    }
    let (rest, cert) = X509Certificate::from_der(der).map_err(|_| VerifyRefusal::Malformed)?;
    if !rest.is_empty() {
        return Err(VerifyRefusal::Malformed);
    }
    let mut sans = cert
        .extensions()
        .iter()
        .filter(|e| e.oid == x509_parser::oid_registry::OID_X509_EXT_SUBJECT_ALT_NAME);
    let san = sans.next().ok_or(VerifyRefusal::Malformed)?;
    if sans.next().is_some() {
        return Err(VerifyRefusal::Malformed);
    }
    let ParsedExtension::SubjectAlternativeName(san) = san.parsed_extension() else {
        return Err(VerifyRefusal::Malformed);
    };
    let [GeneralName::URI(uri)] = san.general_names.as_slice() else {
        return Err(VerifyRefusal::Malformed);
    };
    let (cluster, node) = parse_node_uri(uri).ok_or(VerifyRefusal::Malformed)?;
    let key = KeyDigest::of_spki(cert.public_key().raw).map_err(|_| VerifyRefusal::Malformed)?;
    Ok(Identity { cluster, node, key })
}
pub struct PeerVerifier {
    cluster: ClusterId,
    roots: Arc<RootCertStore>,
    provider: Arc<CryptoProvider>,
    inner: Arc<dyn ClientCertVerifier>,
    registry: Registry,
    expected: Option<NodeId>,
}
impl fmt::Debug for PeerVerifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PeerVerifier")
    }
}
impl PeerVerifier {
    pub fn new(
        root: CertificateDer<'static>,
        cluster: ClusterId,
        registry: Registry,
    ) -> Result<Self, Error> {
        let mut roots = RootCertStore::empty();
        roots.add(root)?;
        let roots = Arc::new(roots);
        let provider = Arc::new(ring::default_provider());
        let inner = WebPkiClientVerifier::builder_with_provider(roots.clone(), provider.clone())
            .build()
            .map_err(|_| VerifyRefusal::Malformed)?;
        Ok(Self {
            cluster,
            roots,
            provider,
            inner,
            registry,
            expected: None,
        })
    }
    pub fn client(mut self, expected: NodeId) -> Arc<Self> {
        self.expected = Some(expected);
        Arc::new(self)
    }
    pub fn server(self) -> Arc<Self> {
        Arc::new(self)
    }
    fn bounds(
        &self,
        ee: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
    ) -> Result<(), Error> {
        if !intermediates.is_empty() {
            return Err(VerifyRefusal::Intermediates.into());
        }
        if ee.len() > MAX_CSR {
            return Err(VerifyRefusal::Oversized.into());
        }
        Ok(())
    }
    fn authorize(&self, ee: &CertificateDer<'_>, expected: Option<NodeId>) -> Result<(), Error> {
        let id = peer_identity(ee)?;
        if id.cluster != self.cluster {
            return Err(VerifyRefusal::WrongCluster.into());
        }
        (self.registry)(id.cluster, id.node, id.key)?;
        if expected.is_some_and(|n| n != id.node) {
            return Err(VerifyRefusal::TargetMismatch.into());
        }
        Ok(())
    }
}
impl ServerCertVerifier for PeerVerifier {
    fn verify_server_cert(
        &self,
        ee: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        self.bounds(ee, intermediates)?;
        rustls::client::verify_server_cert_signed_by_trust_anchor(
            &ParsedCertificate::try_from(ee)?,
            &self.roots,
            &[],
            now,
            self.provider.signature_verification_algorithms.all,
        )?;
        self.authorize(
            ee,
            Some(self.expected.ok_or(VerifyRefusal::TargetMismatch)?),
        )?;
        Ok(ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        _: &[u8],
        _: &CertificateDer<'_>,
        _: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        Err(VerifyRefusal::Tls12.into())
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
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
impl ClientCertVerifier for PeerVerifier {
    fn offer_client_auth(&self) -> bool {
        true
    }
    fn client_auth_mandatory(&self) -> bool {
        true
    }
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        self.inner.root_hint_subjects()
    }
    fn verify_client_cert(
        &self,
        ee: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        now: UnixTime,
    ) -> Result<ClientCertVerified, Error> {
        self.bounds(ee, intermediates)?;
        self.inner.verify_client_cert(ee, &[], now)?;
        self.authorize(ee, None)?;
        Ok(ClientCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        _: &[u8],
        _: &CertificateDer<'_>,
        _: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        Err(VerifyRefusal::Tls12.into())
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
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
pub fn client_config(
    verifier: Arc<PeerVerifier>,
    cert: CertificateDer<'static>,
    key: PrivateKeyDer<'static>,
    time: Arc<dyn TimeProvider>,
) -> Result<ClientConfig, Error> {
    let mut config = ClientConfig::builder_with_details(verifier.provider.clone(), time)
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_client_auth_cert(vec![cert], key)?;
    config.enable_sni = false;
    // Resumed handshakes skip certificate verification, including local revocation.
    config.resumption = rustls::client::Resumption::disabled();
    Ok(config)
}
pub fn server_config(
    verifier: Arc<PeerVerifier>,
    cert: CertificateDer<'static>,
    key: PrivateKeyDer<'static>,
    time: Arc<dyn TimeProvider>,
) -> Result<ServerConfig, Error> {
    let mut config = ServerConfig::builder_with_details(verifier.provider.clone(), time)
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .with_client_cert_verifier(verifier)
        .with_single_cert(vec![cert], key)?;
    config.session_storage = Arc::new(rustls::server::NoServerSessionStorage {});
    config.send_tls13_tickets = 0;
    Ok(config)
}
pub const SERVER_NAME: &str = "arxcovo.invalid";
/// Trusted adapter for this module's reviewed configs; arbitrary insecure rustls
/// configs in the same crate can forge evidence. This is not a capability fence.
#[derive(Clone, Copy)]
pub struct AuthenticatedPeer(NodeId);
impl AuthenticatedPeer {
    pub fn node(self) -> NodeId {
        self.0
    }
    pub fn from_client(c: &ClientConnection) -> Result<Self, VerifyRefusal> {
        if c.is_handshaking() || c.protocol_version() != Some(ProtocolVersion::TLSv1_3) {
            return Err(VerifyRefusal::Incomplete);
        }
        Self::from_verified_certificates(c.peer_certificates())
    }
    pub fn from_server(c: &ServerConnection) -> Result<Self, VerifyRefusal> {
        if c.is_handshaking() || c.protocol_version() != Some(ProtocolVersion::TLSv1_3) {
            return Err(VerifyRefusal::Incomplete);
        }
        Self::from_verified_certificates(c.peer_certificates())
    }
    fn from_verified_certificates(
        certs: Option<&[CertificateDer<'_>]>,
    ) -> Result<Self, VerifyRefusal> {
        let certs = certs.ok_or(VerifyRefusal::Malformed)?;
        let [cert] = certs else {
            return Err(VerifyRefusal::Malformed);
        };
        Ok(Self(peer_identity(cert)?.node))
    }
    /// Placeholder founder only: confers no certificate verification credit.
    #[cfg(test)]
    pub fn assume(node: NodeId) -> Self {
        Self(node)
    }
}
pub fn check_sender(authenticated: NodeId, claimed: u64) -> Result<(), VerifyRefusal> {
    if authenticated.get() == claimed {
        Ok(())
    } else {
        Err(VerifyRefusal::TargetMismatch)
    }
}
