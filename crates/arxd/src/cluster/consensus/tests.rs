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

use super::adapter::{LogAdapter, SmAdapter};
use crate::cluster::{
    log_store::VoteLogStore,
    state_store::{Init, StateStore},
};
use crate::service::application::tests::private_temp;
use openraft::storage::{IOFlushed, RaftLogStorage, RaftStateMachine, StorageHelper};
use std::sync::{Arc, Mutex};
fn init() -> Init {
    Init {
        cluster: [1; 16],
        first_free: std::num::NonZeroU64::new(4).unwrap(),
    }
}
fn stores() -> (tempfile::TempDir, LogAdapter, SmAdapter) {
    let dir = private_temp();
    let ls = LogAdapter(Arc::new(Mutex::new(
        VoteLogStore::open(dir.path(), init().cluster).unwrap(),
    )));
    let sm = SmAdapter(Arc::new(Mutex::new(
        StateStore::open(dir.path(), init()).unwrap(),
    )));
    (dir, ls, sm)
}
struct Builder;
impl openraft::testing::log::StoreBuilder<Arx, LogAdapter, SmAdapter, tempfile::TempDir>
    for Builder
{
    async fn build(
        &self,
    ) -> Result<(tempfile::TempDir, LogAdapter, SmAdapter), openraft::StorageError<Arx>> {
        Ok(stores())
    }
}
type Suite = openraft::testing::log::Suite<Arx, LogAdapter, SmAdapter, Builder, tempfile::TempDir>;

