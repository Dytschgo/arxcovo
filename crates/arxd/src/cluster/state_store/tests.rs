use super::*;
use crate::cluster::enrollment;
use crate::cluster::enrollment::{CODE_TTL_MS, JoinCode, KeyDigest, Reject, SKEW_MS, TOMBSTONE_MS};
use crate::service::application::tests::private_temp;
use std::{
    process::{Child, Command as ProcessCommand, Stdio},
    time::Instant,
};
fn init() -> Init {
    Init {
        cluster: [1; 16],
        first_free: NodeId::new(10).unwrap(),
    }
}
fn pos(index: u64) -> LogPos {
    LogPos {
        term: 2,
        leader: 3,
        index: index + 1,
    }
}
pub(in crate::cluster) fn code(n: u8) -> JoinCode {
    let mut call = 0;
    JoinCode::generate([1; 16], [4; 32], |b: &mut [u8]| {
        call += 1;
        b.fill(if call == 1 { n } else { 0xa7 });
        Ok::<_, ()>(())
    })
    .unwrap()
}
pub(in crate::cluster) fn mint(n: u8, time: u64) -> Entry {
    let c = code(n);
    Entry::Enrollment(Command::Mint {
        cluster: init().cluster,
        actor: [2; 16],
        request: [n; 16],
        code: c.code_id(),
        verifier: c.verifier(),
        proposed_at_ms: time,
    })
}
pub(in crate::cluster) fn redeem(n: u8, time: u64) -> Entry {
    let c = code(n);
    Entry::Enrollment(Command::redeem(
        init().cluster,
        [8; 16],
        c.code_id(),
        &c.proof(),
        KeyDigest::of_spki(&[n]).unwrap(),
        time,
    ))
}
fn open(dir: &Path) -> StateStore {
    StateStore::open(dir, init()).unwrap()
}
pub(in crate::cluster) fn found() -> Entry {
    Entry::Enrollment(Command::Found {
        cluster: [1; 16],
        members: enrollment::FoundMembers::new([(
            NodeId::new(1).unwrap(),
            KeyDigest::of_spki(b"founder").unwrap(),
        )])
        .unwrap(),
        proposed_at_ms: 0,
    })
}
fn base(s: &mut StateStore) {
    s.apply(LogPos { index: 0, ..pos(0) }, &found()).unwrap();
    s.apply(pos(0), &mint(5, 0)).unwrap();
    s.apply(
        pos(1),
        &Entry::Membership(MembershipBlob::new(b"synthetic members".to_vec()).unwrap()),
    )
    .unwrap();
}
fn rich(s: &mut StateStore) {
    base(s);
    s.apply(pos(2), &redeem(5, 1)).unwrap();
    s.apply(pos(3), &mint(6, 2)).unwrap();
    s.apply(pos(4), &mint(7, CODE_TTL_MS)).unwrap();
}
fn resign(b: &mut Vec<u8>) {
    let n = b.len() - 32;
    b.truncate(n);
    let h = Sha256::digest(&*b);
    b.extend(h);
}
fn set64(b: &mut [u8], o: usize, v: u64) {
    b[o..o + 8].copy_from_slice(&v.to_be_bytes());
}
fn corrupt(b: &[u8], edit: impl FnOnce(&mut Vec<u8>)) {
    let mut x = b.to_vec();
    edit(&mut x);
    resign(&mut x);
    assert_eq!(decode(&x), Err(StoreError::Corrupt));
}
#[test]
fn membership_size_boundary_reaches_constructor_and_decoder() {
    assert_eq!(MembershipBlob::new(Vec::new()), Err(StoreError::Corrupt));
    assert!(MembershipBlob::new(vec![0x42; 4096]).is_ok());
    assert_eq!(
        MembershipBlob::new(vec![0x42; 4097]),
        Err(StoreError::Corrupt)
    );
    let d = private_temp();
    let mut s = open(d.path());
    rich(&mut s);
    let mut image = s.image.clone();
    image.membership = Some((pos(1), MembershipBlob(vec![0x42; 4096])));
    assert_eq!(decode(&encode(&image)), Ok(image.clone()));
    image.membership = Some((pos(1), MembershipBlob(vec![0x42; 4097])));
    let oversized = encode(&image);
    assert!(oversized.len() < LIMIT);
    assert_eq!(decode(&oversized), Err(StoreError::Corrupt));
}

