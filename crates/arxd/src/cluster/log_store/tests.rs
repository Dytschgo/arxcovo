use super::*;
use crate::cluster::{
    enrollment::{EnrollmentState, NodeId, Outcome, Reject},
    state_store::{
        Init, StateStore,
        tests::{KillOnDrop, absent, mint, privacy_markers, redeem},
    },
};
use crate::service::application::tests::private_temp;
use std::{
    process::{Command as ProcessCommand, Stdio},
    time::{Duration, Instant},
};
fn pos(index: u64) -> LogPos {
    LogPos {
        term: 2,
        leader: 3,
        index,
    }
}
fn open(p: &Path) -> VoteLogStore {
    VoteLogStore::open(p, [1; 16]).unwrap()
}
fn batch(start: u64, end: u64) -> Vec<(LogPos, Entry)> {
    (start..end).map(|i| (pos(i), Entry::Blank)).collect()
}
fn payloads() -> Vec<(LogPos, Entry)> {
    vec![
        (pos(0), crate::cluster::state_store::tests::found()),
        (
            pos(1),
            Entry::Membership(MembershipBlob::new(b"synthetic-members-m3".to_vec()).unwrap()),
        ),
        (pos(2), mint(5, 0)),
        (pos(3), redeem(5, 1)),
        (pos(4), mint(6, 2)),
        (pos(5), redeem(5, 3)),
        (pos(6), redeem(6, 4)),
    ]
}
fn resign(b: &mut Vec<u8>) {
    b.truncate(b.len() - 32);
    let h = Sha256::digest(&*b);
    b.extend(h);
}
fn bad_row(b: &[u8], edit: impl FnOnce(&mut Vec<u8>)) {
    let mut x = b.to_vec();
    edit(&mut x);
    resign(&mut x);
    assert_eq!(decode_row(&x), Err(StoreError::Corrupt));
}
fn bad_meta(b: &[u8], edit: impl FnOnce(&mut Vec<u8>)) {
    let mut x = b.to_vec();
    edit(&mut x);
    resign(&mut x);
    assert_eq!(decode_meta(&x), Err(StoreError::Corrupt));
}
#[test]
fn canonical_codecs_and_full_payload_semantic_rejections() {
    let mut entries = payloads();
    entries.push((pos(7), Entry::Blank));
    for n in [1, 4096] {
        entries.push((
            pos(7),
            Entry::Membership(MembershipBlob::new(vec![0x42; n]).unwrap()),
        ));
    }
    entries.push((pos(8), mint(9, u64::MAX)));
    entries.push((pos(9), redeem(9, u64::MAX)));
    for (p, e) in &entries {
        let b = encode_row(*p, e);
        assert_eq!(decode_row(&b), Ok((*p, e.clone())));
        assert_eq!(encode_row(*p, &decode_row(&b).unwrap().1), b);
        for n in 0..b.len() {
            assert_eq!(decode_row(&b[..n]), Err(StoreError::Corrupt));
        }
        bad_row(&b, |x| x[0] ^= 1);
        bad_row(&b, |x| {
            let n = x.len() - 32;
            x.insert(n, 0);
        });
        for tag in [6, 255] {
            bad_row(&b, |x| x[32] = tag);
        }
        bad_row(&b, |x| {
            x[24..32].copy_from_slice(&(i64::MAX as u64 + 1).to_be_bytes())
        });
        let mut flipped = b.clone();
        let n = flipped.len() - 1;
        flipped[n] ^= 1;
        assert_eq!(decode_row(&flipped), Err(StoreError::Corrupt));
    }
    // Full payload with recomputed digest: reach membership length and trailing-byte checks.
    let b = encode_row(
        pos(0),
        &Entry::Membership(MembershipBlob::new(vec![0x42; 4096]).unwrap()),
    );
    bad_row(&b, |x| x[33..35].copy_from_slice(&0u16.to_be_bytes()));
    let mut oversize = b[..b.len() - 32].to_vec();
    oversize[33..35].copy_from_slice(&4097u16.to_be_bytes());
    oversize.push(0x42);
    assert_eq!(decode_row(&seal(oversize)), Err(StoreError::Corrupt));
    for vote in [
        None,
        Some(StoredVote {
            term: 3,
            node: 4,
            committed: true,
        }),
    ] {
        for purged in [None, Some(pos(0))] {
            for tail in [None, Some((1, pos(4)))] {
                let m = Meta {
                    cluster: [1; 16],
                    vote,
                    purged,
                    tail,
                };
                let b = encode_meta(&m);
                assert_eq!(decode_meta(&b), Ok(m));
                for n in 0..b.len() {
                    assert_eq!(decode_meta(&b[..n]), Err(StoreError::Corrupt));
                }
                for offset in [0, 9, 26] {
                    bad_meta(&b, |x| x[offset] = 255);
                }
                bad_meta(&b, |x| {
                    let n = x.len() - 32;
                    x.insert(n, 0);
                });
                let mut x = b.clone();
                let n = x.len() - 1;
                x[n] ^= 1;
                assert_eq!(decode_meta(&x), Err(StoreError::Corrupt));
            }
        }
    }
    let m = Meta {
        cluster: [1; 16],
        vote: Some(StoredVote {
            term: 3,
            node: 4,
            committed: true,
        }),
        purged: Some(pos(0)),
        tail: Some((1, pos(4))),
    };
    let b = encode_meta(&m);
    for offset in [43, 44, 69] {
        bad_meta(&b, |x| x[offset] = 2);
    }
    bad_meta(&b, |x| x[70..78].copy_from_slice(&5u64.to_be_bytes()));
    bad_meta(&b, |x| x[70..78].copy_from_slice(&2u64.to_be_bytes()));
    bad_meta(&b, |x| {
        x[94..102].copy_from_slice(&(i64::MAX as u64 + 1).to_be_bytes())
    });
    // The command codec retains all times; deterministic rejection belongs to apply.
    let mut core = EnrollmentState::new([1; 16], NodeId::new(10).unwrap());
    if let Entry::Enrollment(c) = decode_row(&encode_row(pos(0), &mint(9, u64::MAX)))
        .unwrap()
        .1
    {
        assert_eq!(core.apply(&c), Outcome::Rejected(Reject::TimeOutOfRange));
    }
}
#[test]
fn append_preconditions_and_maximal_capacity_are_atomic() {
    let d = private_temp();
    let mut s = open(d.path());
    s.append(&batch(1, 3)).unwrap();
    let before = s.meta.clone();
    let cases = vec![
        (batch(4, 5), StoreError::OutOfOrder),
        (batch(2, 3), StoreError::OutOfOrder),
        (batch(1, 2), StoreError::OutOfOrder),
        (
            vec![(pos(3), Entry::Blank), (pos(5), Entry::Blank)],
            StoreError::OutOfOrder,
        ),
        (
            vec![(LogPos { term: 1, ..pos(3) }, Entry::Blank)],
            StoreError::OutOfOrder,
        ),
        (batch(3, 68), StoreError::TooLarge),
        (
            batch(i64::MAX as u64 + 1, i64::MAX as u64 + 2),
            StoreError::TooLarge,
        ),
    ];
    for (b, e) in cases {
        assert_eq!(s.append(&b), Err(e));
        assert_eq!(s.meta, before);
        assert_eq!(s.entries(0, u64::MAX, 512).unwrap(), batch(1, 3));
    }
    s.append(&[]).unwrap();
    drop(s);
    assert_eq!(open(d.path()).meta, before);
    let edge = private_temp();
    let mut s = open(edge.path());
    let p = pos(i64::MAX as u64);
    s.append(&[(p, Entry::Blank)]).unwrap();
    s.truncate_after(Some(p)).unwrap();
    assert_eq!(
        s.entries(p.index, u64::MAX, 1).unwrap(),
        vec![(p, Entry::Blank)]
    );
    drop(s);
    assert_eq!(open(edge.path()).live(), Ok(1));
    let d = private_temp();
    let mut s = open(d.path());
    let m = Entry::Membership(MembershipBlob::new(vec![0x42; 4096]).unwrap());
    assert_eq!(encode_row(pos(0), &m).len(), MAX_ROW);
    for start in (0..512).step_by(64) {
        let b: Vec<_> = (start..start + 64).map(|i| (pos(i), m.clone())).collect();
        s.append(&b).unwrap();
    }
    let before = s.meta.clone();
    assert_eq!(s.live(), Ok(512));
    assert_eq!(s.append(&[(pos(512), m.clone())]), Err(StoreError::Full));
    assert_eq!(s.meta, before);
    let pages: u64 =
        s.db.pragma_query_value(None, "page_count", |r| r.get(0))
            .unwrap();
    assert!(pages <= 2048);
    drop(s);
    let mut s = open(d.path());
    assert_eq!(s.meta, before);
    let rows = s.entries(0, u64::MAX, 512).unwrap();
    assert_eq!(rows.len(), 512);
    assert!(rows.iter().all(|(_, e)| *e == m));
}
#[test]
fn bounded_ranges_and_corruption_poison() {
    let d = private_temp();
    let mut s = open(d.path());
    s.append(&batch(0, 10)).unwrap();
    for (lo, hi, limit, want) in [
        (3, 3, 64, vec![]),
        (5, 7, 64, batch(5, 7)),
        (0, u64::MAX, 64, batch(0, 10)),
        (20, 30, 64, vec![]),
        (8, 2, 1, vec![]),
        (5, 9, 1, batch(5, 6)),
    ] {
        assert_eq!(s.entries(lo, hi, limit).unwrap(), want);
    }
    s.purge(pos(3)).unwrap();
    assert_eq!(s.entries(0, 7, 64).unwrap(), batch(4, 7));
    assert_eq!(s.entries(0, 9, 0), Err(StoreError::TooLarge));
    assert_eq!(s.entries(0, 9, 513), Err(StoreError::TooLarge));
    s.db.execute("DELETE FROM log WHERE idx=5", []).unwrap();
    assert_eq!(s.entries(4, 7, 64), Err(StoreError::Corrupt));
    assert_poisoned(&mut s);
}
fn assert_poisoned(s: &mut VoteLogStore) {
    assert_eq!(s.vote(), Err(StoreError::Poisoned));
    assert_eq!(s.live(), Err(StoreError::Poisoned));
    assert_eq!(s.log_state(), Err(StoreError::Poisoned));
    assert_eq!(s.entries(0, 10, 64), Err(StoreError::Poisoned));
    assert_eq!(s.append(&[]), Err(StoreError::Poisoned));
    assert_eq!(s.truncate_after(None), Err(StoreError::Poisoned));
    assert_eq!(s.purge(pos(0)), Err(StoreError::Poisoned));
    assert_eq!(
        s.save_vote(StoredVote {
            term: 9,
            node: 1,
            committed: false
        }),
        Err(StoreError::Poisoned)
    );
}
#[test]
fn truncate_purge_independent_suite_cases_and_id_conflicts() {
    let d = private_temp();
    let mut s = open(d.path());
    s.append(&batch(1, 11)).unwrap();
    let original = s.meta.clone();
    s.truncate_after(Some(pos(11))).unwrap();
    assert_eq!(s.meta, original);
    for op in [false, true] {
        let bad = LogPos {
            leader: 4,
            ..pos(5)
        };
        let r = if op {
            s.purge(bad)
        } else {
            s.truncate_after(Some(bad))
        };
        assert_eq!(r, Err(StoreError::Conflict));
        assert_eq!(s.meta, original);
    }
    s.truncate_after(None).unwrap();
    assert_eq!(s.live(), Ok(0));
    s.append(&batch(1, 11)).unwrap();
    s.purge(LogPos {
        term: 0,
        leader: 0,
        index: 0,
    })
    .unwrap();
    s.append(&[(LogPos { term: 3, ..pos(11) }, Entry::Blank)])
        .unwrap();
    assert_eq!(s.truncate_after(None), Err(StoreError::OutOfOrder));
    s.truncate_after(Some(pos(8))).unwrap();
    s.append(&[(LogPos { term: 4, ..pos(9) }, Entry::Blank)])
        .unwrap();
    drop(s);
    let s = open(d.path());
    assert_eq!(
        s.log_state().unwrap().last,
        Some(LogPos { term: 4, ..pos(9) })
    );
    let d = private_temp();
    let mut s = open(d.path());
    s.append(&batch(1, 11)).unwrap();
    s.purge(pos(20)).unwrap();
    assert_eq!(
        s.log_state(),
        Ok(LogState {
            purged: Some(pos(20)),
            last: Some(pos(20))
        })
    );
    assert_eq!(s.append(&batch(11, 12)), Err(StoreError::OutOfOrder));
    s.append(&batch(21, 22)).unwrap();
    assert_eq!(s.purge(pos(19)), Err(StoreError::OutOfOrder));
    assert_eq!(s.truncate_after(Some(pos(19))), Err(StoreError::OutOfOrder));
    assert_eq!(
        s.purge(LogPos {
            leader: 4,
            ..pos(20)
        }),
        Err(StoreError::Conflict)
    );
    s.purge(pos(20)).unwrap();
    s.truncate_after(Some(pos(20))).unwrap();
    assert_eq!(s.live(), Ok(0));
    s.append(&batch(21, 22)).unwrap();
    let d = private_temp();
    let mut s = open(d.path());
    s.append(&batch(5, 8)).unwrap();
    assert_eq!(s.truncate_after(Some(pos(2))), Err(StoreError::Conflict));
    assert_eq!(s.purge(pos(2)), Err(StoreError::Conflict));
    s.truncate_after(Some(pos(4))).unwrap();
    assert_eq!(s.live(), Ok(0));
}
#[test]
fn votes_use_leader_then_committed_order_without_log_vote_constraint() {
    let d = private_temp();
    let mut s = open(d.path());
    assert_eq!(s.vote(), Ok(None));
    s.append(&batch(0, 1)).unwrap();
    assert_eq!(s.vote(), Ok(None));
    let v = StoredVote {
        term: 2,
        node: 3,
        committed: true,
    };
    s.save_vote(v).unwrap();
    let writes = s.db.total_changes();
    s.save_vote(v).unwrap();
    assert_eq!(s.db.total_changes(), writes);
    for old in [
        StoredVote {
            term: 1,
            node: 9,
            committed: true,
        },
        StoredVote { node: 2, ..v },
        StoredVote {
            committed: false,
            ..v
        },
    ] {
        assert_eq!(s.save_vote(old), Err(StoreError::VoteRegressed));
        assert_eq!(s.vote(), Ok(Some(v)));
    }
    let newer = StoredVote {
        term: 3,
        node: 0,
        committed: false,
    };
    s.save_vote(newer).unwrap();
    drop(s);
    assert_eq!(open(d.path()).vote(), Ok(Some(newer)));
}
#[test]
fn reopen_rejects_row_meta_inconsistency_versions_clusters_locks_sidecars() {
    for edit in [
        "UPDATE meta SET blob=zeroblob(40)",
        "UPDATE log SET body=zeroblob(65) WHERE idx=2",
        "DELETE FROM log WHERE idx=0",
        "DELETE FROM log WHERE idx=2",
        "DELETE FROM log WHERE idx=3",
        "INSERT INTO log SELECT 4,body FROM log WHERE idx=3",
    ] {
        let d = private_temp();
        let s = open(d.path());
        drop(s);
        let mut s = open(d.path());
        s.append(&batch(0, 4)).unwrap();
        s.db.execute_batch(edit).unwrap();
        drop(s);
        assert_eq!(
            VoteLogStore::open(d.path(), [1; 16]).unwrap_err(),
            StoreError::Corrupt
        );
    }
    // Actual bit flips and valid-checksum bodies exercise both integrity and semantic scans.
    for mode in 0..6 {
        let d = private_temp();
        let mut s = open(d.path());
        s.append(&batch(0, 4)).unwrap();
        if mode == 0 {
            let mut b = encode_meta(&s.meta);
            b[20] ^= 1;
            s.db.execute("UPDATE meta SET blob=?1", [b]).unwrap();
        } else if mode == 1 {
            let mut b = encode_row(pos(2), &Entry::Blank);
            b[20] ^= 1;
            s.db.execute("UPDATE log SET body=?1 WHERE idx=2", [b])
                .unwrap();
        } else if mode == 2 {
            let b = encode_row(LogPos { term: 1, ..pos(2) }, &Entry::Blank);
            s.db.execute("UPDATE log SET body=?1 WHERE idx=2", [b])
                .unwrap();
        } else if mode == 3 {
            let mut m = s.meta.clone();
            m.tail = Some((1, pos(3)));
            s.db.execute("UPDATE meta SET blob=?1", [encode_meta(&m)])
                .unwrap();
        } else if mode == 4 {
            s.db.execute_batch("PRAGMA ignore_check_constraints=ON; UPDATE log SET body=zeroblob(4164) WHERE idx=2;").unwrap();
        } else {
            s.db.execute_batch("PRAGMA ignore_check_constraints=ON; INSERT INTO log SELECT -1,body FROM log WHERE idx=0;").unwrap();
        }
        drop(s);
        assert_eq!(
            VoteLogStore::open(d.path(), [1; 16]).unwrap_err(),
            StoreError::Corrupt
        );
    }
    let small = private_temp();
    let path = small.path().join("cluster-log.sqlite3");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    drop(options.open(&path).unwrap());
    crate::security::private(&path).unwrap();
    let db = Connection::open(&path).unwrap();
    db.execute_batch("PRAGMA page_size=1024; CREATE TABLE sentinel(x);")
        .unwrap();
    assert_eq!(
        db.pragma_query_value(None, "page_size", |r| r.get::<_, u64>(0))
            .unwrap(),
        1024
    );
    drop(db);
    crate::security::private(&path).unwrap();
    assert_eq!(
        VoteLogStore::open(small.path(), [1; 16]).unwrap_err(),
        StoreError::Unsupported
    );
    let d = private_temp();
    let s = open(d.path());
    assert_eq!(
        VoteLogStore::open(d.path(), [1; 16]).unwrap_err(),
        StoreError::Io
    );
    drop(s);
    assert_eq!(
        VoteLogStore::open(d.path(), [2; 16]).unwrap_err(),
        StoreError::WrongCluster
    );
    let s = open(d.path());
    s.db.pragma_update(None, "user_version", 2).unwrap();
    drop(s);
    assert_eq!(
        VoteLogStore::open(d.path(), [1; 16]).unwrap_err(),
        StoreError::Unsupported
    );
    for suffix in ["-journal", "-wal", "-shm"] {
        let d = private_temp();
        std::fs::create_dir(d.path().join(format!("cluster-log.sqlite3{suffix}"))).unwrap();
        assert_eq!(
            VoteLogStore::open(d.path(), [1; 16]).unwrap_err(),
            StoreError::Io
        );
    }
    #[cfg(unix)]
    {
        let d = private_temp();
        let target = d.path().join("target");
        std::fs::write(&target, []).unwrap();
        std::os::unix::fs::symlink(&target, d.path().join("cluster-log.sqlite3-journal")).unwrap();
        assert_eq!(
            VoteLogStore::open(d.path(), [1; 16]).unwrap_err(),
            StoreError::Io
        );
    }
    assert_eq!(
        open_db(d.path(), "../escape", 2048).unwrap_err(),
        StoreError::Unsupported
    );
}
#[test]
fn sqlite_faults_poison_and_leave_prestate_until_reopen() {
    for table in ["log", "meta"] {
        let d = private_temp();
        let mut s = open(d.path());
        s.append(&batch(0, 4)).unwrap();
        let before = s.meta.clone();
        let verb = if table == "log" { "INSERT" } else { "UPDATE" };
        s.db.execute_batch(&format!("CREATE TRIGGER fail_write BEFORE {verb} ON {table} BEGIN SELECT RAISE(FAIL,'synthetic'); END;")).unwrap();
        assert_eq!(s.append(&batch(4, 6)), Err(StoreError::Sql));
        assert_eq!(s.meta, before);
        assert_poisoned(&mut s);
        drop(s);
        let mut s = open(d.path());
        assert_eq!(s.meta, before);
        assert_eq!(s.entries(0, 10, 64).unwrap(), batch(0, 4));
    }
}
#[test]
fn two_store_ordering_replay_and_snapshot_install_ahead_of_tail() {
    let d = private_temp();
    let init = Init {
        cluster: [1; 16],
        first_free: NodeId::new(10).unwrap(),
    };
    let mut log = open(d.path());
    let mut sm = StateStore::open(d.path(), init).unwrap();
    let entries = payloads();
    log.append(&entries).unwrap();
    let mut reference = EnrollmentState::new(init.cluster, init.first_free);
    for (p, e) in &entries[..5] {
        let out = sm.apply(*p, e).unwrap();
        if let Entry::Enrollment(c) = e {
            assert_eq!(out, Some(reference.apply(c)));
        }
    }
    assert!(pos(4).index <= sm.applied().unwrap().index);
    log.purge(pos(4)).unwrap();
    let snap = sm.snapshot();
    drop(log);
    drop(sm);
    let mut log = open(d.path());
    let mut sm = StateStore::open(d.path(), init).unwrap();
    assert_eq!(sm.snapshot(), snap);
    assert!(log.log_state().unwrap().purged.unwrap().index <= sm.applied().unwrap().index);
    for (p, e) in log.entries(0, 10, 64).unwrap() {
        let out = sm.apply(p, &e).unwrap();
        if let Entry::Enrollment(c) = e {
            assert_eq!(out, Some(reference.apply(&c)));
        }
    }
    let source = private_temp();
    let mut source = StateStore::open(source.path(), init).unwrap();
    for (p, e) in &entries {
        source.apply(*p, e).unwrap();
    }
    for i in 7..=10 {
        source.apply(pos(i), &Entry::Blank).unwrap();
    }
    sm.install(&source.snapshot()).unwrap();
    drop(sm);
    drop(log);
    let sm = StateStore::open(d.path(), init).unwrap();
    let mut log = open(d.path());
    assert!(sm.applied().unwrap().index > log.log_state().unwrap().last.unwrap().index);
    log.purge(sm.applied().unwrap()).unwrap();
    log.append(&batch(11, 12)).unwrap();
    assert_eq!(log.entries(0, 20, 64).unwrap(), batch(11, 12));
}
fn scan(dir: &Path) {
    let markers = privacy_markers();
    for f in std::fs::read_dir(dir).unwrap() {
        assert!(absent(&std::fs::read(f.unwrap().path()).unwrap(), &markers));
    }
}
#[test]
fn credentials_absent_from_rows_meta_files_and_debug_positive_control() {
    let d = private_temp();
    let mut s = open(d.path());
    let markers = privacy_markers();
    let control = private_temp();
    for marker in &markers {
        let path = control.path().join("synthetic-positive-control");
        std::fs::write(&path, marker).unwrap();
        assert!(!absent(&std::fs::read(path).unwrap(), &markers));
    }
    s.append(&payloads()).unwrap();
    for (p, e) in payloads() {
        assert!(absent(&encode_row(p, &e), &markers));
        assert!(absent(
            format!("{e:?}{s:?}{:?}{}", StoreError::Sql, StoreError::Sql).as_bytes(),
            &markers
        ));
    }
    assert!(absent(&encode_meta(&s.meta), &markers));
    drop(s);
    scan(d.path());
}
#[test]
#[ignore = "owned current_exe process-kill fixture only"]
fn crash_child() {
    let dir = std::path::PathBuf::from(std::env::var_os("ARX_LOG_CRASH_DIR").unwrap());
    let mode = std::env::var("ARX_LOG_CRASH_MODE").unwrap();
    let mut s = open(&dir);
    s.append(&payloads()[..5]).unwrap();
    s.save_vote(StoredVote {
        term: 2,
        node: 3,
        committed: false,
    })
    .unwrap();
    s.hook = Some((dir.clone(), mode.ends_with("after")));
    if mode.starts_with("append") {
        s.append(&payloads()[5..]).unwrap();
    } else if mode.starts_with("truncate") {
        s.truncate_after(Some(pos(2))).unwrap();
    } else {
        s.save_vote(StoredVote {
            term: 3,
            node: 4,
            committed: true,
        })
        .unwrap();
    }
}
#[test]
fn four_process_kills_recover_exact_pre_post_and_replay() {
    let started = Instant::now();
    for mode in [
        "append-before",
        "append-after",
        "truncate-before",
        "vote-after",
    ] {
        let d = private_temp();
        let reference = private_temp();
        let mut r = open(reference.path());
        r.append(&payloads()[..5]).unwrap();
        r.save_vote(StoredVote {
            term: 2,
            node: 3,
            committed: false,
        })
        .unwrap();
        let pre = r.meta.clone();
        let pre_rows = r.entries(0, 10, 64).unwrap();
        if mode.starts_with("append") {
            r.append(&payloads()[5..]).unwrap();
        } else if mode.starts_with("truncate") {
            r.truncate_after(Some(pos(2))).unwrap();
        } else {
            r.save_vote(StoredVote {
                term: 3,
                node: 4,
                committed: true,
            })
            .unwrap();
        }
        let post = r.meta.clone();
        let post_rows = r.entries(0, 10, 64).unwrap();
        let mut child = KillOnDrop(
            ProcessCommand::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "cluster::log_store::tests::crash_child",
                    "--ignored",
                    "--nocapture",
                ])
                .env("ARX_LOG_CRASH_DIR", d.path())
                .env("ARX_LOG_CRASH_MODE", mode)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        while !d.path().join("ready").exists() {
            assert!(
                child.0.try_wait().unwrap().is_none(),
                "child exited: {mode}"
            );
            assert!(Instant::now() < deadline, "readiness: {mode}");
            std::thread::sleep(Duration::from_millis(10));
        }
        let journal = d.path().join("cluster-log.sqlite3-journal");
        if mode.ends_with("before") {
            use std::io::Read;
            let mut header = [0; 28];
            File::open(&journal)
                .unwrap()
                .read_exact(&mut header)
                .unwrap();
            assert_eq!(
                &header[..8],
                &[0xd9, 0xd5, 0x05, 0xf9, 0x20, 0xa1, 0x63, 0xd7]
            );
            assert!(u32::from_be_bytes(header[8..12].try_into().unwrap()) > 0);
        } else {
            assert!(std::fs::metadata(&journal).map_or(true, |m| m.len() == 0));
        }
        child.0.kill().unwrap();
        assert!(!child.0.wait().unwrap().success());
        scan(d.path());
        let mut s = open(d.path());
        if mode.ends_with("before") {
            assert_eq!(s.meta, pre);
            assert_eq!(s.entries(0, 10, 64).unwrap(), pre_rows);
            if mode.starts_with("append") {
                s.append(&payloads()[5..]).unwrap();
            } else {
                s.truncate_after(Some(pos(2))).unwrap();
            }
        } else {
            assert_eq!(s.meta, post);
            assert_eq!(s.entries(0, 10, 64).unwrap(), post_rows);
        }
        assert_eq!(s.meta, post);
        assert_eq!(s.entries(0, 10, 64).unwrap(), post_rows);
        if mode.starts_with("append") {
            assert_eq!(s.append(&payloads()[5..]), Err(StoreError::OutOfOrder));
        } else if mode.starts_with("truncate") {
            s.append(&payloads()[3..]).unwrap();
            assert_eq!(s.entries(0, 10, 64).unwrap(), payloads());
        } else {
            assert_eq!(
                s.save_vote(pre.vote.unwrap()),
                Err(StoreError::VoteRegressed)
            );
        }
        let tail = s.log_state().unwrap().last.unwrap();
        assert_eq!(
            s.append(&[(tail, Entry::Blank)]),
            Err(StoreError::OutOfOrder)
        );
        drop(s);
        scan(d.path());
        assert!(Instant::now() < deadline);
    }
    assert!(started.elapsed() < Duration::from_secs(60));
}

