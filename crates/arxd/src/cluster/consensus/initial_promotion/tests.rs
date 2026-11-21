use super::*;
use crate::cluster::enrollment::{EnrollmentState, FoundMembers, KeyDigest, NodeId, PeerRefusal};

const CLUSTER: ClusterId = [9; 16];
fn position(index: u64) -> LogPos {
    LogPos {
        term: 3,
        leader: 1,
        index,
    }
}

fn view(configs: &[&[u64]], nodes: &[u64], bound: &[u64]) -> PromotionView {
    let key = KeyDigest::of_spki(b"synthetic").unwrap();
    PromotionView {
        leader: id(position(12)).leader_id,
        watermark: id(position(10)),
        members: Members::new(
            configs
                .iter()
                .map(|c| c.iter().copied().collect())
                .collect(),
            nodes
                .iter()
                .map(|n| (*n, BasicNode::new(format!("192.168.1.{n}:9300"))))
                .collect::<BTreeMap<_, _>>(),
        )
        .unwrap(),
        committed: position(12),
        tail: id(position(13)),
        matched: nodes.iter().map(|n| (*n, Some(position(12)))).collect(),
        bindings: bound.iter().map(|n| (*n, key)).collect(),
    }
}

#[test]
fn sole_learner_stays_nonvoting_and_three_bound_learners_refuse() {
    assert_eq!(decide(&view(&[&[1]], &[1, 5], &[1, 5])), None);
    assert_eq!(decide(&view(&[&[1]], &[1, 5, 9, 11], &[1, 5, 9, 11])), None);
}

#[test]
fn exact_noncontiguous_targets_keep_excluded_learner_metadata() {
    let view = view(&[&[1]], &[1, 5, 9, 11], &[1, 5, 9]);
    let Some(Transition::Joint(joint)) = decide(&view) else {
        panic!("no joint")
    };
    assert_eq!(
        joint.get_joint_config(),
        &vec![[1].into(), [1, 5, 9].into()]
    );
    assert!(joint.nodes().eq(view.members.nodes()));
    assert_eq!(joint.non_write_voter_ids().collect::<Vec<_>>(), [11]);
    assert_eq!(
        guards(&view),
        [
            Precondition::LastMembershipLogId {
                last_membership_log_id: Some(id(position(10)))
            },
            Precondition::CommittedLeaderId {
                committed_leader_id: id(position(12)).leader_id
            },
            Precondition::LastLogId {
                last_log_id: Some(id(position(13)))
            },
        ]
    );
}

#[test]
fn learners_must_match_fresh_commit_with_full_identity_at_equality() {
    let mut view = view(&[&[1]], &[1, 5, 9], &[1, 5, 9]);
    for unmatched in [
        None,
        Some(position(11)),
        Some(LogPos {
            leader: 9,
            ..position(12)
        }),
    ] {
        view.matched.insert(9, unmatched);
        assert_eq!(decide(&view), None);
    }
    for matched in [position(12), position(13)] {
        view.matched.insert(9, Some(matched));
        assert!(matches!(decide(&view), Some(Transition::Joint(_))));
    }
}

#[test]
fn exact_j_is_recoverable_without_cached_bindings_or_matches() {
    let mut view = view(&[&[1], &[1, 5, 9]], &[1, 5, 9, 11], &[1, 5]);
    view.matched.clear();
    let Some(Transition::Flatten(flat)) = decide(&view) else {
        panic!("no recovery")
    };
    assert_eq!(flat.get_joint_config(), &vec![[1, 5, 9].into()]);
    assert!(flat.nodes().eq(view.members.nodes()));
    assert_eq!(flat.non_write_voter_ids().collect::<Vec<_>>(), [11]);
    assert!(!view.bindings.contains_key(&9));
}

#[test]
fn refuses_wrong_joint_shapes_and_other_uniform_transitions() {
    for configs in [
        vec![&[1][..], &[1, 5]],
        vec![&[1][..], &[1, 5, 9, 11]],
        vec![&[1][..], &[5, 9, 11]],
        vec![&[5][..], &[1, 5, 9]],
        vec![&[1, 5][..]],
        vec![&[1, 5, 9][..]],
    ] {
        assert_eq!(decide(&view(&configs, &[1, 5, 9, 11], &[1, 5, 9])), None);
    }
}

