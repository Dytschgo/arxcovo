//! Private bootstrap client. Chain trust alone grants no membership authority.
use super::verify::{SERVER_NAME, peer_identity};
use crate::cluster::enrollment::{
    BoundVoters, ClusterId, JoinAnchor, JoinCode, RequestId,
    session::{self, QuotaSocket, Reply, SOCKET_BUDGET, SocketLimits},
};
use rustls::{
    ClientConfig, DigitallySignedStruct, Error, HandshakeKind, RootCertStore, SignatureScheme,
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    crypto::{CryptoProvider, ring, verify_tls13_signature},
    pki_types::{CertificateDer, ServerName, UnixTime},
    server::ParsedCertificate,
    time_provider::TimeProvider,
};
use sha2::{Digest, Sha256};
use std::{net::SocketAddr, sync::Arc};
use tokio::{
    io::AsyncWriteExt,
    net::TcpStream,
    time::{Instant, timeout_at},
};
use tokio_rustls::TlsConnector;

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
fn client_config(
    code: &JoinCode,
    root: &[u8],
    time: Arc<dyn TimeProvider>,
) -> Result<ClientConfig, Refused> {
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
    Ok(config)
}

#[cfg(test)]
pub(in crate::cluster) fn test_config(
    code: &JoinCode,
    root: &[u8],
    time: Arc<dyn TimeProvider>,
) -> ClientConfig {
    let mut config = client_config(code, root, time).unwrap();
    config.resumption = rustls::client::Resumption::in_memory_sessions(8);
    config
}