fn durable_bytes(s: &VoteLogStore) -> (Vec<u8>, Vec<(i64, Vec<u8>)>) {
    let meta =
        s.db.query_row("SELECT blob FROM meta", [], |r| r.get(0))
            .unwrap();
    let mut stmt =
        s.db.prepare("SELECT idx,body FROM log ORDER BY idx")
            .unwrap();
    let rows = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    (meta, rows)
}

#[test]
fn invalid_purge_and_purged_truncate_preserve_memory_and_disk() {
    for case in 0..4 {
        let d = private_temp();
        let mut s = open(d.path());
        s.append(&batch(if case == 3 { 5 } else { 1 }, 11)).unwrap();
        if case == 1 {
            s.purge(pos(20)).unwrap();
        }
        if case == 2 {
            s.purge(pos(3)).unwrap();
        }
        let before_meta = s.meta.clone();
        let before_disk = durable_bytes(&s);
        let (result, expected) = match case {
            0 => (
                s.purge(LogPos { term: 1, ..pos(20) }),
                StoreError::OutOfOrder,
            ),
            1 => (
                s.purge(LogPos { term: 1, ..pos(25) }),
                StoreError::OutOfOrder,
            ),
            2 => (
                s.truncate_after(Some(LogPos {
                    leader: 4,
                    ..pos(3)
                })),
                StoreError::Conflict,
            ),
            3 => (s.purge(LogPos { term: 3, ..pos(4) }), StoreError::Conflict),
            _ => unreachable!(),
        };
        assert_eq!(result, Err(expected), "case {case}");
        assert_eq!(s.meta, before_meta);
        assert_eq!(durable_bytes(&s), before_disk);
        drop(s);
        let reopened = open(d.path());
        assert_eq!(reopened.meta, before_meta);
        assert_eq!(durable_bytes(&reopened), before_disk);
    }
}

