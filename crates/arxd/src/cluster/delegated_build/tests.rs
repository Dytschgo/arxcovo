use super::wire::*;

#[test]
fn descriptor_cannot_name_a_host_path_or_unbounded_command() {
    let mut descriptor = Descriptor {
        workspace: "ci-build".into(),
        tool: "cargo".into(),
        args: vec!["build".into(), "--offline".into()],
        timeout_s: 30,
        output_bytes: 512,
    };
    assert_eq!(descriptor.request().unwrap().cwd, "");
    descriptor.workspace = "../ci-build".into();
    assert!(descriptor.request().is_none());
    descriptor.workspace = "ci-build".into();
    descriptor.tool = "/usr/bin/cargo".into();
    assert!(descriptor.request().is_none());
}

use super::{Authority, IO_BUDGET, accept, call};
use crate::cluster::{
    enrollment::{ClusterId, KeyDigest, NodeId},
    pki::{node_uri, verify},
};
use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, ExtendedKeyUsagePurpose, IsCa, Issuer,
    KeyPair, KeyUsagePurpose, PublicKeyData, SanType,
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

struct Peer {
    node: NodeId,
    root: CertificateDer<'static>,
    leaf: CertificateDer<'static>,
    key: PrivateKeyDer<'static>,
    bindings: [[u8; 32]; 2],
    revoked: Arc<AtomicBool>,
}

impl Peer {
    fn verifier(&self) -> verify::PeerVerifier {
        let bindings = self.bindings;
        let revoked = self.revoked.clone();
        verify::PeerVerifier::new(
            self.root.clone(),
            [4; 16],
            Arc::new(move |cluster, node, key| {
                let allowed = cluster == [4; 16]
                    && (1..=2).contains(&node.get())
                    && !(node.get() == 2 && revoked.load(Ordering::Acquire))
                    && bindings[node.get() as usize - 1] == *key.as_bytes();
                allowed
                    .then_some(())
                    .ok_or(verify::VerifyRefusal::NotMember)
            }),
        )
        .unwrap()
    }
}

impl Authority for Peer {
    fn identity(&self) -> Option<(ClusterId, NodeId)> {
        Some(([4; 16], self.node))
    }
    fn binding(&self, cluster: ClusterId, node: NodeId) -> Option<[u8; 32]> {
        if cluster != [4; 16]
            || !(1..=2).contains(&node.get())
            || (node.get() == 2 && self.revoked.load(Ordering::Acquire))
        {
            return None;
        }
        Some(self.bindings[node.get() as usize - 1])
    }
    fn client(&self, target: NodeId) -> Option<Arc<rustls::ClientConfig>> {
        let mut config = verify::client_config(
            self.verifier().client(target),
            self.leaf.clone(),
            self.key.clone_key(),
            Arc::new(rustls::time_provider::DefaultTimeProvider),
        )
        .ok()?;
        config.alpn_protocols = vec![b"arxcovo-build/1".to_vec()];
        Some(Arc::new(config))
    }
    fn server(&self) -> Option<Arc<rustls::ServerConfig>> {
        let mut config = verify::server_config(
            self.verifier().server(),
            self.leaf.clone(),
            self.key.clone_key(),
            Arc::new(rustls::time_provider::DefaultTimeProvider),
        )
        .ok()?;
        config.alpn_protocols = vec![b"arxcovo-build/1".to_vec()];
        Some(Arc::new(config))
    }
}

fn peers() -> [Peer; 2] {
    let authority = KeyPair::generate().unwrap();
    let mut ca = CertificateParams::default();
    ca.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
    ca.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    let root = ca.self_signed(&authority).unwrap().der().clone();
    let issuer = Issuer::new(ca, authority);
    let keys = std::array::from_fn::<_, 2, _>(|_| KeyPair::generate().unwrap());
    let bindings = keys.each_ref().map(|key| {
        *KeyDigest::of_spki(&key.subject_public_key_info())
            .unwrap()
            .as_bytes()
    });
    let revoked = Arc::new(AtomicBool::new(false));
    let mut ids = 1..=2;
    keys.map(|key| {
        let node = NodeId::new(ids.next().unwrap()).unwrap();
        let mut leaf = CertificateParams::default();
        leaf.distinguished_name = DistinguishedName::new();
        leaf.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        leaf.extended_key_usages = vec![
            ExtendedKeyUsagePurpose::ServerAuth,
            ExtendedKeyUsagePurpose::ClientAuth,
        ];
        leaf.subject_alt_names = vec![SanType::URI(node_uri([4; 16], node).try_into().unwrap())];
        let certificate = leaf.signed_by(&key, &issuer).unwrap();
        Peer {
            node,
            root: root.clone(),
            leaf: certificate.der().clone(),
            key: PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
            bindings,
            revoked: revoked.clone(),
        }
    })
}

#[tokio::test]
async fn actual_mutual_tls_checks_peer_identity_and_status_scope() {
    let [source, target] = peers();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let request = || Envelope {
        version: VERSION,
        cluster: [4; 16],
        source: source.node,
        target: target.node,
        agent: "fixture".into(),
        query: Query::Status { operation: [5; 32] },
    };
    let server = async {
        let (stream, _) = listener.accept().await.unwrap();
        let (mut stream, peer, envelope) = accept(stream, &target).await.unwrap();
        assert_eq!(peer.key, source.bindings[0]);
        assert_eq!(peer.node, source.node);
        assert_eq!(envelope.agent, "fixture");
        assert!(matches!(envelope.query, Query::Status { operation } if operation == [5; 32]));
        send(
            &mut stream,
            &Receipt {
                version: VERSION,
                cluster: [4; 16],
                source: target.node,
                target: source.node,
                result: Response::Unknown { operation: [5; 32] },
            },
        )
        .await
        .unwrap();
    };
    let (_, result) = tokio::join!(
        server,
        call(&source, address, source.bindings[1], request(), IO_BUDGET)
    );
    assert!(matches!(result, Some(Response::Unknown { operation }) if operation == [5; 32]));
    assert!(
        call(&source, address, [9; 32], request(), IO_BUDGET)
            .await
            .is_none()
    );
    source.revoked.store(true, Ordering::Release);
    assert!(
        call(&source, address, source.bindings[1], request(), IO_BUDGET)
            .await
            .is_none()
    );
}