#[test]
fn exact_registry_lookup_refuses_wrong_cluster_absence_and_revocation() {
    let one = NodeId::new(1).unwrap();
    let five = NodeId::new(5).unwrap();
    let key1 = KeyDigest::of_spki(b"one").unwrap();
    let key5 = KeyDigest::of_spki(b"five").unwrap();
    let mut registry = EnrollmentState::new(CLUSTER, NodeId::new(10).unwrap());
    assert_eq!(
        registry.bound_key(CLUSTER, five),
        Err(PeerRefusal::NotMember)
    );
    assert!(matches!(
        registry.apply(
            &Command::Found {
                cluster: CLUSTER,
                members: FoundMembers::new([(one, key1), (five, key5)]).unwrap(),
                proposed_at_ms: 100,
            },
            position(1),
            None
        ),
        Outcome::Founded { replay: false }
    ));
    assert_eq!(registry.bound_key(CLUSTER, five), Ok(key5));
    assert_eq!(
        registry.bound_key([7; 16], five),
        Err(PeerRefusal::WrongCluster)
    );
    assert!(matches!(
        registry.apply(
            &Command::Remove {
                cluster: CLUSTER,
                node: five,
                proposed_at_ms: 101,
            },
            position(2),
            None
        ),
        Outcome::Removed { replay: false }
    ));
    assert_eq!(
        registry.bound_key(CLUSTER, five),
        Err(PeerRefusal::NotMember)
    );
    assert_eq!(decide(&view(&[&[1]], &[1, 5, 9], &[1, 9])), None);
}

#[tokio::test]
async fn shared_intent_denies_overlap_and_retains_its_release_wake() {
    use crate::cluster::consensus::learner_admission::{Intent, IntentSlot};
    use futures_util::FutureExt;
    let slot = IntentSlot::default();
    let admission = Intent::waking(&slot).unwrap();
    assert!(matches!(Intent::waking(&slot), Err(Reply::Pending)));
    assert!(slot.notified().now_or_never().is_none());
    drop(admission);
    assert!(slot.notified().now_or_never().is_some());
    assert!(slot.notified().now_or_never().is_none());
    assert!(Intent::waking(&slot).is_ok());
}

#[tokio::test]
async fn release_wake_observes_joint_once_after_registration_drops_the_node_borrow() {
    use crate::cluster::consensus::learner_admission::{Intent, IntentSlot, register_with_node};
    use futures_util::FutureExt;
    use std::{
        cell::Cell,
        task::{Context, Waker},
    };

    let slot = IntentSlot::default();
    let first = Intent::waking(&slot).unwrap();
    assert!(matches!(Intent::waking(&slot), Err(Reply::Pending)));
    drop(first);
    assert!(slot.notified().now_or_never().is_some());

    let joint = view(&[&[1], &[1, 5, 9]], &[1, 5, 9], &[1, 5, 9]);
    let node = tokio::sync::Mutex::new(joint);
    let observations = Cell::new(0);
    let reconciliations = Cell::new(0);
    let intent = Intent::waking(&slot).unwrap();
    let work = async {
        let _intent = intent;
        let Ok(view) = node.try_lock() else { return };
        observations.set(observations.get() + 1);
        if matches!(decide(&view), Some(Transition::Flatten(_))) {
            reconciliations.set(reconciliations.get() + 1);
        }
    };

    // Poll at the owner spawn boundary while the real registration helper
    // still holds the node mutex. No Engine or metric notification is needed.
    let (first_poll, task) = register_with_node(&node, |_, start| {
        assert!(node.try_lock().is_err());
        let mut task = Box::pin(start.run(work));
        let first_poll = task.as_mut().poll(&mut Context::from_waker(Waker::noop()));
        (first_poll, task)
    })
    .unwrap();
    if first_poll.is_pending() {
        task.await;
    }

    assert_eq!(
        observations.get(),
        1,
        "release wake must perform one fresh observation"
    );
    assert_eq!(
        reconciliations.get(),
        1,
        "the exact J must reconcile without more progress"
    );
    assert!(first_poll.is_pending());
    assert!(slot.notified().now_or_never().is_some());
    assert!(slot.notified().now_or_never().is_none());
}