#[test]
fn reopen_rejects_first_row_below_purged_and_meta_position_regression() {
    let d = private_temp();
    let mut s = open(d.path());
    s.append(&batch(1, 11)).unwrap();
    s.purge(pos(3)).unwrap();
    let mut meta = s.meta.clone();
    meta.tail.as_mut().unwrap().1.term = 1;
    assert_eq!(decode_meta(&encode_meta(&meta)), Err(StoreError::Corrupt));
    let row = encode_row(LogPos { term: 1, ..pos(4) }, &Entry::Blank);
    assert!(decode_row(&row).is_ok());
    s.db.execute("UPDATE log SET body=?1 WHERE idx=4", [row])
        .unwrap();
    drop(s);
    assert!(matches!(
        VoteLogStore::open(d.path(), [1; 16]),
        Err(StoreError::Corrupt)
    ));
}

#[test]
fn reads_recheck_purged_position_and_exact_tail_id_then_poison() {
    for purged_case in [true, false] {
        let d = private_temp();
        let mut s = open(d.path());
        s.append(&batch(1, 11)).unwrap();
        let (index, changed) = if purged_case {
            s.purge(pos(3)).unwrap();
            (4, LogPos { term: 1, ..pos(4) })
        } else {
            (
                10,
                LogPos {
                    leader: 2,
                    ..pos(10)
                },
            )
        };
        let row = encode_row(changed, &Entry::Blank);
        assert!(decode_row(&row).is_ok());
        s.db.execute("UPDATE log SET body=?1 WHERE idx=?2", params![row, index])
            .unwrap();
        assert_eq!(s.entries(index, index + 1, 1), Err(StoreError::Corrupt));
        assert_eq!(s.vote(), Err(StoreError::Poisoned));
        assert_eq!(s.entries(0, 100, 64), Err(StoreError::Poisoned));
        assert_eq!(s.append(&[]), Err(StoreError::Poisoned));
    }
}