#[test]
fn canonical_roundtrip_and_semantic_decoder_rejections() {
    let d = private_temp();
    let mut s = open(d.path());
    rich(&mut s);
    let b = s.snapshot();
    assert_eq!(decode(&b).unwrap(), s.image);
    assert_eq!(encode(&decode(&b).unwrap()), b);
    assert_eq!(
        s.db.query_row("SELECT snapshot FROM sm", [], |r| r.get::<_, Vec<u8>>(0))
            .unwrap(),
        b
    );
    for n in 0..b.len() {
        assert_eq!(decode(&b[..n]), Err(StoreError::Corrupt));
    }
    corrupt(&b, |x| x[0] ^= 1);
    corrupt(&b, |x| x[9] = 3);
    corrupt(&b, |x| x[10] = 2);
    corrupt(&b, |x| x[35] = 2);
    // Membership starts at 36; applied position starts at 11.
    corrupt(&b, |x| set64(x, 36, 3));
    corrupt(&b, |x| set64(x, 52, 6));
    corrupt(&b, |x| {
        set64(x, 52, 5);
        set64(x, 44, 4);
    });
    corrupt(&b, |x| x[60..62].copy_from_slice(&0u16.to_be_bytes()));
    corrupt(&b, |x| x[60..62].copy_from_slice(&4097u16.to_be_bytes()));
    let start = 62 + b"synthetic members".len(); // state cluster, next, committed, count
    corrupt(&b, |x| set64(x, start + 16, 0));
    corrupt(&b, |x| set64(x, start + 24, enrollment::MAX_TIME_MS + 1));
    corrupt(&b, |x| {
        x[start + 32..start + 34].copy_from_slice(&65u16.to_be_bytes())
    });
    let first = start + 34;
    let second = first + 145;
    let third = second + 89;
    corrupt(&b, |x| {
        let id = x[first..first + 16].to_vec();
        x[second..second + 16].copy_from_slice(&id);
    });
    corrupt(&b, |x| x[first..first + 16].fill(255));
    corrupt(&b, |x| {
        let owner = x[first + 16..first + 48].to_vec();
        x[second + 16..second + 48].copy_from_slice(&owner);
    });
    corrupt(&b, |x| set64(x, first + 80, CODE_TTL_MS - 1));
    corrupt(&b, |x| set64(x, first + 80, 2 * CODE_TTL_MS + 1));
    corrupt(&b, |x| set64(x, start + 24, CODE_TTL_MS + TOMBSTONE_MS));
    corrupt(&b, |x| x[first + 88] = 2);
    corrupt(&b, |x| set64(x, first + 137, 0));
    corrupt(&b, |x| set64(x, first + 137, 11));
    // A second consumption with a duplicate node; recompute digest to reach node validation.
    corrupt(&b, |x| {
        x[second + 88] = 1;
        let consumption = x[first + 89..first + 145].to_vec();
        x.splice(second + 89..second + 89, consumption);
    });
    corrupt(&b, |x| {
        let n = x.len() - 32;
        x.insert(n, 0);
    });
    // Nine usable pending records with unique owner/request and ordered IDs.
    for count in [8u8, 9] {
        let mut many = b[..first].to_vec();
        many[start + 32..start + 34].copy_from_slice(&u16::from(count).to_be_bytes());
        for n in 1..=count {
            let mut record = b[third..third + 89].to_vec();
            record[..16].fill(n);
            record[32..48].fill(n);
            many.extend(record);
        }
        many.extend(&b[third + 89..b.len() - 32]);
        let digest = Sha256::digest(&many);
        many.extend(digest);
        if count == 8 {
            assert!(decode(&many).is_ok());
        } else {
            assert_eq!(decode(&many), Err(StoreError::Corrupt));
        }
    }
    let mut no_applied = s.image.clone();
    no_applied.applied = None;
    assert_eq!(decode(&encode(&no_applied)), Err(StoreError::Corrupt));
}
#[test]
fn open_and_order_fail_closed() {
    let d = private_temp();
    let mut s = open(d.path());
    assert!(StateStore::open(d.path(), init()).is_err());
    let initial = s.snapshot();
    assert_eq!(s.apply(pos(1), &Entry::Blank), Err(StoreError::OutOfOrder));
    assert_eq!(s.snapshot(), initial);
    base(&mut s);
    let b = s.snapshot();
    for p in [pos(1), pos(3), LogPos { term: 1, ..pos(2) }] {
        assert_eq!(s.apply(p, &Entry::Blank), Err(StoreError::OutOfOrder));
        assert_eq!(s.snapshot(), b);
    }
    drop(s);
    assert_eq!(open(d.path()).snapshot(), b);
    assert_eq!(
        StateStore::open(
            d.path(),
            Init {
                cluster: [9; 16],
                ..init()
            }
        )
        .unwrap_err(),
        StoreError::WrongCluster
    );
    let db = Connection::open(d.path().join("cluster-state.sqlite3")).unwrap();
    db.pragma_update(None, "user_version", 2).unwrap();
    drop(db);
    assert_eq!(
        StateStore::open(d.path(), init()).unwrap_err(),
        StoreError::Unsupported
    );
    let db = Connection::open(d.path().join("cluster-state.sqlite3")).unwrap();
    db.pragma_update(None, "user_version", 1).unwrap();
    let mut bad = b;
    bad[20] ^= 1;
    db.execute("UPDATE sm SET snapshot=?1", [bad]).unwrap();
    drop(db);
    assert_eq!(
        StateStore::open(d.path(), init()).unwrap_err(),
        StoreError::Corrupt
    );
}
#[test]
fn sql_fault_poison_preserves_memory_and_disk() {
    let d = private_temp();
    let mut s = open(d.path());
    base(&mut s);
    let b = s.snapshot();
    s.db.execute_batch(
        "CREATE TRIGGER fail_write BEFORE UPDATE ON sm BEGIN SELECT RAISE(FAIL,'fixture'); END;",
    )
    .unwrap();
    assert_eq!(s.apply(pos(2), &redeem(5, 1)), Err(StoreError::Sql));
    assert_eq!(s.snapshot(), b);
    assert_eq!(encode(&s.image), b);
    assert_eq!(s.apply(pos(2), &Entry::Blank), Err(StoreError::Poisoned));
    assert_eq!(s.install(&b), Err(StoreError::Poisoned));
    drop(s);
    assert_eq!(open(d.path()).snapshot(), b);
}
#[test]
fn recovery_preserves_time_dedup_pruning_and_node_counter() {
    let d = private_temp();
    let mut s = open(d.path());
    base(&mut s);
    let a = s.apply(pos(2), &redeem(5, 2 * SKEW_MS)).unwrap();
    drop(s);
    let mut s = open(d.path());
    let retry = s.apply(pos(3), &redeem(5, 2 * SKEW_MS)).unwrap();
    let (Some(Outcome::Enrolled(a)), Some(Outcome::Enrolled(b))) = (a, retry) else {
        panic!("grant expected")
    };
    assert_eq!(a.node(), b.node());
    assert!(b.replay());
    assert_eq!(
        s.apply(pos(4), &mint(6, 0)).unwrap(),
        Some(Outcome::Rejected(Reject::ClockBehind))
    );
    let prune = CODE_TTL_MS + TOMBSTONE_MS;
    assert_eq!(
        s.apply(pos(5), &redeem(5, prune)).unwrap(),
        Some(Outcome::Rejected(Reject::UnknownCode))
    );
    drop(s);
    let mut s = open(d.path());
    s.apply(pos(6), &mint(7, prune)).unwrap();
    let Some(Outcome::Enrolled(g)) = s.apply(pos(7), &redeem(7, prune)).unwrap() else {
        panic!("grant expected")
    };
    assert_eq!(g.node().get(), 11);
}
#[test]
fn install_and_replica_determinism() {
    let a = private_temp();
    let b = private_temp();
    let mut source = open(a.path());
    let mut dest = open(b.path());
    rich(&mut source);
    rich(&mut dest);
    assert_eq!(source.snapshot(), dest.snapshot());
    let old = dest.snapshot();
    assert_eq!(dest.install(&old), Err(StoreError::StaleSnapshot));
    let mut foreign = source.image.clone();
    foreign.state = EnrollmentState::new([9; 16], init().first_free);
    assert_eq!(
        dest.install(&encode(&foreign)),
        Err(StoreError::WrongCluster)
    );
    assert_eq!(dest.install(&vec![0; LIMIT + 1]), Err(StoreError::TooLarge));
    assert_eq!(dest.install(b"bad"), Err(StoreError::Corrupt));
    let mut lower = source.image.clone();
    lower.applied = Some(LogPos { term: 1, ..pos(5) });
    lower.membership = None;
    assert_eq!(
        dest.install(&encode(&lower)),
        Err(StoreError::StaleSnapshot)
    );
    assert_eq!(dest.snapshot(), old);
    assert_eq!(
        dest.db
            .query_row("SELECT snapshot FROM sm", [], |r| r.get::<_, Vec<u8>>(0))
            .unwrap(),
        old
    );
    let empty = Image {
        applied: None,
        membership: None,
        state: EnrollmentState::new(init().cluster, init().first_free),
    };
    assert_eq!(
        dest.install(&encode(&empty)),
        Err(StoreError::StaleSnapshot)
    );

    source.apply(pos(5), &Entry::Blank).unwrap();
    dest.install(&source.snapshot()).unwrap();
    assert_eq!(dest.snapshot(), source.snapshot());
    drop(dest);
    assert_eq!(open(b.path()).snapshot(), source.snapshot());
}
pub(in crate::cluster) fn absent(bytes: &[u8], markers: &[Vec<u8>]) -> bool {
    markers
        .iter()
        .all(|m| !bytes.windows(m.len()).any(|w| w == m))
}
pub(in crate::cluster) fn privacy_markers() -> Vec<Vec<u8>> {
    let c = code(5);
    let text = c.expose_for_owner_transfer();
    let raw = hex::decode(&text[6..]).unwrap();
    let secret = &raw[32..64];
    let proof = Sha256::new()
        .chain_update(b"arxcovo/join/proof/v1")
        .chain_update(c.cluster())
        .chain_update(c.code_id())
        .chain_update(secret)
        .finalize();
    let mut markers = vec![secret.to_vec(), proof.to_vec(), text.as_bytes().to_vec()];
    let encoded: Vec<_> = markers
        .iter()
        .map(|m| hex::encode(m).into_bytes())
        .collect();
    markers.extend(encoded);
    markers
}
#[test]
fn credentials_absent_from_files_snapshots_and_debug_with_positive_control() {
    let a = private_temp();
    let b = private_temp();
    let mut s = open(a.path());
    let markers = privacy_markers();
    let control = a.path().join("control");
    for marker in &markers {
        std::fs::write(&control, marker).unwrap();
        assert!(!absent(&std::fs::read(&control).unwrap(), &markers));
    }
    std::fs::remove_file(control).unwrap();
    for (i, e) in [found(), mint(5, 0), redeem(5, 1), redeem(5, 2)]
        .iter()
        .enumerate()
    {
        s.apply(
            LogPos {
                index: i as u64,
                ..pos(0)
            },
            e,
        )
        .unwrap();
        assert!(absent(
            format!("{e:?}{s:?}{}", StoreError::Sql).as_bytes(),
            &markers
        ));
    }
    let snapshot = s.snapshot();
    let mut dest = open(b.path());
    dest.install(&snapshot).unwrap();
    assert!(absent(&snapshot, &markers));
    assert!(absent(format!("{dest:?}").as_bytes(), &markers));
    drop(s);
    drop(dest);
    for d in [a.path(), b.path()] {
        for file in std::fs::read_dir(d).unwrap() {
            assert!(absent(
                &std::fs::read(file.unwrap().path()).unwrap(),
                &markers
            ));
        }
    }
}
pub(in crate::cluster) fn pause(path: &Path) {
    std::fs::write(path.join("ready"), b"ready").unwrap();
    std::thread::sleep(Duration::from_secs(10));
    panic!("parent kill deadline");
}
pub(in crate::cluster) struct KillOnDrop(pub(in crate::cluster) Child);
impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[test]
#[ignore = "owned process-kill fixture only"]
fn crash_child() {
    let d = std::path::PathBuf::from(std::env::var_os("ARX_SM_CRASH_DIR").unwrap());
    let mode = std::env::var("ARX_SM_CRASH_MODE").unwrap();
    let mut s = open(&d);
    base(&mut s);
    let mut image = s.image.clone();
    image.applied = Some(pos(2));
    image.state.apply(match &redeem(5, 1) {
        Entry::Enrollment(c) => c,
        _ => unreachable!(),
    });
    s.hook = Some((d.clone(), mode.ends_with("after")));
    if mode.starts_with("install") {
        s.install(&encode(&image)).unwrap();
    } else {
        s.apply(pos(2), &redeem(5, 1)).unwrap();
    }
}
#[test]
fn four_process_kills_recover_exact_reference_and_next_index() {
    let started = Instant::now();
    for mode in [
        "apply-before",
        "apply-after",
        "install-before",
        "install-after",
    ] {
        let d = private_temp();
        let mut s = open(d.path());
        let initial = s.snapshot();
        base(&mut s);
        let pre = s.snapshot();
        let outcome = s.apply(pos(2), &redeem(5, 1)).unwrap();
        let post = s.snapshot();
        drop(s);
        // Independent reference replay, then start the child from k.
        let reference = private_temp();
        let mut r = open(reference.path());
        base(&mut r);
        assert_eq!(r.snapshot(), pre);
        assert_eq!(r.apply(pos(2), &redeem(5, 1)).unwrap(), outcome);
        assert_eq!(r.snapshot(), post);
        let db = Connection::open(d.path().join("cluster-state.sqlite3")).unwrap();
        db.execute("UPDATE sm SET snapshot=?1", [&initial]).unwrap();
        drop(db);
        let mut child = KillOnDrop(
            ProcessCommand::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "cluster::state_store::tests::crash_child",
                    "--ignored",
                    "--nocapture",
                ])
                .env("ARX_SM_CRASH_DIR", d.path())
                .env("ARX_SM_CRASH_MODE", mode)
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
        if mode.ends_with("before") {
            use std::io::Read;
            let mut header = [0; 28];
            File::open(d.path().join("cluster-state.sqlite3-journal"))
                .unwrap()
                .read_exact(&mut header)
                .unwrap();
            assert_eq!(
                &header[..8],
                &[0xd9, 0xd5, 0x05, 0xf9, 0x20, 0xa1, 0x63, 0xd7]
            );
            assert!(u32::from_be_bytes(header[8..12].try_into().unwrap()) > 0);
        }
        child.0.kill().unwrap();
        assert!(!child.0.wait().unwrap().success());
        let markers = privacy_markers();
        for file in std::fs::read_dir(d.path()).unwrap() {
            assert!(absent(
                &std::fs::read(file.unwrap().path()).unwrap(),
                &markers
            ));
        }
        let mut recovered = open(d.path());
        if mode.ends_with("before") {
            assert_eq!(recovered.snapshot(), pre);
            assert_eq!(recovered.apply(pos(2), &redeem(5, 1)).unwrap(), outcome);
        } else {
            assert_eq!(recovered.snapshot(), post);
            assert_eq!(
                recovered.apply(pos(2), &redeem(5, 1)),
                Err(StoreError::OutOfOrder)
            );
        }
        assert_eq!(recovered.snapshot(), post);
        recovered.apply(pos(3), &Entry::Blank).unwrap();
        assert_eq!(
            recovered.apply(pos(3), &Entry::Blank),
            Err(StoreError::OutOfOrder)
        );
        assert!(Instant::now() < deadline, "child budget: {mode}");
    }
    assert!(started.elapsed() < Duration::from_secs(60));
}

