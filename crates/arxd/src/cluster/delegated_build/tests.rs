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
    enrollment::{ClusterId, Command, FoundMembers, KeyDigest, NodeId},
    membership::Membership,
    pki::{node_uri, verify},
    state_store::{Entry, Init, LogPos, MembershipBlob, StateStore},
};
use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, ExtendedKeyUsagePurpose, IsCa, Issuer,
    KeyPair, KeyUsagePurpose, PublicKeyData, SanType,
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use std::sync::{Arc, Mutex};

struct Peer {
    node: NodeId,
    root: CertificateDer<'static>,
    leaf: CertificateDer<'static>,
    key: PrivateKeyDer<'static>,
    bindings: [[u8; 32]; 2],
    snapshot: Arc<Mutex<StateStore>>,
}

impl Peer {
    fn verifier(&self) -> verify::PeerVerifier {
        let snapshot = self.snapshot.clone();
        verify::PeerVerifier::new(
            self.root.clone(),
            [4; 16],
            Arc::new(move |cluster, node, key| {
                (super::applied_binding(&snapshot.lock().unwrap(), cluster, node) == Some(key))
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
        super::applied_binding(&self.snapshot.lock().unwrap(), cluster, node)
            .map(|key| *key.as_bytes())
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

fn pos(index: u64) -> LogPos {
    LogPos {
        index,
        term: 1,
        leader: 1,
    }
}

fn membership(joint: bool) -> MembershipBlob {
    let voters = if joint {
        vec![[1].into(), [1, 2].into()]
    } else {
        vec![[1].into()]
    };
    MembershipBlob::new(
        Membership::new(
            voters,
            [(1, "127.0.0.1:42001".into()), (2, "127.0.0.1:42002".into())].into(),
        )
        .unwrap()
        .encode()
        .unwrap(),
    )
    .unwrap()
}

fn peers(root_dir: &std::path::Path) -> [Peer; 2] {
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
    let mut snapshot = StateStore::open(
        root_dir,
        Init {
            cluster: [4; 16],
            first_free: NodeId::new(3).unwrap(),
        },
    )
    .unwrap();
    // Synthetic state-machine entries. No retained job, Found operation or Engine.
    snapshot
        .apply(
            pos(0),
            &Entry::Enrollment(Command::Found {
                cluster: [4; 16],
                members: FoundMembers::new(keys.iter().enumerate().map(|(i, k)| {
                    (
                        NodeId::new(i as u64 + 1).unwrap(),
                        KeyDigest::of_spki(&k.subject_public_key_info()).unwrap(),
                    )
                }))
                .unwrap(),
                proposed_at_ms: 1,
            }),
        )
        .unwrap();
    assert!(super::applied_binding(&snapshot, [4; 16], NodeId::new(2).unwrap()).is_none());
    snapshot
        .apply(pos(1), &Entry::Membership(membership(false)))
        .unwrap();
    let snapshot = Arc::new(Mutex::new(snapshot));
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
            snapshot: snapshot.clone(),
        }
    })
}

#[tokio::test]
async fn actual_mutual_tls_checks_peer_identity_and_status_scope() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let [source, target] = peers(dir.path());
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
    {
        let mut applied = source.snapshot.lock().unwrap();
        applied
            .apply(pos(2), &Entry::Membership(membership(true)))
            .unwrap();
        assert!(super::applied_binding(&applied, [4; 16], source.node).is_none());
        applied
            .apply(pos(3), &Entry::Membership(membership(false)))
            .unwrap();
        assert!(super::applied_binding(&applied, [4; 16], target.node).is_some());
        applied
            .apply(
                pos(4),
                &Entry::Enrollment(Command::Remove {
                    cluster: [4; 16],
                    node: target.node,
                    proposed_at_ms: 2,
                }),
            )
            .unwrap();
    }
    assert!(
        call(&source, address, source.bindings[1], request(), IO_BUDGET)
            .await
            .is_none()
    );
}
