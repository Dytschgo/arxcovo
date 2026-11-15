use super::*;

const CLUSTER: ClusterId = [1; 16];
fn node(n: u64) -> NodeId {
    NodeId::new(n).unwrap()
}
fn key(n: u8) -> KeyDigest {
    KeyDigest::of_spki(&[n]).unwrap()
}
fn code(n: u8) -> JoinCode {
    JoinCode::generate(CLUSTER, [2; 32], |bytes: &mut [u8]| {
        bytes.fill(n);
        Ok::<_, ()>(())
    })
    .unwrap()
}
fn state() -> EnrollmentState {
    let mut s = EnrollmentState::new(CLUSTER, node(10));
    s.apply(&found(vec![(node(1), key(250))]));
    s
}
fn found(pairs: Vec<(NodeId, KeyDigest)>) -> Command {
    Command::Found {
        cluster: CLUSTER,
        members: FoundMembers::new(pairs).unwrap(),
        proposed_at_ms: 0,
    }
}
fn mint(c: &JoinCode, request: u8, time: u64) -> Command {
    Command::Mint {
        cluster: CLUSTER,
        actor: [3; 16],
        request: [request; 16],
        code: c.code_id(),
        verifier: c.verifier(),
        proposed_at_ms: time,
    }
}
fn redeem(c: &JoinCode, request: u8, k: u8, time: u64) -> Command {
    Command::redeem(
        CLUSTER,
        [request; 16],
        c.code_id(),
        &c.proof(),
        key(k),
        time,
    )
}
fn grant(o: Outcome) -> EnrollmentGrant {
    match o {
        Outcome::Enrolled(g) => g,
        other => panic!("expected grant, got {other:?}"),
    }
}
fn reject(s: &mut EnrollmentState, cmd: Command, r: Reject) {
    assert_eq!(s.apply(&cmd), Outcome::Rejected(r));
}
fn only_time_changed(before: &EnrollmentState, after: &EnrollmentState, time: u64) {
    let mut expected = before.clone();
    expected.committed_ms = time;
    assert_eq!(&expected, after);
}

#[test]
fn text_is_bounded_checked_and_round_trips() {
    let c = code(7);
    let text = c.expose_for_owner_transfer();
    assert_eq!(text.len(), 206);
    assert_eq!(JoinCode::parse(&format!(" \t{text}\r\n")).unwrap(), c);
    assert_eq!(
        JoinCode::parse(&format!("arxj1-{}", text[6..].to_uppercase())).unwrap(),
        c
    );
    assert_eq!(c.cluster(), CLUSTER);
    assert_eq!(c.ca_fingerprint(), [2; 32]);
    let mut corrupt = text.clone().into_bytes();
    corrupt[205] = if corrupt[205] == b'0' { b'1' } else { b'0' };
    let cases = [
        " ".repeat(MAX_CODE_TEXT + 1),
        format!("{}{text}", " ".repeat(MAX_CODE_TEXT)),
        text.replacen("arxj1", "arxj2", 1),
        text[..205].into(),
        format!("arxj1-g{}", &text[7..]),
        format!("arxj1-é{}", &text[8..]),
        String::from_utf8(corrupt).unwrap(),
        format!("\u{a0}{text}"),
    ];
    for input in cases {
        let e = JoinCode::parse(&input).unwrap_err();
        assert_eq!(e, CodeError::Malformed);
        assert_eq!(format!("{e}"), "malformed join code");
        assert_eq!(format!("{e:?}"), "Malformed");
    }
    for failure_call in [1, 2] {
        let mut calls = 0;
        assert_eq!(
            JoinCode::generate(CLUSTER, [0; 32], |_: &mut [u8]| {
                calls += 1;
                if calls == failure_call {
                    Err(())
                } else {
                    Ok(())
                }
            }),
            Err(CodeError::Rng)
        );
        assert_eq!(calls, failure_call);
    }
}

