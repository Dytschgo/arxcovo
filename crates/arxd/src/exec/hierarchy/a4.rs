//! Test-only A4 parent orchestration. Pure tests supply only mock child ops.
use super::*;

pub(crate) const IMAGE_HASH: &str =
    "fc814889c294db4216093e8f360201401be2c1b2dc6c4b01fd84617701f3a92e";
pub(crate) const IMAGE_SIZE: usize = 3696;
pub(crate) const SOURCE_HASH: &str =
    "770ab397264ab78deb5d54854bd00d4cfabb1b3660b0e975ee751472fbeada5a";

pub(crate) fn image(bytes: &[u8]) -> bool {
    use sha2::{Digest, Sha256};
    bytes.len() == IMAGE_SIZE && hex::encode(Sha256::digest(bytes)) == IMAGE_HASH
}

pub(crate) fn remaining(deadline: u64, last: u64, now: u64) -> Option<u64> {
    (now >= last && now < deadline).then(|| deadline - now)
}

// One bounded getdents64 batch followed by native EOF confirmation.
pub(crate) fn single_task(bytes: &[u8], pid: &[u8]) -> bool {
    if bytes.len() > 4096 {
        return false;
    }
    let mut offset = 0;
    let mut count = 0;
    while offset < bytes.len() {
        if offset + 20 > bytes.len() {
            return false;
        }
        let len = u16::from_ne_bytes([bytes[offset + 16], bytes[offset + 17]]) as usize;
        if len < 20 || offset + len > bytes.len() {
            return false;
        }
        let name = &bytes[offset + 19..offset + len];
        let Some(end) = name.iter().position(|b| *b == 0) else {
            return false;
        };
        let name = &name[..end];
        if name != b"." && name != b".." {
            if name != pid {
                return false;
            }
            count += 1;
        }
        offset += len;
    }
    count == 1
}

#[test]
fn task_inventory_and_original_deadline_refuse_bad_evidence() {
    let record = |name: &[u8]| {
        let mut r = vec![0; 24];
        r[16..18].copy_from_slice(&24u16.to_ne_bytes());
        r[19..19 + name.len()].copy_from_slice(name);
        r
    };
    let good = [record(b"."), record(b".."), record(b"17")].concat();
    assert!(single_task(&good, b"17"));
    assert!(!single_task(&good, b"18"));
    assert!(!single_task(&[], b"17"));
    assert!(!single_task(&[good.clone(), record(b"18")].concat(), b"17"));
    assert!(!single_task(&[good.clone(), record(b"17")].concat(), b"17"));
    for n in 1..24 {
        assert!(!single_task(&good[..n], b"17"));
    }
    let mut malformed = good.clone();
    malformed[16..18].fill(0);
    assert!(!single_task(&malformed, b"17"));
    assert!(!single_task(&vec![0; 4097], b"17"));
    assert_eq!(remaining(15, 2, 3), Some(12));
    assert_eq!(remaining(15, 3, 2), None);
    assert_eq!(remaining(15, 2, 15), None);
    assert_eq!(remaining(15, 2, 16), None);
    assert_eq!(remaining(15, 14, 14), Some(1));
}

/// Fixture-only controller preparation. Never places or kills a process.
pub(crate) fn limits<F: CgroupFs>(
    fs: &F,
    unit: &F::Dir,
    runs: &F::Dir,
    leaf: &F::Dir,
) -> FsResult<()> {
    for dir in [unit, runs] {
        fs.write(dir, "cgroup.subtree_control", b"+memory +pids")?;
        let bytes = fs.read(dir, "cgroup.subtree_control", 4096)?;
        let text = std::str::from_utf8(&bytes).map_err(|_| FsError::Invalid)?;
        let c = super::parse::controllers(text);
        if !c.memory || !c.pids {
            return Err(FsError::Refused);
        }
    }
    for (name, value) in [
        ("memory.max", b"67108864".as_slice()),
        ("memory.swap.max", b"0"),
        ("pids.max", b"1"),
    ] {
        fs.write(leaf, name, value)?;
    }
    for (name, value) in [
        ("memory.max", "67108864"),
        ("memory.swap.max", "0"),
        ("pids.max", "1"),
    ] {
        let bytes = fs.read(leaf, name, 4096)?;
        if bytes.len() > 4096
            || std::str::from_utf8(&bytes)
                .ok()
                .map(|s| s.trim_end_matches('\n'))
                != Some(value)
        {
            return Err(FsError::Refused);
        }
    }
    Ok(())
}

pub(crate) trait ChildOps {
    // Must establish live owned child, exactly one task and actual membership.
    fn live(&mut self) -> bool;
    fn remaining(&mut self) -> Option<Duration>;
    fn release(&mut self) -> bool;
    // Reap the retained identity; require SIGKILL and original deadline unexpired.
    fn killed(&mut self) -> bool;
}

