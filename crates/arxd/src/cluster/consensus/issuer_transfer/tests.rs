use super::super::peer_control_wire::issuer as wire;
use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt, duplex};

fn claim() -> Claim {
    Claim {
        cluster: [1; 16],
        root: [2; 32],
        sender: (NodeId::new(1).unwrap(), [3; 32]),
        receiver: (NodeId::new(2).unwrap(), [4; 32]),
        epoch: LeaderId {
            term: 5,
            node_id: 1,
        },
        membership: LogPos {
            term: 3,
            leader: 1,
            index: 7,
        },
    }
}

#[tokio::test]
async fn closed_frames_round_trip_at_the_package_ceiling() {
    let (mut client, mut server) = duplex(256);
    let claim = claim();
    let challenge = [9; CHALLENGE_LEN];
    let ciphertext = vec![17; wire::PACKAGE_MAX];
    tokio::join!(
        async {
            wire::hello(&mut client, claim).await.unwrap();
            let actual = wire::read_challenge(&mut client).await.unwrap();
            assert_eq!(actual, challenge);
            wire::sealed(&mut client, &ciphertext).await.unwrap();
            wire::read_acknowledgement(&mut client, &actual)
                .await
                .unwrap();
        },
        async {
            let mut magic = [0; 4];
            server.read_exact(&mut magic).await.unwrap();
            assert_eq!(magic, wire::MAGIC);
            assert!(wire::read_hello(&mut server).await.unwrap() == claim);
            wire::challenge(&mut server, &challenge).await.unwrap();
            assert_eq!(wire::read_sealed(&mut server).await.unwrap(), ciphertext);
            wire::acknowledge(&mut server, &challenge).await.unwrap();
        }
    );
}

#[tokio::test]
async fn wrong_verbs_versions_and_oversize_refuse_without_waiting_for_body() {
    for header in [[2, 3, 0, 26], [1, 4, 0, 26], [1, 3, 64, 1], [1, 3, 0, 25]] {
        let mut bytes = header.as_slice();
        assert!(wire::read_sealed(&mut bytes).await.is_err());
        assert!(bytes.len() <= 2);
    }
}

#[tokio::test]
async fn interrupted_package_duplicate_and_wrong_acknowledgement_refuse() {
    let (mut client, mut server) = duplex(256);
    client.write_all(&[1, 3, 0, 26, 99]).await.unwrap();
    client.shutdown().await.unwrap();
    assert!(wire::read_sealed(&mut server).await.is_err());

    let mut duplicate = vec![1, 3, 0, 26];
    duplicate.extend([0; 26]);
    duplicate.extend([1, 3, 0, 26]);
    assert!(wire::read_sealed(&mut duplicate.as_slice()).await.is_err());
    let mut bad_ack = vec![1, 4];
    bad_ack.extend([3; CHALLENGE_LEN]);
    assert!(
        wire::read_acknowledgement(&mut bad_ack.as_slice(), &[4; CHALLENGE_LEN])
            .await
            .is_err()
    );
}

#[test]
fn context_binds_the_full_membership_position_and_both_peer_keys() {
    let claim = claim();
    let mut changed = claim;
    let mut contexts = Vec::new();
    for change in 0..7 {
        changed = match change {
            0 => Claim {
                cluster: [6; 16],
                ..changed
            },
            1 => Claim {
                root: [6; 32],
                ..changed
            },
            2 => Claim {
                sender: (changed.sender.0, [6; 32]),
                ..changed
            },
            3 => Claim {
                receiver: (changed.receiver.0, [6; 32]),
                ..changed
            },
            4 => Claim {
                membership: LogPos {
                    leader: 2,
                    ..changed.membership
                },
                ..changed
            },
            5 => Claim {
                membership: LogPos {
                    term: 4,
                    ..changed.membership
                },
                ..changed
            },
            _ => Claim {
                membership: LogPos {
                    index: 8,
                    ..changed.membership
                },
                ..changed
            },
        };
        let actual = changed.transcript(&[7; CHALLENGE_LEN]);
        assert_ne!(claim.transcript(&[7; CHALLENGE_LEN]), actual);
        assert!(!contexts.contains(&actual));
        contexts.push(actual);
    }
    assert_ne!(
        claim.transcript(&[7; CHALLENGE_LEN]),
        claim.transcript(&[8; CHALLENGE_LEN])
    );
}