#[test]
fn registry_rows_match_independent_raw_format_and_bounds() {
    use crate::cluster::enrollment::{Command, FoundMembers, KeyDigest};
    assert_eq!(std::mem::size_of::<ClusterId>(), 16);
    for count in [0, 1, 16, 17, 64] {
        let pairs: Vec<_> = (1..=count)
            .map(|n| {
                (
                    NodeId::new(n).unwrap(),
                    KeyDigest::of_spki(&n.to_be_bytes()).unwrap(),
                )
            })
            .collect();
        let c = Entry::Enrollment(Command::Found {
            cluster: [0xab; 16],
            proposed_at_ms: 123,
            members: FoundMembers::new(pairs.clone()).unwrap(),
        });
        let row = encode_row(pos(42), &c);
        let mut raw = b"ARXLE1\0\0".to_vec();
        for n in [2u64, 3, 42] {
            raw.extend(n.to_be_bytes());
        }
        raw.push(4);
        raw.extend([0xab; 16]);
        raw.extend(123u64.to_be_bytes());
        raw.push(count as u8);
        for (n, k) in pairs {
            raw.extend(n.get().to_be_bytes());
            raw.extend(k.as_bytes());
        }
        let digest = Sha256::digest(&raw);
        raw.extend(digest);
        assert_eq!(row, raw);
        assert_eq!(row.len(), 90 + 40 * count as usize);
        assert_eq!(decode_row(&row), Ok((pos(42), c)));
        for n in 0..row.len() {
            assert_eq!(decode_row(&row[..n]), Err(StoreError::Corrupt));
        }
        bad_row(&row, |b| b[57] = 65);
        bad_row(&row, |b| {
            let end = b.len() - 32;
            b.insert(end, 0);
        });
        if count == 64 {
            bad_row(&row, |b| {
                b.drain(58..98);
            });
        }
    }
    let c = Entry::Enrollment(Command::Remove {
        cluster: [0xab; 16],
        node: NodeId::new(9).unwrap(),
        proposed_at_ms: 123,
    });
    let row = encode_row(pos(42), &c);
    let mut raw = b"ARXLE1\0\0".to_vec();
    for n in [2u64, 3, 42] {
        raw.extend(n.to_be_bytes());
    }
    raw.push(5);
    raw.extend([0xab; 16]);
    raw.extend(9u64.to_be_bytes());
    raw.extend(123u64.to_be_bytes());
    let digest = Sha256::digest(&raw);
    raw.extend(digest);
    assert_eq!(row, raw);
    assert_eq!(row.len(), 97);
    assert_eq!(decode_row(&row), Ok((pos(42), c)));
    bad_row(&row, |b| b[32] = 6);
}

#[test]
fn store_close_completion() {
    use crate::cluster::state_store::DropGate;
    use std::sync::mpsc;
    let dir = private_temp();
    let mut store = VoteLogStore::open(dir.path(), [1; 16]).unwrap();
    let released = store.released();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    store.hold_drop(DropGate {
        entered: entered_tx,
        release: release_rx,
    });
    let dropping = std::thread::spawn(move || drop(store));
    entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(!released.is_set(), "ack preceded field destruction");
    assert!(
        matches!(VoteLogStore::open(dir.path(), [1; 16]), Err(StoreError::Io)),
        "gate must hold the real file lock"
    );
    release_tx.send(()).unwrap();
    dropping.join().unwrap();
    assert!(released.is_set());
    drop(VoteLogStore::open(dir.path(), [1; 16]).unwrap());
}