#[test]
fn maximal_state_is_bounded_and_unsafe_sidecars_are_refused() {
    let d = private_temp();
    let mut s = open(d.path());
    s.apply(LogPos { index: 0, ..pos(0) }, &found()).unwrap();
    let mut index = 0;
    for n in 1..=64 {
        s.apply(pos(index), &mint(n, 0)).unwrap();
        index += 1;
        assert!(matches!(
            s.apply(pos(index), &redeem(n, 0)).unwrap(),
            Some(Outcome::Enrolled(_))
        ));
        index += 1;
        if n <= 49 {
            s.apply(
                pos(index),
                &Entry::Enrollment(Command::Remove {
                    cluster: [1; 16],
                    node: NodeId::new(9 + u64::from(n)).unwrap(),
                    proposed_at_ms: 0,
                }),
            )
            .unwrap();
            index += 1;
        }
    }
    s.apply(
        pos(index),
        &Entry::Membership(MembershipBlob::new(vec![0x42; 4096]).unwrap()),
    )
    .unwrap();
    let b = s.snapshot();
    assert!(b.len() <= LIMIT);
    assert_eq!(decode(&b).unwrap(), s.image);
    drop(s);
    assert_eq!(open(d.path()).snapshot(), b);
    for suffix in ["-journal", "-wal", "-shm"] {
        let d = private_temp();
        std::fs::create_dir(d.path().join(format!("cluster-state.sqlite3{suffix}"))).unwrap();
        assert_eq!(
            StateStore::open(d.path(), init()).unwrap_err(),
            StoreError::Io
        );
    }
    #[cfg(unix)]
    {
        use std::os::unix::{fs::PermissionsExt, fs::symlink};
        let d = private_temp();
        let target = d.path().join("target");
        std::fs::write(&target, []).unwrap();
        symlink(&target, d.path().join("cluster-state.sqlite3-journal")).unwrap();
        assert!(StateStore::open(d.path(), init()).is_err());
        std::fs::remove_file(d.path().join("cluster-state.sqlite3-journal")).unwrap();
        std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(StateStore::open(d.path(), init()).is_err());
    }
}