#[test]
fn spki_is_exact_bytes_with_explicit_bounds() {
    assert_eq!(KeyDigest::of_spki(&[]), Err(CodeError::Malformed));
    assert_eq!(
        KeyDigest::of_spki(&vec![1; MAX_SPKI + 1]),
        Err(CodeError::Malformed)
    );
    assert!(KeyDigest::of_spki(&vec![1; MAX_SPKI]).is_ok());
    assert_ne!(key(1), key(2));
    assert_eq!(
        hex::encode(KeyDigest::of_spki(b"abc").unwrap().as_bytes()),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn secret_markers_never_escape_debug_even_after_retry() {
    let marker: [u8; 32] = *b"SYNTHETIC-ONLY-SECRET-MARKER-001";
    let c = JoinCode::generate(CLUSTER, [2; 32], |bytes: &mut [u8]| {
        if bytes.len() == 32 {
            bytes.copy_from_slice(&marker)
        } else {
            bytes.fill(9)
        };
        Ok::<_, ()>(())
    })
    .unwrap();
    let (secret, proof_bytes) = code::privacy_fixture(&c);
    let proof = c.proof();
    let mut s = state();
    let m = s.apply(&mint(&c, 1, 0));
    let r = s.apply(&redeem(&c, 2, 4, 1));
    let retry = s.apply(&redeem(&c, 2, 4, 2));
    let g = grant(r.clone());
    for output in [
        format!("{s:?}"),
        format!("{m:?}{r:?}{retry:?}{g:?}"),
        format!("{c:?}{proof:?}{secret:?}"),
        format!("{:?}", redeem(&c, 2, 4, 2)),
    ] {
        for forbidden in [
            String::from_utf8(marker.to_vec()).unwrap(),
            hex::encode(marker),
            format!("{marker:?}"),
            hex::encode(proof_bytes),
            format!("{proof_bytes:?}"),
            c.expose_for_owner_transfer(),
        ] {
            assert!(!output.contains(&forbidden));
        }
    }
    assert_eq!(format!("{c:?}"), "<redacted>");
    assert_eq!(format!("{secret:?}"), "<redacted>");
    assert_eq!(format!("{proof:?}"), "<redacted>");
    assert_eq!(s.records[&c.code_id()].verifier, c.verifier());
}

#[test]
fn identity_replay_and_node_ids_survive_pruning() {
    let mut s = state();
    let a = code(1);
    let b = code(2);
    s.apply(&mint(&a, 1, 0));
    s.apply(&mint(&b, 2, 0));
    let first = grant(s.apply(&redeem(&a, 3, 1, 1)));
    assert_eq!(first.node(), node(10));
    assert_eq!(first.cluster(), CLUSTER);
    assert_eq!(first.key(), key(1));
    assert_eq!(first.request(), [3; 16]);
    assert!(!first.replay());
    let before = s.clone();
    let replay = grant(s.apply(&redeem(&a, 3, 1, 2)));
    assert!(replay.replay());
    assert_eq!(replay.node(), first.node());
    assert_eq!(replay.code_expires_ms(), first.code_expires_ms());
    only_time_changed(&before, &s, 2);
    for (request, k, reason, time) in [
        (4, 1, Reject::Consumed, 3),
        (3, 2, Reject::KeyConflict, 4),
        (4, 2, Reject::Consumed, 5),
    ] {
        let before = s.clone();
        reject(&mut s, redeem(&a, request, k, time), reason);
        only_time_changed(&before, &s, time);
    }
    assert_eq!(grant(s.apply(&redeem(&b, 4, 2, 6))).node(), node(11));
    // Consumed replay is available after expiry, but issuance still refuses.
    let replay = grant(s.apply(&redeem(&a, 3, 1, CODE_TTL_MS)));
    assert_eq!(replay.node(), node(10));
    assert_eq!(
        replay.check_issuable(CODE_TTL_MS),
        Err(IssueRefusal::GrantExpired)
    );
    let prune = CODE_TTL_MS + TOMBSTONE_MS;
    reject(&mut s, redeem(&a, 3, 1, prune), Reject::UnknownCode);
    assert!(s.records.is_empty());
    let c = code(3);
    s.apply(&mint(&c, 3, prune));
    assert_eq!(grant(s.apply(&redeem(&c, 5, 3, prune))).node(), node(12));
}

#[test]
fn first_applier_wins_and_proof_precedes_consumed_or_expired_status() {
    for (winner, loser) in [(1, 2), (2, 1)] {
        let c = code(1);
        let mut s = state();
        s.apply(&mint(&c, 1, 0));
        let mut wrong = redeem(&c, 5, winner, 1);
        if let Command::Redeem { presented, .. } = &mut wrong {
            *presented = code(2).verifier();
        }
        let before = s.clone();
        reject(&mut s, wrong.clone(), Reject::BadProof);
        only_time_changed(&before, &s, 1);
        let g = grant(s.apply(&redeem(&c, 5, winner, 2)));
        assert_eq!(g.key(), key(winner));
        reject(&mut s, redeem(&c, 5, loser, 3), Reject::KeyConflict);
        reject(&mut s, wrong, Reject::BadProof);
        assert_eq!(s.next_node_id, node(11));
        let mut wrong_cluster = redeem(&c, 5, winner, u64::MAX);
        if let Command::Redeem { cluster, .. } = &mut wrong_cluster {
            *cluster = [0; 16];
        }
        let before = s.clone();
        reject(&mut s, wrong_cluster, Reject::WrongCluster);
        assert_eq!(s, before);
    }
}

#[test]
fn skew_boundaries_ratchet_and_invalid_times_do_not_mutate() {
    let c = code(1);
    let mut base = state();
    base.apply(&mint(&c, 1, 0));
    let last = CODE_TTL_MS - 2 * SKEW_MS - 1;
    assert_eq!(
        grant(base.clone().apply(&redeem(&c, 2, 1, last))).applied_ms(),
        last
    );
    reject(
        &mut base.clone(),
        redeem(&c, 2, 1, last + 1),
        Reject::Expired,
    );
    let mut s = base.clone();
    reject(&mut s, redeem(&code(2), 2, 1, 100_000), Reject::UnknownCode);
    let g = grant(s.clone().apply(&redeem(&c, 2, 1, 70_000)));
    assert_eq!(g.applied_ms(), 100_000);
    let before = s.clone();
    reject(&mut s, redeem(&c, 2, 1, 69_999), Reject::ClockBehind);
    assert_eq!(s, before);
    reject(&mut s, redeem(&c, 2, 1, CODE_TTL_MS), Reject::Expired);
    let before = s.clone();
    reject(&mut s, redeem(&c, 2, 1, 100_000), Reject::ClockBehind);
    assert_eq!(s, before);
    for time in [MAX_TIME_MS + 1, u64::MAX] {
        for cmd in [mint(&c, 1, time), redeem(&c, 2, 1, time)] {
            let before = s.clone();
            reject(&mut s, cmd, Reject::TimeOutOfRange);
            assert_eq!(s, before);
        }
    }
    // Maximum permitted timestamp remains representable through TTL and tombstone arithmetic.
    let mut high = state();
    let outcome = high.apply(&mint(&c, 1, MAX_TIME_MS));
    assert_eq!(
        outcome,
        Outcome::Minted {
            expires_ms: MAX_TIME_MS + CODE_TTL_MS,
            replay: false
        }
    );
    assert!(matches!(
        high.apply(&redeem(&c, 2, 1, MAX_TIME_MS)),
        Outcome::Enrolled(_)
    ));
}

#[test]
fn mint_order_pending_cap_and_replay_never_extend_expiry() {
    let mut s = state();
    for n in 1..=8 {
        assert_eq!(
            s.apply(&mint(&code(n), n, 0)),
            Outcome::Minted {
                expires_ms: CODE_TTL_MS,
                replay: false
            }
        );
    }
    assert_eq!(
        s.apply(&mint(&code(1), 1, 1)),
        Outcome::Minted {
            expires_ms: CODE_TTL_MS,
            replay: true
        }
    );
    reject(&mut s, mint(&code(9), 1, 2), Reject::RequestConflict);
    let mut changed = mint(&code(1), 1, 3);
    if let Command::Mint { verifier, .. } = &mut changed {
        *verifier = code(2).verifier();
    }
    reject(&mut s, changed, Reject::RequestConflict);
    reject(&mut s, mint(&code(1), 9, 4), Reject::CodeIdCollision);
    let before = s.clone();
    reject(&mut s, mint(&code(9), 9, 5), Reject::TooManyPending);
    only_time_changed(&before, &s, 5);
    assert!(matches!(
        s.apply(&mint(&code(9), 9, CODE_TTL_MS - 2 * SKEW_MS)),
        Outcome::Minted { replay: false, .. }
    ));
    assert_eq!(s.records.len(), 9);
    reject(
        &mut s,
        redeem(&code(1), 10, 1, CODE_TTL_MS + TOMBSTONE_MS),
        Reject::UnknownCode,
    );
}

#[test]
fn retained_cap_only_pruning_frees_records_and_replay_precedes_full() {
    let mut s = state();
    for n in 1..=64 {
        s.apply(&mint(&code(n), n, u64::from(n)));
        grant(s.apply(&redeem(&code(n), n, n, u64::from(n))));
        if n > 1 {
            s.apply(&Command::Remove {
                cluster: CLUSTER,
                node: node(9 + u64::from(n)),
                proposed_at_ms: u64::from(n),
            });
        }
    }
    let before = s.clone();
    reject(&mut s, mint(&code(65), 65, 65), Reject::StateFull);
    only_time_changed(&before, &s, 65);
    assert_eq!(s.records.len(), MAX_RECORDS);
    assert_eq!(
        s.apply(&mint(&code(1), 1, 66)),
        Outcome::Minted {
            expires_ms: CODE_TTL_MS + 1,
            replay: true
        }
    );
    assert!(grant(s.apply(&redeem(&code(1), 1, 1, 67))).replay());
    reject(&mut s, mint(&code(65), 1, 68), Reject::RequestConflict);
    reject(&mut s, mint(&code(1), 65, 69), Reject::CodeIdCollision);
    let prune = 1 + CODE_TTL_MS + TOMBSTONE_MS;
    reject(&mut s, mint(&code(65), 65, prune - 1), Reject::StateFull);
    assert!(matches!(
        s.apply(&mint(&code(65), 65, prune)),
        Outcome::Minted { replay: false, .. }
    ));
    assert_eq!(s.records.len(), 64);
    assert!(!s.records.contains_key(&code(1).code_id()));
    assert!(s.records.contains_key(&code(2).code_id()));
    assert_eq!(s.next_node_id, node(74));
}

#[test]
fn node_exhaustion_never_issues_max_and_leaves_pending_intact() {
    let mut s = EnrollmentState::new(CLUSTER, node(u64::MAX - 1));
    s.apply(&found(vec![(node(1), key(250))]));
    let a = code(1);
    let b = code(2);
    s.apply(&mint(&a, 1, 0));
    s.apply(&mint(&b, 2, 0));
    assert_eq!(
        grant(s.apply(&redeem(&a, 3, 1, 1))).node(),
        node(u64::MAX - 1)
    );
    let before = s.clone();
    reject(&mut s, redeem(&b, 4, 2, 2), Reject::NodeIdsExhausted);
    only_time_changed(&before, &s, 2);
    assert!(s.records[&b.code_id()].consumed.is_none());
    assert_eq!(
        grant(s.apply(&redeem(&a, 3, 1, 3))).node(),
        node(u64::MAX - 1)
    );
}

#[test]
fn delayed_sole_proposal_grants_by_proposal_time_then_issuance_guard_refuses() {
    let c = code(1);
    let mut s = state();
    s.apply(&mint(&c, 1, 0));
    // Applying at physical 3_600_000 is deliberately invisible to this pure state machine.
    let g = grant(s.apply(&redeem(&c, 2, 1, 1_000)));
    assert_eq!(g.applied_ms(), 1_000);
    assert_eq!(g.check_issuable(3_600_000), Err(IssueRefusal::GrantExpired));
    assert_eq!(g.check_issuable(CODE_TTL_MS - 2 * SKEW_MS - 1), Ok(()));
    for now in [CODE_TTL_MS - 2 * SKEW_MS, u64::MAX] {
        assert_eq!(g.check_issuable(now), Err(IssueRefusal::GrantExpired));
    }
    let mut s = state();
    s.apply(&mint(&c, 1, 0));
    let g = grant(s.apply(&redeem(&c, 2, 1, 100_000)));
    assert_eq!(
        g.check_issuable(69_999),
        Err(IssueRefusal::SignerClockBehind)
    );
    assert_eq!(g.check_issuable(70_000), Ok(()));
}

#[test]
fn replies_are_generic_and_replicas_deterministic() {
    let rejects = [
        Reject::NotFounded,
        Reject::BadFounders,
        Reject::AlreadyFounded,
        Reject::KeyInUse,
        Reject::MembersFull,
        Reject::Revoked,
        Reject::UnknownNode,
        Reject::LastMember,
        Reject::WrongCluster,
        Reject::TimeOutOfRange,
        Reject::ClockBehind,
        Reject::RequestConflict,
        Reject::CodeIdCollision,
        Reject::TooManyPending,
        Reject::StateFull,
        Reject::UnknownCode,
        Reject::BadProof,
        Reject::KeyConflict,
        Reject::Consumed,
        Reject::Expired,
        Reject::NodeIdsExhausted,
    ];
    for r in rejects {
        assert_eq!(
            Outcome::Rejected(r).joiner_reply(),
            if r == Reject::ClockBehind {
                JoinerReply::RetryLater
            } else {
                JoinerReply::Rejected
            }
        );
        assert!(!r.to_string().contains("["));
    }
    assert_eq!(
        Outcome::Minted {
            expires_ms: 42,
            replay: false
        }
        .joiner_reply(),
        JoinerReply::Rejected
    );
    assert_eq!(
        IssueRefusal::SignerClockBehind.to_string(),
        "signer clock behind"
    );
    assert_eq!(IssueRefusal::GrantExpired.to_string(), "grant expired");
    let a = code(1);
    let b = code(2);
    let mut s = state();
    let mut replica = s.clone();
    let cmds = [
        mint(&a, 1, 0),
        mint(&a, 1, 1),
        mint(&b, 2, 2),
        redeem(&a, 3, 1, 3),
        redeem(&a, 3, 1, 4),
        redeem(&a, 3, 2, 5),
        redeem(&b, 4, 2, CODE_TTL_MS),
        mint(&code(3), 3, CODE_TTL_MS + TOMBSTONE_MS),
    ];
    for cmd in cmds {
        let result = s.apply(&cmd);
        assert_eq!(result, replica.apply(&cmd));
        assert_eq!(s, replica);
        if let Outcome::Enrolled(g) = result {
            assert_eq!(
                Outcome::Enrolled(g.clone()).joiner_reply(),
                JoinerReply::Enrolled { node: g.node() }
            );
        }
    }
}

#[test]
fn transfer_hashes_match_independent_synthetic_vectors() {
    // Fixed vectors calculated with Python hashlib, independently of this implementation.
    let c = code(7);
    let (_, proof) = code::privacy_fixture(&c);
    assert_eq!(
        hex::encode(proof),
        "80280cf08ba16406f70ef0fa14066da0bb4c3b9c342e9bba5ff64a079800e61f"
    );
    assert_eq!(
        hex::encode(c.verifier().bytes()),
        "c48198575d67741c8d415e3dc4f88849fc0962bb7321f9680af095e1153da3de"
    );
    assert!(c.expose_for_owner_transfer().ends_with("569a558e"));
    let other_cluster = JoinCode::generate([4; 16], [2; 32], |bytes: &mut [u8]| {
        bytes.fill(7);
        Ok::<_, ()>(())
    })
    .unwrap();
    assert_ne!(c.proof(), other_cluster.proof());
    let mut calls = 0;
    let other_id = JoinCode::generate(CLUSTER, [2; 32], |bytes: &mut [u8]| {
        calls += 1;
        bytes.fill(if calls == 1 { 8 } else { 7 });
        Ok::<_, ()>(())
    })
    .unwrap();
    assert_ne!(c.proof(), other_id.proof());
}

#[test]
fn pending_precedes_total_cap_and_rejects_still_prune() {
    let mut s = state();
    for n in 1..=64 {
        s.apply(&mint(&code(n), n, 0));
        if n <= 56 {
            let g = grant(s.apply(&redeem(&code(n), n, n, 0)));
            s.apply(&Command::Remove {
                cluster: CLUSTER,
                node: g.node(),
                proposed_at_ms: 0,
            });
        }
    }
    assert_eq!(s.records.len(), 64);
    reject(&mut s, mint(&code(65), 65, 1), Reject::TooManyPending);
    let prune = CODE_TTL_MS + TOMBSTONE_MS;
    let next = s.next_node_id;
    // A later rejection performs the common pruning step but never allocates.
    reject(&mut s, redeem(&code(65), 65, 1, prune), Reject::UnknownCode);
    assert!(s.records.is_empty());
    assert_eq!(s.next_node_id, next);
}

#[test]
fn wrong_proof_on_expired_pending_is_bad_proof() {
    let mut s = state();
    let c = code(7);
    s.apply(&mint(&c, 1, 0));
    let cmd = Command::redeem(
        CLUSTER,
        [9; 16],
        c.code_id(),
        &code(8).proof(),
        key(4),
        CODE_TTL_MS,
    );
    assert_eq!(s.apply(&cmd), Outcome::Rejected(Reject::BadProof));
}

#[test]
fn registry_validation_replay_gates_and_authorization() {
    for (r, text) in [
        (PeerRefusal::WrongCluster, "wrong cluster"),
        (PeerRefusal::NotMember, "not member"),
        (PeerRefusal::KeyMismatch, "key mismatch"),
    ] {
        assert_eq!(r.to_string(), text);
    }
    let mut s = EnrollmentState::new(CLUSTER, node(10));
    for cmd in [
        mint(&code(1), 1, 0),
        redeem(&code(1), 1, 1, 0),
        Command::Remove {
            cluster: CLUSTER,
            node: node(1),
            proposed_at_ms: 0,
        },
    ] {
        reject(&mut s, cmd, Reject::NotFounded);
    }
    let invalid = vec![
        vec![],
        vec![(node(10), key(1))],
        vec![(node(2), key(2)), (node(1), key(1))],
        vec![(node(1), key(1)), (node(1), key(2))],
        vec![(node(1), key(1)), (node(2), key(1))],
        (1..=17).map(|n| (node(n), key(n as u8))).collect(),
    ];
    for founded in [false, true] {
        if founded {
            assert_eq!(
                s.apply(&found(vec![(node(1), key(250))])),
                Outcome::Founded { replay: false }
            );
        }
        for pairs in &invalid {
            let before = s.clone();
            reject(&mut s, found(pairs.clone()), Reject::BadFounders);
            assert_eq!(s, before);
        }
    }
    assert_eq!(
        s.apply(&found(vec![(node(1), key(250))])),
        Outcome::Founded { replay: true }
    );
    reject(
        &mut s,
        found(vec![(node(1), key(249))]),
        Reject::AlreadyFounded,
    );
    assert_eq!(
        s.authorize_peer([0; 16], node(1), key(250)),
        Err(PeerRefusal::WrongCluster)
    );
    assert_eq!(
        s.authorize_peer(CLUSTER, node(10), key(250)),
        Err(PeerRefusal::NotMember)
    );
    assert_eq!(
        s.authorize_peer(CLUSTER, node(1), key(1)),
        Err(PeerRefusal::KeyMismatch)
    );
    let remove = |n| Command::Remove {
        cluster: CLUSTER,
        node: node(n),
        proposed_at_ms: 0,
    };
    reject(&mut s, remove(10), Reject::UnknownNode);
    reject(&mut s, remove(1), Reject::LastMember);
    for (n, k) in [(1, 1), (2, 2)] {
        s.apply(&mint(&code(n), n, 0));
        reject(&mut s, redeem(&code(n), n, 250, 0), Reject::KeyInUse);
        if n == 2 {
            reject(&mut s, redeem(&code(n), n, 1, 0), Reject::KeyInUse);
        }
        grant(s.apply(&redeem(&code(n), n, k, 0)));
    }
    assert_eq!(
        s.apply(&found(vec![(node(10), key(1))])),
        Outcome::Founded { replay: true }
    );
    assert_eq!(s.apply(&remove(10)), Outcome::Removed { replay: false });
    assert_eq!(s.apply(&remove(10)), Outcome::Removed { replay: true });
    assert_eq!(
        s.authorize_peer(CLUSTER, node(10), key(1)),
        Err(PeerRefusal::NotMember)
    );
    reject(&mut s, redeem(&code(1), 1, 1, 0), Reject::Revoked);
    let prune = CODE_TTL_MS + TOMBSTONE_MS;
    reject(&mut s, redeem(&code(2), 2, 2, prune), Reject::UnknownCode);
    assert!(s.records.is_empty());
    assert_eq!(s.authorize_peer(CLUSTER, node(11), key(2)), Ok(()));
}
#[test]
fn registry_capacity_rejection_keeps_code_and_counter() {
    let mut s = EnrollmentState::new(CLUSTER, node(17));
    assert_eq!(
        s.apply(&found((1..=16).map(|n| (node(n), key(n as u8))).collect())),
        Outcome::Founded { replay: false }
    );
    s.apply(&mint(&code(80), 80, 0));
    let before = s.clone();
    reject(&mut s, redeem(&code(80), 80, 80, 0), Reject::MembersFull);
    assert_eq!(s, before);
    s.apply(&Command::Remove {
        cluster: CLUSTER,
        node: node(16),
        proposed_at_ms: 0,
    });
    assert_eq!(
        grant(s.apply(&redeem(&code(80), 80, 80, 0))).node(),
        node(17)
    );
}
#[test]
fn founding_iterator_is_bounded_before_command_construction() {
    use std::cell::Cell;
    for n in [0, 1, 16, 17, 64] {
        assert_eq!(
            FoundMembers::new((0..n).map(|_| (node(1), key(1))))
                .unwrap()
                .as_slice()
                .len(),
            n
        );
    }
    let pulls = Cell::new(0);
    assert_eq!(
        FoundMembers::new((0..1_000_000).map(|_| {
            pulls.set(pulls.get() + 1);
            (node(1), key(1))
        })),
        Err(FoundTooLarge)
    );
    assert_eq!(pulls.get(), 65);
}

#[test]
fn registry_commands_preserve_common_time_and_prune_prefix() {
    let mut s = state();
    s.apply(&mint(&code(1), 1, 0));
    for remove in [false, true] {
        for (cluster, time, reason) in [
            ([0; 16], u64::MAX, Reject::WrongCluster),
            (CLUSTER, MAX_TIME_MS + 1, Reject::TimeOutOfRange),
        ] {
            let cmd = if remove {
                Command::Remove {
                    cluster,
                    node: node(1),
                    proposed_at_ms: time,
                }
            } else {
                Command::Found {
                    cluster,
                    members: FoundMembers::new([]).unwrap(),
                    proposed_at_ms: time,
                }
            };
            let before = s.clone();
            reject(&mut s, cmd, reason);
            assert_eq!(s, before);
        }
    }
    let prune = CODE_TTL_MS + TOMBSTONE_MS;
    let members = s.members.clone();
    let next = s.next_node_id;
    reject(
        &mut s,
        Command::Found {
            cluster: CLUSTER,
            members: FoundMembers::new([]).unwrap(),
            proposed_at_ms: prune,
        },
        Reject::BadFounders,
    );
    assert!(s.records.is_empty());
    assert_eq!(s.members, members);
    assert_eq!(s.next_node_id, next);
    let before = s.clone();
    reject(
        &mut s,
        Command::Remove {
            cluster: CLUSTER,
            node: node(1),
            proposed_at_ms: prune - SKEW_MS - 1,
        },
        Reject::ClockBehind,
    );
    assert_eq!(s, before);
}
