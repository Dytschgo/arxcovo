use super::super::peer_registry::crossed;
use super::*;
use crate::cluster::{
    bound_open::{BoundIntent, BoundStores},
    enrollment::{FoundMembers, NodeId},
    join_receipt::{Origin, tests::fixture},
    startup_plan::{Mode, tests::Layout},
    state_store::Init,
};

const CLUSTER: [u8; 16] = [1; 16];
const ADDRESS: &str = "192.168.1.2:7001";

fn position(term: u64, leader: u64, index: u64) -> LogPos {
    LogPos {
        term,
        leader,
        index,
    }
}

fn membership(address: Option<&str>) -> MembershipBlob {
    let mut nodes = std::collections::BTreeMap::from([(1, BasicNode::new("192.168.1.1:7001"))]);
    if let Some(address) = address {
        nodes.insert(2, BasicNode::new(address));
    }
    encode(&Members::new(vec![[1].into_iter().collect()], nodes).unwrap()).unwrap()
}

fn origin() -> Origin {
    Origin::Redeem {
        term: 1,
        leader: 1,
        index: 1,
        anchors: vec![(1, [3; 32])],
    }
}

fn found(expected: &ExpectedIdentity) -> StoredEntry {
    StoredEntry::Enrollment(Command::Found {
        cluster: CLUSTER,
        members: FoundMembers::new([(expected.node, expected.key)]).unwrap(),
        proposed_at_ms: 1,
    })
}

fn init() -> Init {
    Init {
        cluster: CLUSTER,
        first_free: NodeId::new(3).unwrap(),
    }
}

fn wrong_position_refuses(wrong: LogPos) {
    let layout = Layout::new();
    let (record, root, expected) = fixture(2, origin());
    layout.write("credentials/identity.json", &record);
    layout.write("credentials/ca.der", &root);
    layout.write("credentials/sealed-key", b"opaque legacy fixture");
    layout.write("state/cluster-owner.lock", b"");
    let mut plan = layout.plan("recover-joiner");
    plan.peer = ADDRESS.into();
    plan.enrollment = "192.168.1.2:7002".into();
    let genesis = position(1, 1, 0);
    let entries = [
        (genesis, StoredEntry::Membership(membership(None))),
        (wrong, found(&expected)),
    ];
    {
        let mut sm = StateStore::open(&plan.state, init()).unwrap();
        for (pos, entry) in &entries {
            sm.apply(*pos, entry).unwrap();
        }
        let mut log = VoteLogStore::open(&plan.state, CLUSTER).unwrap();
        log.append(&entries).unwrap();
        preflight(&mut log, &sm).unwrap();
        assert_eq!(
            sm.state()
                .authorize_peer(CLUSTER, expected.node, expected.key),
            Ok(())
        );
    }
    let before = layout.snapshot();
    let start = StartIntent {
        mode: Mode::RecoverJoiner,
        node: expected.node,
        cluster: CLUSTER,
        peer: plan.peer.parse().unwrap(),
        enrollment: plan.enrollment.parse().unwrap(),
    };
    let mut owner = Owner::acquire(plan).unwrap();
    let (mut stores, clock) =
        startup_recovery::prepare_strict_joiner(&mut owner, &start, &expected);
    assert!(clock.is_none());
    assert_eq!(stores.first_failure_for_learner(), Refusal::Identity);
    assert_eq!(
        stores.io_counts().1,
        2,
        "only the two read-only connections"
    );
    assert!(
        stores.take_pair().is_err(),
        "nothing can be transferred to Engine"
    );
    assert_eq!(layout.snapshot(), before);
    let external = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(owner.plan.state.join("cluster-owner.lock"))
        .unwrap();
    assert!(external.try_lock().is_err());
}

#[test]
fn recover_joiner_refuses_same_index_higher_term_before_writable_acquisition() {
    wrong_position_refuses(position(2, 1, 1));
}

#[test]
fn recover_joiner_refuses_same_index_higher_leader_before_writable_acquisition() {
    wrong_position_refuses(position(1, 2, 1));
}

#[test]
fn crossing_requires_full_position_canonical_membership_and_local_binding() {
    let layout = Layout::new();
    let (_, _, expected) = fixture(2, origin());
    let plan = layout.plan("join");
    let mut store = StateStore::open(&plan.state, init()).unwrap();
    let genesis = position(1, 1, 0);
    store
        .apply(genesis, &StoredEntry::Membership(membership(None)))
        .unwrap();
    let address = ADDRESS.parse().unwrap();
    let e = expected.origin.position();
    assert!(!crossed(
        &expected,
        address,
        genesis,
        store.membership(),
        store.state()
    ));
    store.apply(e, &found(&expected)).unwrap();
    assert!(crossed(
        &expected,
        address,
        e,
        store.membership(),
        store.state()
    ));
    assert!(!crossed(
        &expected,
        address,
        position(2, 1, 1),
        store.membership(),
        store.state()
    ));
    assert!(!crossed(
        &expected,
        address,
        position(1, 2, 1),
        store.membership(),
        store.state()
    ));
    let after = position(2, 1, 2);
    store
        .apply(after, &StoredEntry::Membership(membership(Some(ADDRESS))))
        .unwrap();
    let blob = store.snapshot();
    let target = Layout::new();
    let mut installed = StateStore::open(&target.plan("join").state, init()).unwrap();
    installed.install(&blob).unwrap();
    assert!(crossed(
        &expected,
        address,
        after,
        installed.membership(),
        installed.state()
    ));
    assert!(!crossed(
        &expected,
        "192.168.1.2:7003".parse().unwrap(),
        after,
        installed.membership(),
        installed.state()
    ));
    assert!(!crossed(&expected, address, after, None, installed.state()));
}

#[test]
fn fresh_join_cannot_construct_stores_without_retained_publication() {
    let layout = Layout::new();
    let mut owner = Owner::acquire(layout.plan("join")).unwrap();
    let before = layout.snapshot();
    let (stores, ready) = BoundStores::operational(
        &mut owner,
        BoundIntent::PublishedJoin(init()),
        |_| Ok(()),
        |_, _, _| Ok(()),
    );
    assert!(ready.is_none());
    assert_eq!(stores.io_counts(), (0, 0));
    assert_eq!(layout.snapshot(), before);
}