macro_rules! official {
    ($test:ident,$case:ident,$sparse:expr) => {
        #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
        async fn $test() {
            let result = tokio::spawn(async {
                let (_dir, ls, sm) = stores();
                Suite::$case(ls, sm).await
            })
            .await
            .expect("official Suite panicked");
            if $sparse {
                assert!(result.unwrap_err().to_string().contains("out of order"));
            } else {
                result.unwrap();
            }
        }
    };
}
official!(
    suite_last_membership_in_log_initial,
    last_membership_in_log_initial,
    false
);
official!(suite_last_membership_in_log, last_membership_in_log, true);
official!(
    suite_last_membership_in_log_multi_step,
    last_membership_in_log_multi_step,
    false
);
official!(suite_get_membership_initial, get_membership_initial, false);
official!(
    suite_get_membership_from_log_and_empty_sm,
    get_membership_from_log_and_empty_sm,
    false
);
official!(
    suite_get_membership_from_empty_log_and_sm,
    get_membership_from_empty_log_and_sm,
    true
);
official!(
    suite_get_membership_from_log_le_sm_last_applied,
    get_membership_from_log_le_sm_last_applied,
    true
);
official!(
    suite_get_membership_from_log_gt_sm_last_applied_1,
    get_membership_from_log_gt_sm_last_applied_1,
    true
);
official!(
    suite_get_membership_from_log_gt_sm_last_applied_2,
    get_membership_from_log_gt_sm_last_applied_2,
    true
);
official!(
    suite_get_initial_state_without_init,
    get_initial_state_without_init,
    false
);
official!(
    suite_get_initial_state_membership_from_empty_log_and_sm,
    get_initial_state_membership_from_empty_log_and_sm,
    true
);
official!(
    suite_get_initial_state_membership_from_sm_inlog_is_smaller,
    get_initial_state_membership_from_sm_inlog_is_smaller,
    true
);
official!(
    suite_get_initial_state_membership_from_log_insm_is_smaller,
    get_initial_state_membership_from_log_insm_is_smaller,
    true
);
official!(
    suite_get_initial_state_with_state,
    get_initial_state_with_state,
    true
);
official!(
    suite_get_initial_state_last_log_gt_sm,
    get_initial_state_last_log_gt_sm,
    true
);
official!(
    suite_get_initial_state_last_log_lt_sm,
    get_initial_state_last_log_lt_sm,
    true
);
official!(
    suite_get_initial_state_last_log_lt_sm_past_snapshot,
    get_initial_state_last_log_lt_sm_past_snapshot,
    true
);
official!(
    suite_get_initial_state_sm_lt_purged,
    get_initial_state_sm_lt_purged,
    true
);
official!(
    suite_get_initial_state_inverted_log_order,
    get_initial_state_inverted_log_order,
    true
);
official!(
    suite_get_initial_state_conflict_at_same_index,
    get_initial_state_conflict_at_same_index,
    true
);
official!(
    suite_get_initial_state_log_ids,
    get_initial_state_log_ids,
    true
);
official!(
    suite_get_initial_state_re_apply_committed,
    get_initial_state_re_apply_committed,
    true
);
official!(suite_save_vote, save_vote, false);
official!(suite_get_log_entries, get_log_entries, false);
official!(
    suite_limited_get_log_entries,
    limited_get_log_entries,
    false
);
official!(suite_leader_bounded_stream, leader_bounded_stream, false);
official!(suite_entries_stream, entries_stream, false);
official!(suite_try_get_log_entry, try_get_log_entry, false);
official!(
    suite_log_reader_reads_new_entries,
    log_reader_reads_new_entries,
    false
);
official!(suite_initial_logs, initial_logs, false);
official!(suite_get_log_state, get_log_state, false);
official!(suite_get_log_id, get_log_id, false);
official!(suite_last_id_in_log, last_id_in_log, true);
official!(suite_last_applied_state, last_applied_state, true);
official!(suite_purge_logs_upto_0, purge_logs_upto_0, false);
official!(suite_purge_logs_upto_5, purge_logs_upto_5, false);
official!(suite_purge_logs_upto_20, purge_logs_upto_20, false);
official!(suite_delete_logs_after_11, delete_logs_after_11, false);
official!(suite_delete_logs_after_5, delete_logs_after_5, false);
official!(suite_delete_logs_after_0, delete_logs_after_0, false);
official!(suite_delete_logs_after_none, delete_logs_after_none, false);
official!(suite_append_to_log, append_to_log, false);
official!(suite_snapshot_meta, snapshot_meta, false);
official!(suite_snapshot_meta_optional, snapshot_meta_optional, false);
official!(suite_apply_single, apply_single, false);
official!(suite_apply_multiple, apply_multiple, false);
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn suite_transfer_snapshot() {
    let r = tokio::spawn(Suite::transfer_snapshot(&Builder))
        .await
        .unwrap();
    assert!(r.unwrap_err().to_string().contains("out of order"));
}