pub(crate) fn finish<F: CgroupFs>(
    fs: &F,
    runs: &F::Dir,
    leaf: &F::Dir,
    clock: &dyn Clock,
    child: &mut impl ChildOps,
) -> bool {
    let observe = || -> FsResult<()> {
        let proc = fs.proc_cgroup()?;
        if proc.magic != PROC_MAGIC {
            return Err(FsError::Refused);
        }
        let location = parse::locate(&proc.bytes).ok_or(FsError::Refused)?;
        a5::preflight(fs, &location)?; // includes sole parent in supervisor
        for (dir, expected) in [(runs, Some(b"arx-0-0".as_slice())), (leaf, None)] {
            let st = fs.stat(dir)?;
            if st.uid != 0
                || st.mode & 0o022 != 0
                || st.mode & 0o170000 != 0o040000
                || st.magic != CGROUP_MAGIC
            {
                return Err(FsError::Refused);
            }
            let entries = fs.list(dir, 512)?;
            if entries.len() > 512 {
                return Err(FsError::TooBig);
            }
            let mut children = 0;
            for e in entries {
                match parse::classify_entry(&e.name, e.d_type, e.fstatat_mode) {
                    parse::Entry::Dot | parse::Entry::Control => (),
                    parse::Entry::Child if expected == Some(e.name.as_slice()) => children += 1,
                    _ => return Err(FsError::Refused),
                }
            }
            if children != usize::from(expected.is_some()) {
                return Err(FsError::Refused);
            }
        }
        let read = |d, n| -> FsResult<String> {
            let b = fs.read(d, n, 4096)?;
            if b.len() > 4096 {
                return Err(FsError::TooBig);
            }
            String::from_utf8(b).map_err(|_| FsError::Invalid)
        };
        if parse::pids(&read(runs, "cgroup.procs")?) != Some(vec![])
            || parse::events(&read(runs, "cgroup.events")?) != Some(true)
            || parse::events(&read(leaf, "cgroup.events")?) != Some(true)
        {
            return Err(FsError::Refused);
        }
        for (name, value) in [
            ("memory.max", "67108864"),
            ("memory.swap.max", "0"),
            ("pids.max", "1"),
        ] {
            if read(leaf, name)?.trim_end_matches('\n') != value {
                return Err(FsError::Refused);
            }
        }
        Ok(())
    };
    if observe().is_err()
        || !child.live()
        || child
            .remaining()
            .is_none_or(|d| d <= Duration::from_secs(6))
        || !child.release()
        || child
            .remaining()
            .is_none_or(|d| d <= Duration::from_secs(6))
    {
        return false;
    }
    // A5's production setup/evidence/fresh S9/literal-absence assertions apply
    // unchanged. This is the only setup call. No fixture kill on this path.
    a5::finish(fs, clock, &Cancellation::default()) && child.killed()
}

#[test]
fn source_and_image_pins_are_reviewed_cross_image() {
    use sha2::{Digest, Sha256};
    let source = include_bytes!("../../../../../tools/unix-exec-validation/a4-helper/helper.rs");
    assert_eq!(hex::encode(Sha256::digest(source)), SOURCE_HASH);
    let pin: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../../tools/unix-exec-validation/a4-helper/image-pin.json"
    ))
    .unwrap();
    assert_eq!(pin["image_sha256"], IMAGE_HASH);
    assert_eq!(pin["image_size"], IMAGE_SIZE);
    assert_eq!(pin["source_sha256"], SOURCE_HASH);
    assert!(
        pin["compiler_identity"]
            .as_str()
            .unwrap()
            .contains("host: x86_64-pc-windows-msvc")
    );
    assert!(!image(&[]));
    assert!(!image(&vec![0; IMAGE_SIZE]));
    assert!(!image(&vec![0; IMAGE_SIZE + 1]));
}

