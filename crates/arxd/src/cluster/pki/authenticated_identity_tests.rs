//! Fixed-size retained identity evidence; no engine or transport acceptance.
use super::*;
use crate::cluster::{
    enrollment::{ClusterId, Command, EnrollmentState, FoundMembers, Outcome, PeerRefusal},
    state_store::LogPos,
};
use rcgen::{BasicConstraints, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyUsagePurpose, SanType};

// Returns booleans only: designated mutation assertions run after fixtures drop.
fn retained_fixture() -> (bool, bool, bool, bool) {
    let ca = ClusterCa::ephemeral([1; 16], NOW);
    let key = KeyPair::generate().unwrap();
    let expected = KeyDigest::of_spki(&key.subject_public_key_info()).unwrap();
    let der = leaf(&ca, &key);
    let (c, s) = connections(&ca, &der, &key, registry(&key), node(4), 1001);
    let incomplete = matches!(
        AuthenticatedPeer::from_client(&c),
        Err(VerifyRefusal::Incomplete)
    ) && matches!(
        AuthenticatedPeer::from_server(&s),
        Err(VerifyRefusal::Incomplete)
    );
    let (c, s) = complete(c, s).unwrap();
    let a = AuthenticatedPeer::from_client(&c)
        .unwrap()
        .authenticated_identity();
    let b = AuthenticatedPeer::from_server(&s)
        .unwrap()
        .authenticated_identity();
    let cluster = [a, b]
        .iter()
        .all(|id| matches!(id, Ok((cluster, _, _)) if *cluster == [1; 16]));
    let nodes = [a, b]
        .iter()
        .all(|id| matches!(id, Ok((_, n, _)) if *n == node(4)));
    let keys = [a, b]
        .iter()
        .all(|id| matches!(id, Ok((_, _, k)) if *k == expected));
    (cluster, nodes, keys, incomplete)
}

fn second_cluster_fixture() -> bool {
    let cluster = [3; 16];
    let mut params = CertificateParams::default();
    params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
    params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    let ca_key = KeyPair::generate().unwrap();
    let ca = params.self_signed(&ca_key).unwrap();
    let issuer = Issuer::new(params, ca_key);
    let key = KeyPair::generate().unwrap();
    let expected = KeyDigest::of_spki(&key.subject_public_key_info()).unwrap();
    let mut params = CertificateParams::default();
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![
        ExtendedKeyUsagePurpose::ServerAuth,
        ExtendedKeyUsagePurpose::ClientAuth,
    ];
    params.subject_alt_names = vec![SanType::URI(
        crate::cluster::pki::node_uri(cluster, node(4))
            .try_into()
            .unwrap(),
    )];
    let leaf = params.signed_by(&key, &issuer).unwrap();
    let registry: Registry = Arc::new(move |c, n, k| {
        if c != cluster {
            Err(VerifyRefusal::WrongCluster)
        } else if n != node(4) {
            Err(VerifyRefusal::NotMember)
        } else if k != expected {
            Err(VerifyRefusal::KeyMismatch)
        } else {
            Ok(())
        }
    });
    let client = client_config(
        PeerVerifier::new(ca.der().clone(), cluster, registry.clone())
            .unwrap()
            .client(node(4)),
        leaf.der().clone(),
        PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
        Arc::new(FixedTime(1_800_000_000)),
    )
    .unwrap();
    let server = server_config(
        PeerVerifier::new(ca.der().clone(), cluster, registry)
            .unwrap()
            .server(),
        leaf.der().clone(),
        PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
        Arc::new(FixedTime(1_800_000_000)),
    )
    .unwrap();
    let (c, s) = complete(
        ClientConnection::new(Arc::new(client), ServerName::try_from(SERVER_NAME).unwrap())
            .unwrap(),
        ServerConnection::new(Arc::new(server)).unwrap(),
    )
    .unwrap();
    [
        AuthenticatedPeer::from_client(&c),
        AuthenticatedPeer::from_server(&s),
    ]
    .iter()
    .all(|peer| {
        peer.as_ref().is_ok_and(|peer| {
            peer.authenticated_identity()
                .is_ok_and(|(c, n, k)| c == cluster && n == node(4) && k == expected)
        })
    })
}