fn blank(index: u64) -> openraft::type_config::alias::EntryOf<Arx> {
    Entry {
        log_id: id(LogPos {
            term: 1,
            leader: 1,
            index,
        }),
        payload: EntryPayload::Blank,
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn append_eager_unpolled_and_committed() {
    for kind in 0..3 {
        let (dir, mut ls, sm) = stores();
        let (tx, rx) = Arx::oneshot();
        let batch = match kind {
            0 => vec![blank(0)],
            1 => (0..65).map(blank).collect(),
            _ => vec![Entry {
                log_id: blank(0).log_id,
                payload: EntryPayload::Membership(members(vec![[0].into()], &[(0, "mem:0")])),
            }],
        };
        drop(RaftLogStorage::append(
            &mut ls,
            batch,
            IOFlushed::signal(tx),
        ));
        assert!(rx.await.unwrap().is_err());
        drop(ls);
        drop(sm);
        let reopened = VoteLogStore::open(dir.path(), init().cluster).unwrap();
        assert!(reopened.log_state().unwrap().last.is_none());
    }
    let (dir, mut ls, sm) = stores();
    let (tx, rx) = Arx::oneshot();
    ls.append(vec![blank(0)], IOFlushed::signal(tx))
        .await
        .unwrap();
    assert!(rx.await.unwrap().is_ok());
    let (tx, rx) = Arx::oneshot();
    assert!(
        ls.append(vec![blank(0)], IOFlushed::signal(tx))
            .await
            .is_err()
    );
    assert!(rx.await.unwrap().is_err());
    drop(ls);
    drop(sm);
    let reopened = VoteLogStore::open(dir.path(), init().cluster).unwrap();
    assert_eq!(reopened.log_state().unwrap().last.unwrap().index, 0);
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sparse_sm_and_metadata_mismatch_leave_disk_unchanged() {
    let (_dir, _ls, mut sm) = stores();
    let before = sm.0.lock().unwrap().snapshot();
    assert!(
        sm.apply(futures_util::stream::iter([Ok((blank(1), None))]))
            .await
            .unwrap_err()
            .to_string()
            .contains("out of order")
    );
    let meta = openraft::SnapshotMeta {
        last_log_id: Some(blank(1).log_id),
        last_membership: openraft::StoredMembership::default(),
    };
    assert!(
        sm.install_snapshot(&meta, before.clone())
            .await
            .unwrap_err()
            .to_string()
            .contains("corrupt")
    );
    assert_eq!(sm.0.lock().unwrap().snapshot(), before);
}

use openraft::type_config::TypeConfigExt;

fn position(term: u64, leader: u64, index: u64) -> LogPos {
    LogPos {
        term,
        leader,
        index,
    }
}
fn fixture(dir: &std::path::Path, scenario: usize) {
    let mut log = VoteLogStore::open(dir, init().cluster).unwrap();
    let mut sm = StateStore::open(dir, init()).unwrap();
    log.save_vote(StoredVote {
        term: 3,
        node: 1,
        committed: true,
    })
    .unwrap();
    let applied = match scenario {
        10 => None,
        11 => Some(1),
        12 => None,
        13 => None,
        _ => Some(5),
    };
    let leader = if scenario == 9 { 2 } else { 1 };
    if let Some(a) = applied {
        for i in 0..=a {
            sm.apply(position(1, leader, i), &StoredEntry::Blank)
                .unwrap();
        }
    }
    let (start, end, term, node) = match scenario {
        0 => (0, 5, 2, 1),
        1 => (0, 5, 0, 1),
        2 => (0, 5, 1, 2),
        3 => (0, 5, 2, 1),
        4 => (0, 7, 2, 1),
        5 => (0, 7, 0, 1),
        6 => (7, 9, 2, 1),
        7 => (0, 3, 2, 1),
        8 => (0, 3, 1, 2),
        9 => (0, 5, 1, 1),
        10 | 11 => (0, 5, 1, 1),
        12 => (0, 3, 1, 1),
        13 | 18 => (1, 0, 1, 1),
        14 => (0, 5, 1, 1),
        15 | 16 => (0, 7, 1, 1),
        17 => (0, 3, 1, 1),
        _ => unreachable!(),
    };
    let entries = (start..=end)
        .map(|i| {
            let t = if (scenario == 4 || scenario == 5) && i != 5 {
                if i < 5 { 0 } else { 2 }
            } else if (scenario == 15 || scenario == 16) && i > 5 {
                2
            } else {
                term
            };
            (position(t, node, i), StoredEntry::Blank)
        })
        .collect::<Vec<_>>();
    log.append(&entries).unwrap();
    if scenario == 3 {
        log.purge(position(2, 1, 5)).unwrap();
    }
    if scenario == 10 || scenario == 11 {
        log.purge(position(1, 1, 3)).unwrap();
    }
    if scenario == 16 {
        log.purge(position(1, 1, 5)).unwrap();
    }
}
type CapturedDisk = (
    Option<StoredVote>,
    super::super::log_store::LogState,
    Vec<(LogPos, StoredEntry)>,
    Vec<u8>,
);
fn disk(dir: &std::path::Path) -> CapturedDisk {
    let mut log = VoteLogStore::open(dir, init().cluster).unwrap();
    let sm = StateStore::open(dir, init()).unwrap();
    (
        log.vote().unwrap(),
        log.log_state().unwrap(),
        log.entries(0, u64::MAX, 512).unwrap(),
        sm.snapshot(),
    )
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn startup_preflight_refuses_without_changing_existing_stores() {
    for scenario in 0..12 {
        let dir = private_temp();
        fixture(dir.path(), scenario);
        let before = disk(dir.path());
        {
            let mut log = VoteLogStore::open(dir.path(), init().cluster).unwrap();
            let sm = StateStore::open(dir.path(), init()).unwrap();
            assert_eq!(
                preflight(&mut log, &sm),
                Err(StoreError::Corrupt),
                "scenario {scenario}"
            );
        }
        let result = ConsensusNode::start(
            1,
            dir.path(),
            init(),
            Params::default(),
            super::mem_net::Factory {
                source: 1,
                router: Arc::default(),
            },
        )
        .await;
        assert!(
            matches!(result, Err(StartError::Preflight(StoreError::Corrupt))),
            "scenario {scenario}"
        );
        assert_eq!(disk(dir.path()), before, "scenario {scenario}");
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn startup_accepts_exact_empty_and_ordered_hole() {
    for scenario in 12..19 {
        let dir = private_temp();
        fixture(dir.path(), scenario);
        let mut n = ConsensusNode::start(
            1,
            dir.path(),
            init(),
            Params::default(),
            super::mem_net::Factory {
                source: 1,
                router: Arc::default(),
            },
        )
        .await
        .unwrap();
        n.shutdown().await.unwrap();
        if scenario == 17 {
            let mut log = tokio::time::timeout(Duration::from_secs(2), async {
                loop {
                    match VoteLogStore::open(dir.path(), init().cluster) {
                        Ok(s) => break s,
                        Err(StoreError::Io) => tokio::task::yield_now().await,
                        Err(e) => panic!("reopen: {e:?}"),
                    }
                }
            })
            .await
            .unwrap();
            assert_eq!(log.log_state().unwrap().purged, Some(position(1, 1, 5)));
            assert_eq!(log.log_state().unwrap().last, Some(position(1, 1, 5)));
            assert!(
                log.append(&[(position(1, 1, 4), StoredEntry::Blank)])
                    .is_err()
            );
            log.append(&[(position(1, 1, 6), StoredEntry::Blank)])
                .unwrap();
        }
    }
}
// Characterization only: bypasses our preflight, never claims conflict enforcement.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn raw_helper_asymmetry_and_raw_behind_purged_fatal() {
    for scenario in 0..12 {
        let dir = private_temp();
        fixture(dir.path(), scenario);
        let mut ls = LogAdapter(Arc::new(Mutex::new(
            VoteLogStore::open(dir.path(), init().cluster).unwrap(),
        )));
        let mut sm = SmAdapter(Arc::new(Mutex::new(
            StateStore::open(dir.path(), init()).unwrap(),
        )));
        let result = StorageHelper::new(&mut ls, &mut sm)
            .with_id(1)
            .get_initial_state()
            .await;
        assert_eq!(
            result.is_err(),
            matches!(scenario, 1 | 3 | 6 | 9 | 10 | 11),
            "raw scenario {scenario}: {result:?}"
        );
    }
    let dir = private_temp();
    fixture(dir.path(), 11);
    let ls = LogAdapter(Arc::new(Mutex::new(
        VoteLogStore::open(dir.path(), init().cluster).unwrap(),
    )));
    let sm = SmAdapter(Arc::new(Mutex::new(
        StateStore::open(dir.path(), init()).unwrap(),
    )));
    let result = Engine::new(
        1,
        Arc::new(Params::default().config().unwrap()),
        super::mem_net::Factory {
            source: 1,
            router: Arc::default(),
        },
        ls,
        sm,
    )
    .await;
    assert!(matches!(
        result,
        Err(openraft::errors::Fatal::StorageError(_))
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn append_spawned_cancellation_panic_and_poison_complete_once() {
    let (dir, mut ls, sm) = stores();
    let held = ls.0.clone();
    let lock = held.lock().unwrap();
    let (tx, rx) = Arx::oneshot();
    let mut future = Box::pin(ls.append(vec![blank(0)], IOFlushed::signal(tx)));
    assert!(
        std::future::Future::poll(
            future.as_mut(),
            &mut std::task::Context::from_waker(std::task::Waker::noop())
        )
        .is_pending()
    );
    drop(future);
    drop(lock);
    assert!(
        tokio::time::timeout(Duration::from_secs(2), rx)
            .await
            .unwrap()
            .unwrap()
            .is_ok()
    );
    drop(ls);
    drop(sm);
    drop(held);
    assert_eq!(disk(dir.path()).1.last, Some(pos(blank(0).log_id)));
    let (dir, mut ls, sm) = stores();
    *super::adapter::PANIC_APPEND.lock().unwrap() = Some(Arc::as_ptr(&ls.0) as usize);
    let (tx, rx) = Arx::oneshot();
    assert!(
        ls.append(vec![blank(0)], IOFlushed::signal(tx))
            .await
            .unwrap_err()
            .to_string()
            .contains("poison")
    );
    assert!(rx.await.unwrap().is_err());
    assert!(
        ls.get_log_state()
            .await
            .unwrap_err()
            .to_string()
            .contains("poison")
    );
    drop(ls);
    drop(sm);
    assert!(disk(dir.path()).1.last.is_none());
    let (_dir, mut ls, _sm) = stores();
    let (tx, rx) = Arx::oneshot();
    let iterator = std::iter::from_fn(|| -> Option<openraft::type_config::alias::EntryOf<Arx>> {
        panic!("iterator panic")
    });
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            drop(ls.append(iterator, IOFlushed::signal(tx)));
        }))
        .is_err()
    );
    assert!(rx.await.unwrap().is_err());
}
#[test]
fn parameter_profiles_validate_and_reject_unsafe_sizes() {
    for p in [
        Params::default(),
        Params {
            snapshot_logs: 8,
            keep: 0,
            purge_batch: 1,
            lag: 16,
            ..Params::default()
        },
        Params {
            election_min: 3000,
            election_max: 6000,
            ..Params::default()
        },
    ] {
        p.config().unwrap().validate().unwrap();
    }
    for p in [
        Params {
            snapshot_logs: 0,
            ..Params::default()
        },
        Params {
            keep: u64::MAX,
            ..Params::default()
        },
        Params {
            purge_batch: 0,
            ..Params::default()
        },
        Params {
            lag: 64,
            ..Params::default()
        },
        Params {
            keep: 500,
            ..Params::default()
        },
        Params {
            election_min: 2000,
            election_max: 1000,
            ..Params::default()
        },
    ] {
        assert!(p.config().is_err());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn preflight_rejects_unrepresentable_snapshot_index() {
    use sha2::{Digest, Sha256};
    for index in [i64::MAX as u64 + 1, u64::MAX] {
        let dir = private_temp();
        {
            let mut sm = StateStore::open(dir.path(), init()).unwrap();
            sm.apply(position(1, 1, 0), &StoredEntry::Blank).unwrap();
            let mut blob = sm.snapshot();
            let len = blob.len() - 32;
            blob[27..35].copy_from_slice(&index.to_be_bytes());
            let hash = Sha256::digest(&blob[..len]);
            blob[len..].copy_from_slice(&hash);
            sm.install(&blob).unwrap();
            VoteLogStore::open(dir.path(), init().cluster).unwrap();
        }
        let before = disk(dir.path());
        assert!(matches!(
            ConsensusNode::start(
                1,
                dir.path(),
                init(),
                Params::default(),
                super::mem_net::Factory {
                    source: 1,
                    router: Arc::default()
                }
            )
            .await,
            Err(StartError::Preflight(StoreError::Corrupt))
        ));
        assert_eq!(disk(dir.path()), before);
    }
}