#[test]
fn only_the_uniform_promoted_set_provides_transfer_targets() {
    use super::super::Members;
    use openraft::BasicNode;
    use std::collections::BTreeMap;

    let nodes = [1, 3, 5, 7]
        .into_iter()
        .map(|id| (id, BasicNode::new("192.168.0.1:9000")))
        .collect::<BTreeMap<_, _>>();
    for configs in [
        vec![[1].into_iter().collect()],
        vec![[1, 3].into_iter().collect()],
        vec![[1].into_iter().collect(), [1, 3, 5].into_iter().collect()],
        vec![[3, 5, 7].into_iter().collect()],
    ] {
        assert_eq!(
            promoted_targets(&Members::new(configs, nodes.clone()).unwrap(), node_one()),
            None
        );
    }
    let flat = Members::new(vec![[1, 3, 5].into_iter().collect()], nodes).unwrap();
    assert_eq!(promoted_targets(&flat, node_one()), Some([3, 5]));
    assert_eq!(promoted_targets(&flat, NodeId::new(3).unwrap()), None);
}

fn node_one() -> NodeId {
    NodeId::new(1).unwrap()
}

#[test]
fn voter_binding_rejects_learner_wrong_cluster_revocation_and_unqualified_endpoint() {
    use super::super::{LogPos, Members};
    use crate::cluster::enrollment::{Command, FoundMembers, Outcome};
    use openraft::BasicNode;
    use std::collections::BTreeMap;

    let cluster = [1; 16];
    let local = NodeId::new(2).unwrap();
    let learner = NodeId::new(3).unwrap();
    let key = KeyDigest::of_spki(b"synthetic voter SPKI").unwrap();
    let other_key = KeyDigest::of_spki(b"synthetic learner SPKI").unwrap();
    let mut state = EnrollmentState::new(cluster, NodeId::new(4).unwrap());
    let e = LogPos {
        term: 1,
        leader: 1,
        index: 2,
    };
    assert!(matches!(
        state.apply(
            &Command::Found {
                cluster,
                members: FoundMembers::new([(local, key), (learner, other_key)]).unwrap(),
                proposed_at_ms: 1,
            },
            e,
            None,
        ),
        Outcome::Founded { replay: false },
    ));
    let members = |address| {
        Members::new(
            vec![[1, 2].into_iter().collect()],
            BTreeMap::from([
                (1, BasicNode::new("192.168.0.1:9000")),
                (2, BasicNode::new(address)),
                (3, BasicNode::new("192.168.0.3:9000")),
            ]),
        )
        .unwrap()
    };
    let applied = members("192.168.0.2:9000");
    let proof = bound_voter(&applied, &state, cluster, local).unwrap();
    assert_eq!(proof.key, key);
    assert_eq!(proof.address, "192.168.0.2:9000".parse().unwrap());
    assert!(bound_voter(&applied, &state, cluster, learner).is_none());
    assert!(bound_voter(&applied, &state, [9; 16], local).is_none());
    assert!(bound_voter(&members("203.0.113.2:9000"), &state, cluster, local).is_none());
    assert!(matches!(
        state.apply(
            &Command::Remove {
                cluster,
                node: local,
                proposed_at_ms: 2
            },
            LogPos { index: 3, ..e },
            None,
        ),
        Outcome::Removed { replay: false },
    ));
    assert!(bound_voter(&applied, &state, cluster, local).is_none());
}
