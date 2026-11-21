//! Private D4 TLS channels. The service owns requests and execution.
use super::{
    enrollment::{ClusterId, NodeId},
    pki::verify::{AuthenticatedPeer, SERVER_NAME},
};
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::net::TcpStream;
use tokio_rustls::{TlsAcceptor, TlsConnector};

#[cfg(test)]
mod tests;
pub(crate) mod wire;

pub(crate) const IO_BUDGET: Duration = Duration::from_secs(5);
const PROTOCOL: &[u8] = b"arxcovo-build/1";

#[derive(Clone, Copy)]
pub(crate) struct Identity {
    pub(crate) cluster: ClusterId,
    pub(crate) node: NodeId,
    pub(crate) key: [u8; 32],
}

pub(crate) trait Authority: Send + Sync {
    fn identity(&self) -> Option<(ClusterId, NodeId)>;
    fn binding(&self, cluster: ClusterId, node: NodeId) -> Option<[u8; 32]>;
    fn client(&self, target: NodeId) -> Option<Arc<rustls::ClientConfig>>;
    fn server(&self) -> Option<Arc<rustls::ServerConfig>>;
}

fn checked(peer: AuthenticatedPeer, authority: &dyn Authority) -> Option<Identity> {
    let (cluster, node, key) = peer.authenticated_identity().ok()?;
    let key = *key.as_bytes();
    (authority.binding(cluster, node)? == key).then_some(Identity { cluster, node, key })
}

pub(crate) async fn call(
    authority: &dyn Authority,
    address: SocketAddr,
    target_key: [u8; 32],
    request: wire::Envelope,
    budget: Duration,
) -> Option<wire::Response> {
    let (cluster, source) = authority.identity()?;
    if cluster != request.cluster
        || source != request.source
        || source == request.target
        || authority.binding(cluster, request.target)? != target_key
    {
        return None;
    }
    let target = request.target;
    let mut stream = tokio::time::timeout(IO_BUDGET, async {
        let config = authority.client(target)?;
        let connection = TcpStream::connect(address).await.ok()?;
        let name = rustls::pki_types::ServerName::try_from(SERVER_NAME).ok()?;
        let stream = TlsConnector::from(config)
            .connect(name, connection)
            .await
            .ok()?;
        let tls = stream.get_ref().1;
        let peer = checked(AuthenticatedPeer::from_client(tls).ok()?, authority)?;
        if tls.alpn_protocol()? != PROTOCOL
            || peer.cluster != cluster
            || peer.node != target
            || peer.key != target_key
        {
            return None;
        }
        Some(stream)
    })
    .await
    .ok()??;
    tokio::time::timeout(IO_BUDGET, wire::send(&mut stream, &request))
        .await
        .ok()?
        .ok()?;
    let reply: wire::Receipt = tokio::time::timeout(budget, wire::receive(&mut stream))
        .await
        .ok()?
        .ok()?;
    (reply.version == wire::VERSION
        && reply.cluster == cluster
        && reply.target == source
        && reply.source == target
        && authority.binding(cluster, target)? == target_key)
        .then_some(reply.result)
}

pub(crate) async fn accept(
    stream: TcpStream,
    authority: &dyn Authority,
) -> Option<(
    tokio_rustls::server::TlsStream<TcpStream>,
    Identity,
    wire::Envelope,
)> {
    tokio::time::timeout(IO_BUDGET, async {
        let (cluster, node) = authority.identity()?;
        let mut stream = TlsAcceptor::from(authority.server()?)
            .accept(stream)
            .await
            .ok()?;
        let tls = stream.get_ref().1;
        if tls.alpn_protocol()? != PROTOCOL {
            return None;
        }
        let peer = checked(AuthenticatedPeer::from_server(tls).ok()?, authority)?;
        let envelope: wire::Envelope = wire::receive(&mut stream).await.ok()?;
        (envelope.version == wire::VERSION
            && envelope.cluster == cluster
            && peer.cluster == cluster
            && envelope.source == peer.node
            && envelope.target == node
            && peer.node != node)
            .then_some((stream, peer, envelope))
    })
    .await
    .ok()?
}
