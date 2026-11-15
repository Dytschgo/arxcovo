use super::*;
fn members(voters: Vec<BTreeSet<u64>>, nodes: &[(u64, &str)]) -> Members {
    Membership::new(
        voters,
        nodes
            .iter()
            .map(|(i, a)| (*i, BasicNode::new(*a)))
            .collect::<BTreeMap<_, _>>(),
    )
    .unwrap()
}
#[test]
fn mapping_roundtrips_and_vote_order() {
    for p in [
        LogPos {
            term: 0,
            leader: 0,
            index: 0,
        },
        LogPos {
            term: 3,
            leader: 2,
            index: 7,
        },
        LogPos {
            term: u64::MAX,
            leader: u64::MAX,
            index: u64::MAX,
        },
    ] {
        assert_eq!(pos(id(p)), p);
    }
    let vs = [
        StoredVote {
            term: 1,
            node: 9,
            committed: true,
        },
        StoredVote {
            term: 2,
            node: 1,
            committed: false,
        },
        StoredVote {
            term: 2,
            node: 2,
            committed: false,
        },
        StoredVote {
            term: 2,
            node: 2,
            committed: true,
        },
    ];
    for a in vs {
        assert_eq!(stored_vote(vote(a)), a);
        for b in vs {
            assert_eq!(a.cmp(&b), vote(a).partial_cmp(&vote(b)).unwrap());
        }
    }
    let p = LogPos {
        term: 3,
        leader: 2,
        index: 7,
    };
    let m = members(vec![[1, 2].into()], &[(1, "mem:1"), (2, "mem:2")]);
    for e in [
        StoredEntry::Blank,
        StoredEntry::Membership(encode(&m).unwrap()),
    ] {
        assert_eq!(
            to_stored(from_stored((p, e.clone())).unwrap()).unwrap(),
            (p, e)
        );
    }
}
#[test]
fn maximal_codec_is_canonical_and_every_truncation_fails() {
    let nodes = (1..=16)
        .map(|i| (i, BasicNode::new("a".repeat(200))))
        .collect::<BTreeMap<_, _>>();
    let m = Membership::new(vec![(1..=16).collect(), (1..=16).collect()], nodes).unwrap();
    let b = encode(&m).unwrap();
    assert_eq!(b.bytes().len(), 3605);
    assert_eq!(decode(&b).unwrap(), m);
    for n in 1..b.bytes().len() {
        assert_eq!(
            decode(&MembershipBlob::new(b.bytes()[..n].to_vec()).unwrap()),
            Err(StoreError::Membership)
        );
    }
    let mut trailing = b.bytes().to_vec();
    trailing.push(0);
    assert!(decode(&MembershipBlob::new(trailing).unwrap()).is_err());
}
#[test]
fn codec_rejects_noncanonical_and_out_of_bounds_memberships() {
    let m = members(vec![[1, 2].into()], &[(1, "mem:1"), (2, "mem:2")]);
    let b = encode(&m).unwrap().bytes().to_vec();
    let reject = |bytes: Vec<u8>| {
        assert_eq!(
            decode(&MembershipBlob::new(bytes).unwrap()),
            Err(StoreError::Membership)
        )
    };
    for (at, value) in [
        (0, 2),
        (1, 0),
        (1, 3),
        (2, 0),
        (2, 17),
        (10, 0),
        (18, 1),
        (19, 0),
        (19, 17),
        (28, 0),
        (28, 201),
        (29, b'!'),
    ] {
        let mut bad = b.clone();
        bad[at] = value;
        reject(bad);
    }
    // First voter absent from nodes, duplicate voters, and descending voters.
    for (a, z) in [(3u64, 4u64), (1, 1), (2, 1)] {
        let mut bad = b.clone();
        bad[3..11].copy_from_slice(&a.to_be_bytes());
        bad[11..19].copy_from_slice(&z.to_be_bytes());
        reject(bad);
    }
    for address in [String::new(), "a".repeat(201), "bad/addr".to_owned()] {
        assert_eq!(
            encode(&members(vec![[1].into()], &[(1, &address)])),
            Err(StoreError::Membership)
        );
    }
    assert_eq!(
        encode(&members(vec![[0].into()], &[(0, "mem:0")])),
        Err(StoreError::Membership)
    );
    let many = (1..=17)
        .map(|i| (i, BasicNode::new("mem")))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(
        encode(&Membership::new(vec![[1].into()], many).unwrap()),
        Err(StoreError::Membership)
    );
    let default = Membership::new(
        vec![[1].into()],
        BTreeMap::from([(1, BasicNode::default())]),
    )
    .unwrap();
    assert_eq!(decode(&encode(&default).unwrap()).unwrap(), default);
}

#[test]
fn command_mapping_and_display_never_emit_command_contents() {
    use crate::cluster::enrollment::{JoinCode, KeyDigest};
    let code = JoinCode::generate([1; 16], [2; 32], |b| {
        b.fill(7);
        Ok::<_, ()>(())
    })
    .unwrap();
    let mint = Command::Mint {
        cluster: [1; 16],
        actor: [3; 16],
        request: [4; 16],
        code: code.code_id(),
        verifier: code.verifier(),
        proposed_at_ms: 1,
    };
    let redeem = Command::redeem(
        [1; 16],
        [5; 16],
        code.code_id(),
        &code.proof(),
        KeyDigest::of_spki(b"test-public-key").unwrap(),
        2,
    );
    assert_eq!(mint.to_string(), "enrollment mint");
    assert_eq!(redeem.to_string(), "enrollment redeem");
    let p = LogPos {
        term: 1,
        leader: 1,
        index: 2,
    };
    for c in [mint, redeem] {
        let e = StoredEntry::Enrollment(c);
        assert_eq!(
            to_stored(from_stored((p, e.clone())).unwrap()).unwrap(),
            (p, e)
        );
    }
}