struct MockChild {
    live: bool,
    remaining: Option<Duration>,
    released: bool,
    killed: bool,
    reaps: usize,
}
impl ChildOps for MockChild {
    fn live(&mut self) -> bool {
        self.live
    }
    fn remaining(&mut self) -> Option<Duration> {
        self.remaining
    }
    fn release(&mut self) -> bool {
        self.released
    }
    fn killed(&mut self) -> bool {
        self.reaps += 1;
        self.killed
    }
}
fn fixture() -> (Fake, usize, usize, MockChild) {
    let fs = Fake::new();
    let runs = fs.prepared();
    let leaf = fs.0.borrow_mut().child(runs, b"arx-0-0");
    limits(&fs, &Dir(1), &Dir(runs), &Dir(leaf)).unwrap();
    fs.0.borrow_mut().nodes[leaf].procs = vec![17];
    fs.0.borrow_mut().log.clear();
    (
        fs,
        runs,
        leaf,
        MockChild {
            live: true,
            remaining: Some(Duration::from_secs(7)),
            released: true,
            killed: true,
            reaps: 0,
        },
    )
}
#[test]
fn populated_leaf_requires_s7_and_one_removal() {
    let (fs, r, l, mut child) = fixture();
    assert!(finish(
        &fs,
        &Dir(r),
        &Dir(l),
        &Manual::default(),
        &mut child
    ));
    assert_eq!(child.reaps, 1);
    let m = fs.0.borrow();
    assert_eq!(m.log.iter().filter(|o| o.kind == "rmdir").count(), 1);
    assert_eq!(
        m.log
            .iter()
            .filter(|o| o.kind == "write" && o.name == b"cgroup.kill")
            .count(),
        1
    );
}
#[test]
fn preconditions_refuse_without_setup_or_reap() {
    for case in 0..15 {
        let (fs, r, l, mut child) = fixture();
        match case {
            0 => child.live = false,
            1 => child.remaining = None,
            2 => child.remaining = Some(Duration::from_secs(6)),
            3 => child.released = false,
            4 => fs.0.borrow_mut().clear(l),
            5 => fs.0.borrow_mut().nodes[r].procs.push(17),
            6 => fs.0.borrow_mut().nodes[2].procs.push(17),
            10 => {
                fs.0.borrow_mut().child(l, b"nested");
            }
            11 => {
                fs.0.borrow_mut().child(r, b"unknown");
            }
            12 => fs.0.borrow_mut().nodes[l].uid = 1,
            13 => fs.0.borrow_mut().nodes[r].mode |= 0o020,
            14 => extra(&mut fs.0.borrow_mut().nodes[l], 10),
            _ => {
                fs.0.borrow_mut().nodes[l].overrides.insert(
                    ["memory.max", "memory.swap.max", "pids.max"][case - 7].into(),
                    b"max".to_vec(),
                );
            }
        }
        assert!(
            !finish(&fs, &Dir(r), &Dir(l), &Manual::default(), &mut child),
            "case {case}"
        );
        fs.no_mutation();
        assert_eq!(child.reaps, 0);
    }
}
#[test]
fn omitted_kill_residue_and_non_sigkill_never_accept() {
    for case in 0..3 {
        let (fs, r, l, mut child) = fixture();
        if case == 0 {
            fs.0.borrow_mut().kill_delay = None;
        }
        if case == 1 {
            fs.0.borrow_mut().hook = Some(Box::new(move |m, o| {
                if o.kind == "rmdir" {
                    m.child(r, b"arx-2-0");
                }
            }));
        }
        if case == 2 {
            child.killed = false;
        }
        assert!(!finish(
            &fs,
            &Dir(r),
            &Dir(l),
            &Manual::default(),
            &mut child
        ));
    }
}

#[test]
fn fixture_limits_are_ordered_and_refuse_failed_or_wrong_controls() {
    for case in 0..4 {
        let fs = Fake::new();
        let r = fs.prepared();
        let l = fs.0.borrow_mut().child(r, b"arx-0-0");
        if case == 1 {
            let mut operation = op("write", l, b"pids.max");
            operation.bytes = b"1".to_vec();
            fs.0.borrow_mut().faults.push((operation, FsError::Io));
        }
        if case >= 2 {
            fs.0.borrow_mut().hook = Some(Box::new(move |m, o| {
                if o.kind == "read" && o.dir == l && o.name == b"pids.max" {
                    m.nodes[l].overrides.insert(
                        "pids.max".into(),
                        if case == 2 {
                            b"max".to_vec()
                        } else {
                            vec![b'x'; 4097]
                        },
                    );
                }
            }));
        }
        assert_eq!(limits(&fs, &Dir(1), &Dir(r), &Dir(l)).is_ok(), case == 0);
        let m = fs.0.borrow();
        let writes: Vec<_> = m.log.iter().filter(|o| o.kind == "write").collect();
        assert_eq!(writes[0].dir, 1);
        assert_eq!(writes[1].dir, r);
        assert_eq!(writes[0].name, b"cgroup.subtree_control");
        assert_eq!(writes[1].name, b"cgroup.subtree_control");
        assert_eq!(writes[2].name, b"memory.max");
        assert_eq!(writes[3].name, b"memory.swap.max");
        assert_eq!(writes[4].name, b"pids.max");
        assert!(
            writes
                .iter()
                .all(|o| o.name != b"cgroup.kill" && o.name != b"cgroup.procs")
        );
    }
}