fn check_voters(
    connection: &rustls::ClientConnection,
    cluster: ClusterId,
    anchor: JoinAnchor,
    voters: &BoundVoters,
) -> Result<(), Refused> {
    if voters.anchor(cluster) != anchor {
        return Err(Refused);
    }
    let Some([cert]) = connection.peer_certificates() else {
        return Err(Refused);
    };
    let identity = peer_identity(cert).map_err(|_| Refused)?;
    if identity.cluster != cluster || !voters.contains(identity.node, identity.key) {
        return Err(Refused);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::cluster) enum ExchangeError {
    Refused,
    PossibleEffect,
}

// Constructed only by the consuming client after the Result and clean end.
pub(in crate::cluster) struct Completed {
    voters: BoundVoters,
    reply: Reply,
    #[cfg(target_os = "linux")]
    binding: JoinBinding,
}

#[cfg(target_os = "linux")]
#[derive(PartialEq, Eq)]
pub(in crate::cluster) struct JoinBinding {
    pub cluster: ClusterId,
    pub code: crate::cluster::enrollment::CodeId,
    pub root: [u8; 32],
    pub request: RequestId,
    pub csr: [u8; 32],
}

impl Completed {
    pub(in crate::cluster) fn voters(&self) -> &BoundVoters {
        &self.voters
    }

    pub(in crate::cluster) fn reply(&self) -> &Reply {
        &self.reply
    }

    #[cfg(target_os = "linux")]
    pub(in crate::cluster) fn for_join(self, expected: &JoinBinding) -> Result<Self, Refused> {
        if &self.binding != expected {
            return Err(Refused);
        }
        Ok(self)
    }
}

/// Sole client continuation. It owns the socket and cannot be cloned or resumed.
pub(in crate::cluster) struct AnchoredClient {
    socket: tokio_rustls::client::TlsStream<QuotaSocket>,
    code: JoinCode,
    voters: BoundVoters,
    challenge: [u8; 16],
    deadline: Instant,
}

impl AnchoredClient {
    pub(in crate::cluster) async fn redeem(
        self,
        request: RequestId,
        csr: &[u8],
    ) -> Result<Completed, ExchangeError> {
        let Self {
            mut socket,
            code,
            voters,
            challenge,
            deadline,
        } = self;
        let frame = session::redeem_frame(&code, request, challenge, csr)
            .map_err(|_| ExchangeError::Refused)?;
        if Instant::now() >= deadline {
            return Err(ExchangeError::Refused);
        }
        session::nothing_pending(&mut socket)
            .await
            .map_err(|_| ExchangeError::Refused)?;
        if Instant::now() >= deadline {
            return Err(ExchangeError::Refused);
        }

        // From the first attempted write onward, errors may hide an issuer effect.
        timeout_at(deadline, async {
            socket.write_all(&frame).await.map_err(|_| Refused)?;
            socket.shutdown().await.map_err(|_| Refused)?;
            let reply = session::receive_result(&mut socket).await?;
            if Instant::now() >= deadline {
                return Err(Refused);
            }
            Ok(Completed {
                voters,
                reply,
                #[cfg(target_os = "linux")]
                binding: JoinBinding {
                    cluster: code.cluster(),
                    code: code.code_id(),
                    root: code.ca_fingerprint(),
                    request,
                    csr: Sha256::digest(csr).into(),
                },
            })
        })
        .await
        .map_err(|_| ExchangeError::PossibleEffect)?
        .map_err(|_| ExchangeError::PossibleEffect)
    }
}

pub(in crate::cluster) async fn anchor(
    address: SocketAddr,
    code: &JoinCode,
    root: &[u8],
    time: Arc<dyn TimeProvider>,
    limits: SocketLimits,
) -> Result<AnchoredClient, ExchangeError> {
    let deadline = Instant::now() + SOCKET_BUDGET;
    let config = client_config(code, root, time).map_err(|_| ExchangeError::Refused)?;
    timeout_at(deadline, async {
        let tcp = TcpStream::connect(address).await.map_err(|_| Refused)?;
        tcp.set_nodelay(true).map_err(|_| Refused)?;
        let mut socket = TlsConnector::from(Arc::new(config))
            .connect(
                ServerName::try_from(SERVER_NAME).map_err(|_| Refused)?,
                QuotaSocket::new(tcp, limits, deadline),
            )
            .await
            .map_err(|_| Refused)?;
        if socket.get_ref().1.handshake_kind() != Some(HandshakeKind::Full) {
            return Err(Refused);
        }
        socket
            .write_all(&session::query_frame(code.cluster(), code.code_id()))
            .await
            .map_err(|_| Refused)?;
        socket.flush().await.map_err(|_| Refused)?;
        let (challenge, voters) = session::receive_anchor(&mut socket).await?;
        check_voters(socket.get_ref().1, code.cluster(), code.anchor(), &voters)?;
        session::nothing_pending(&mut socket).await?;
        if Instant::now() >= deadline {
            return Err(Refused);
        }
        Ok(AnchoredClient {
            socket,
            code: code.clone(),
            voters,
            challenge,
            deadline,
        })
    })
    .await
    .map_err(|_| ExchangeError::Refused)?
    .map_err(|_: Refused| ExchangeError::Refused)
}

/// Compatibility entry for the existing closed-exchange callers.
pub(in crate::cluster) async fn exchange(
    address: SocketAddr,
    code: &JoinCode,
    root: &[u8],
    time: Arc<dyn TimeProvider>,
    request: RequestId,
    csr: &[u8],
    limits: SocketLimits,
) -> Result<Completed, ExchangeError> {
    anchor(address, code, root, time, limits)
        .await?
        .redeem(request, csr)
        .await
}

// Preserve the in-memory probe for the existing frozen test callers.
#[cfg(test)]
use crate::cluster::enrollment::CodeId;
#[cfg(test)]
use rustls::{ClientConnection, ProtocolVersion};
#[cfg(test)]
use std::io::{self, Read, Write};

#[cfg(test)]
const QUOTA: usize = 128 * 1024;
#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Initial,
    Queried,
    Anchored,
    Released,
    Fatal,
}
#[cfg(test)]
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
#[cfg(test)]
impl PreproofClient {
    pub(in crate::cluster) fn new(
        code: &JoinCode,
        root: &[u8],
        time: Arc<dyn TimeProvider>,
    ) -> Result<Self, Refused> {
        let config = client_config(code, root, time)?;
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
        if check_voters(&self.connection, self.cluster, self.anchor, &voters).is_err() {
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
#[cfg(test)]
struct SliceWriter<'a> {
    buffer: &'a mut [u8],
    copied: usize,
}
#[cfg(test)]
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
    use super::*;
    use crate::cluster::pki::preproof_tests::{ProbeTime, prepared, test_client};
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