#[test]
fn v1_checksum_valid_refused_without_database_mutation() {
    let d = private_temp();
    let mut store = open(d.path());
    base(&mut store);
    // Independent old v1 empty image, no v2 registry suffix.
    let mut old = b"ARXSM1\0\0".to_vec();
    old.extend(1u16.to_be_bytes());
    old.extend([0, 0]);
    old.extend([1; 16]);
    old.extend(10u64.to_be_bytes());
    old.extend(0u64.to_be_bytes());
    old.extend(0u16.to_be_bytes());
    let digest = Sha256::digest(&old);
    old.extend(digest);
    let before = store.snapshot();
    assert_eq!(StateStore::peek(&old), Err(StoreError::Unsupported));
    assert_eq!(store.install(&old), Err(StoreError::Unsupported));
    assert_eq!(store.snapshot(), before);
    store
        .db
        .execute("UPDATE sm SET snapshot=?1", [&old])
        .unwrap();
    drop(store);
    let file = d.path().join("cluster-state.sqlite3");
    let bytes = std::fs::read(&file).unwrap();
    assert_eq!(
        StateStore::open(d.path(), init()).unwrap_err(),
        StoreError::Unsupported
    );
    assert_eq!(std::fs::read(file).unwrap(), bytes);
}
#[test]
fn maximum_v2_image_and_registry_semantics_with_recomputed_checksums() {
    // Build bytes independently: 64 consumed records, 16 current members and maximum
    // actual membership codec payload (two 16-voter configs, 16 200-byte addresses).
    let mut membership = vec![1, 2];
    for _ in 0..2 {
        membership.extend([16]);
        for n in 1..=16u64 {
            membership.extend(n.to_be_bytes());
        }
    }
    membership.push(16);
    for n in 1..=16u64 {
        membership.extend(n.to_be_bytes());
        membership.push(200);
        membership.extend([b'a'; 200]);
    }
    assert_eq!(membership.len(), 3605);
    let mut b = b"ARXSM1\0\0".to_vec();
    b.extend(2u16.to_be_bytes());
    b.push(1);
    put_pos(&mut b, pos(1));
    b.push(1);
    put_pos(&mut b, pos(0));
    b.extend((membership.len() as u16).to_be_bytes());
    b.extend(membership);
    b.extend([1; 16]);
    let next = b.len();
    b.extend(74u64.to_be_bytes());
    b.extend(0u64.to_be_bytes());
    b.extend(64u16.to_be_bytes());
    for n in 0..64u8 {
        b.extend([n; 16]);
        b.extend([2; 16]);
        b.extend([n; 16]);
        b.extend([3; 32]);
        b.extend(CODE_TTL_MS.to_be_bytes());
        b.push(1);
        b.extend([8; 16]);
        b.extend([n; 32]);
        b.extend((10 + u64::from(n)).to_be_bytes());
    }
    let flag = b.len();
    b.extend([1, 16]);
    for n in 48..64u8 {
        b.extend((10 + u64::from(n)).to_be_bytes());
        b.extend([n; 32]);
    }
    let digest = Sha256::digest(&b);
    b.extend(digest);
    assert_eq!(b.len(), 13655);
    assert!(b.len() < 16384);
    let image = decode(&b).unwrap();
    assert_eq!(encode(&image), b);
    for n in 0..b.len() {
        assert_eq!(decode(&b[..n]), Err(StoreError::Corrupt));
    }
    corrupt(&b, |x| x[flag] = 0); // Unfounded records/members.
    corrupt(&b, |x| x[flag + 1] = 17);
    corrupt(&b, |x| {
        x.drain(flag + 2..flag + 2 + 640);
        x[flag + 1] = 0;
    }); // Founded empty.
    corrupt(&b, |x| {
        let k = x[flag + 10..flag + 42].to_vec();
        x[flag + 50..flag + 82].copy_from_slice(&k);
    });
    corrupt(&b, |x| {
        let n = x[flag + 2..flag + 10].to_vec();
        x[flag + 42..flag + 50].copy_from_slice(&n);
    });
    corrupt(&b, |x| {
        let a = x[flag + 2..flag + 42].to_vec();
        let z = x[flag + 42..flag + 82].to_vec();
        x[flag + 2..flag + 42].copy_from_slice(&z);
        x[flag + 42..flag + 82].copy_from_slice(&a);
    });
    corrupt(&b, |x| set64(x, flag + 2, 74));
    // Keep order and consumed-record consistency so only duplicate member keys reject.
    corrupt(&b, |x| {
        let duplicate = x[flag + 10..flag + 42].to_vec();
        x[flag + 50..flag + 82].copy_from_slice(&duplicate);
        let consumed_key = next + 18 + 49 * 145 + 105;
        x[consumed_key..consumed_key + 32].copy_from_slice(&duplicate);
    });
    // The last member has no matching consumed record at74; ordering remains valid.
    corrupt(&b, |x| set64(x, flag + 2 + 15 * 40, 74));
    corrupt(&b, |x| x[flag + 10] ^= 1); // Current consumed member has different key.
    corrupt(&b, |x| set64(x, next, 58));
    corrupt(&b, |x| {
        x.insert(x.len() - 32, 0);
    });
    let d = private_temp();
    let mut store = open(d.path());
    store.install(&b).unwrap();
    assert_eq!(store.snapshot(), b);
    drop(store);
    assert_eq!(open(d.path()).snapshot(), b);
    // No members but retained records while unfounded is independently invalid.
    corrupt(&b, |x| {
        x.drain(flag + 2..flag + 2 + 640);
        x[flag] = 0;
        x[flag + 1] = 0;
    });
}