#[test]
fn peer_identity_retains_verified_tuple() {
    let (cluster, nodes, keys, incomplete) = retained_fixture();
    let second = second_cluster_fixture();
    assert!(
        cluster,
        "actual first-cluster identity retained on both handles"
    );
    assert!(nodes, "actual node retained on both handles");
    assert!(keys, "independently hashed SPKI retained on both handles");
    assert!(incomplete, "incomplete connections refuse identity");
    assert!(
        second,
        "genuine second-cluster identity retained on both handles"
    );
}

fn founded(cluster: ClusterId, pairs: [(NodeId, KeyDigest); 2]) -> (EnrollmentState, bool) {
    let mut state = EnrollmentState::new(cluster, node(10));
    let outcome = state.apply(
        &Command::Found {
            cluster,
            members: FoundMembers::new(pairs).unwrap(),
            proposed_at_ms: NOW,
        },
        LogPos {
            term: 1,
            leader: 4,
            index: 1,
        },
        None,
    );
    (state, matches!(outcome, Outcome::Founded { replay: false }))
}

fn binding_controls_fixture() -> [bool; 10] {
    let ca = ClusterCa::ephemeral([1; 16], NOW);
    let key = KeyPair::generate().unwrap();
    let der = leaf(&ca, &key);
    let (c, s) = connections(&ca, &der, &key, registry(&key), node(4), 1001);
    let (c, s) = complete(c, s).unwrap();
    let peers = [
        AuthenticatedPeer::from_client(&c).unwrap(),
        AuthenticatedPeer::from_server(&s).unwrap(),
    ];
    let digest = KeyDigest::of_spki(&key.subject_public_key_info()).unwrap();
    let other = KeyPair::generate().unwrap();
    let other_digest = KeyDigest::of_spki(&other.subject_public_key_info()).unwrap();
    let third = KeyPair::generate().unwrap();
    let third_digest = KeyDigest::of_spki(&third.subject_public_key_info()).unwrap();
    let (mut right, f1) = founded([1; 16], [(node(4), digest), (node(5), other_digest)]);
    let (wrong_cluster, f2) = founded([2; 16], [(node(4), digest), (node(5), other_digest)]);
    let (wrong_key, f3) = founded([1; 16], [(node(4), third_digest), (node(5), other_digest)]);
    let (missing, f4) = founded([1; 16], [(node(5), other_digest), (node(6), third_digest)]);
    let check = |state: &EnrollmentState, expected: Result<(), PeerRefusal>| {
        peers.iter().all(|p| {
            p.authenticated_identity()
                .is_ok_and(|(c, n, k)| state.authorize_peer(c, n, k) == expected)
        })
    };
    let authorized = check(&right, Ok(()));
    let foreign = check(&wrong_cluster, Err(PeerRefusal::WrongCluster));
    let mismatch = check(&wrong_key, Err(PeerRefusal::KeyMismatch));
    let absent = check(&missing, Err(PeerRefusal::NotMember));
    let removed = right.apply(
        &Command::Remove {
            cluster: [1; 16],
            node: node(4),
            proposed_at_ms: NOW,
        },
        LogPos {
            term: 1,
            leader: 4,
            index: 2,
        },
        None,
    );
    let removal = matches!(removed, Outcome::Removed { replay: false });
    let revoked = check(&right, Err(PeerRefusal::NotMember));
    [
        f1, f2, f3, f4, authorized, foreign, mismatch, absent, removal, revoked,
    ]
}

#[test]
fn peer_identity_fresh_binding_controls() {
    let results = binding_controls_fixture();
    assert!(
        results.into_iter().all(|ok| ok),
        "actual Found/Remove and retained identity authorization controls"
    );
}

#[test]
fn peer_identity_assumed_founder_denied() {
    let peer = AuthenticatedPeer::assume(node(4));
    let node_unchanged = peer.node() == node(4);
    let incomplete = matches!(
        peer.authenticated_identity(),
        Err(VerifyRefusal::Incomplete)
    );
    assert!(
        node_unchanged,
        "synthetic founder keeps sender node behavior"
    );
    assert!(
        incomplete,
        "synthetic founder has no authenticated identity"
    );
}
