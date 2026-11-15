//! Test-only A4 parent orchestration. Pure tests supply only mock child ops.
use super::*;

pub(crate) const IMAGE_HASH: &str =
    "cee895412d58718016dd0db9883526ef8e0b13f8ed00cf213c6f77536b206369";
pub(crate) const IMAGE_SIZE: usize = 3680;
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

// Fixed test-only port: directory provenance belongs to this wrapper, never an fd.
enum Handle<'a, D> {
    Owned(D),
    Held(&'a D),
}
pub(crate) struct Tagged<'a, D> {
    handle: Handle<'a, D>,
    path: Vec<Vec<u8>>,
}
impl<D> Tagged<'_, D> {
    fn inner(&self) -> &D {
        match &self.handle {
            Handle::Owned(d) => d,
            Handle::Held(d) => d,
        }
    }
}
pub(crate) struct OmitS7<'a, F: CgroupFs> {
    fs: &'a F,
    unit: Vec<Vec<u8>>,
    attempts: std::cell::Cell<usize>,
    omitted: std::cell::Cell<usize>,
    forwarded: std::cell::Cell<usize>,
    removals: std::cell::Cell<usize>,
    invalid: std::cell::Cell<bool>,
}
impl<'a, F: CgroupFs> OmitS7<'a, F> {
    pub(crate) fn new(fs: &'a F) -> FsResult<Self> {
        let proc = fs.proc_cgroup()?;
        if proc.magic != PROC_MAGIC {
            return Err(FsError::Refused);
        }
        let location = parse::locate(&proc.bytes).ok_or(FsError::Refused)?;
        Ok(Self {
            fs,
            unit: location
                .unit_components()
                .iter()
                .map(|s| s.as_bytes().to_vec())
                .collect(),
            attempts: 0.into(),
            omitted: 0.into(),
            forwarded: 0.into(),
            removals: 0.into(),
            invalid: false.into(),
        })
    }
    fn forward_write(&self, d: &Tagged<'_, F::Dir>, file: &str, bytes: &[u8]) -> FsResult<()> {
        if file == "cgroup.kill" {
            self.forwarded.set(self.forwarded.get() + 1);
        }
        self.fs.write(d.inner(), file, bytes)
    }
    fn runs_path(&self) -> Vec<Vec<u8>> {
        let mut p = self.unit.clone();
        p.push(b"runs".to_vec());
        p
    }
    pub(crate) fn held(
        &self,
        runs: &'a F::Dir,
        leaf: &'a F::Dir,
    ) -> (Tagged<'a, F::Dir>, Tagged<'a, F::Dir>) {
        let r = self.runs_path();
        let mut l = r.clone();
        l.push(b"arx-0-0".to_vec());
        (
            Tagged {
                handle: Handle::Held(runs),
                path: r,
            },
            Tagged {
                handle: Handle::Held(leaf),
                path: l,
            },
        )
    }
}
impl<'a, F: CgroupFs> CgroupFs for OmitS7<'a, F> {
    type Dir = Tagged<'a, F::Dir>;
    fn proc_cgroup(&self) -> FsResult<ProcFile> {
        self.fs.proc_cgroup()
    }
    fn proc_status(&self) -> FsResult<ProcFile> {
        self.fs.proc_status()
    }
    fn identity(&self) -> FsResult<(u32, u32)> {
        self.fs.identity()
    }
    fn pid(&self) -> FsResult<u32> {
        self.fs.pid()
    }
    fn open_root(&self) -> FsResult<Self::Dir> {
        Ok(Tagged {
            handle: Handle::Owned(self.fs.open_root()?),
            path: vec![],
        })
    }
    fn open_child(&self, dir: &Self::Dir, name: &[u8]) -> FsResult<Self::Dir> {
        let inner = self.fs.open_child(dir.inner(), name)?;
        let mut path = dir.path.clone();
        path.push(name.to_vec());
        Ok(Tagged {
            handle: Handle::Owned(inner),
            path,
        })
    }
    fn stat(&self, d: &Self::Dir) -> FsResult<Stat> {
        self.fs.stat(d.inner())
    }
    fn read(&self, d: &Self::Dir, f: &str, m: usize) -> FsResult<Vec<u8>> {
        self.fs.read(d.inner(), f, m)
    }
    fn write(&self, d: &Self::Dir, f: &str, b: &[u8]) -> FsResult<()> {
        if f == "cgroup.kill" {
            self.attempts.set(self.attempts.get() + 1);
            if d.path != self.runs_path() || b != b"1" || self.attempts.get() != 1 {
                self.invalid.set(true);
                return Err(FsError::Refused);
            }
            self.omitted.set(self.omitted.get() + 1);
            return Ok(());
        }
        self.forward_write(d, f, b)
    }
    fn xattr_delegate(&self, d: &Self::Dir) -> FsResult<Option<Vec<u8>>> {
        self.fs.xattr_delegate(d.inner())
    }
    fn mkdir_runs(&self, d: &Self::Dir) -> FsResult<Created> {
        self.fs.mkdir_runs(d.inner())
    }
    fn list(&self, d: &Self::Dir, m: usize) -> FsResult<Vec<RawEntry>> {
        self.fs.list(d.inner(), m)
    }
    fn rmdir(&self, d: &Self::Dir, n: &[u8]) -> FsResult<()> {
        self.removals.set(self.removals.get() + 1);
        self.fs.rmdir(d.inner(), n)
    }
    fn open_ro(&self, d: &Self::Dir, f: &str) -> FsResult<()> {
        self.fs.open_ro(d.inner(), f)
    }
}

// Only the shared acceptance assertion is caught; payloads are never inspected.
pub(crate) fn assertion_rejected(accepted: bool) -> bool {
    std::panic::catch_unwind(|| acceptance(accepted)).is_err()
}
fn retained_structure<F: CgroupFs>(fs: &F, runs: &F::Dir, leaf: &F::Dir) -> bool {
    for (dir, expected) in [(runs, Some(b"arx-0-0".as_slice())), (leaf, None)] {
        if !fs.stat(dir).is_ok_and(|s| {
            s.uid == 0
                && s.mode & 0o022 == 0
                && s.mode & 0o170000 == 0o040000
                && s.magic == CGROUP_MAGIC
        }) {
            return false;
        }
        let Ok(entries) = fs.list(dir, 512) else {
            return false;
        };
        if entries.len() > 512 {
            return false;
        }
        let mut children = 0;
        for e in entries {
            match parse::classify_entry(&e.name, e.d_type, e.fstatat_mode) {
                parse::Entry::Dot | parse::Entry::Control => (),
                parse::Entry::Child if expected == Some(e.name.as_slice()) => children += 1,
                _ => return false,
            }
        }
        if children != usize::from(expected.is_some()) {
            return false;
        }
    }
    true
}

pub(crate) fn rejection<F: CgroupFs>(
    fs: &OmitS7<'_, F>,
    runs: &Tagged<'_, F::Dir>,
    leaf: &Tagged<'_, F::Dir>,
    observation: &Observation,
    panicked: bool,
    child: &mut impl ChildOps,
) -> bool {
    panicked
        && !observation.accepted
        && observation.preconditions
        && observation.outcome == Some(SetupOutcome::Unavailable(SetupFailure::KillTimeout))
        && fs.attempts.get() == 1
        && fs.omitted.get() == 1
        && fs.forwarded.get() == 0
        && !fs.invalid.get()
        && fs.removals.get() == 0
        && retained_structure(fs, runs, leaf)
        && fs.open_child(runs, b"arx-0-0").is_ok()
        && [runs, leaf].iter().all(|d| {
            fs.read(d, "cgroup.events", 4096)
                .ok()
                .and_then(|b| String::from_utf8(b).ok())
                .and_then(|s| parse::events(&s))
                == Some(true)
        })
        && child.live()
        && child.remaining().is_some_and(|d| !d.is_zero())
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
    finish_observed(fs, runs, leaf, clock, child).accepted
}

pub(crate) struct Observation {
    pub preconditions: bool,
    pub outcome: Option<SetupOutcome>,
    pub accepted: bool,
}

pub(crate) fn acceptance(accepted: bool) {
    assert!(accepted);
}

pub(crate) fn finish_observed<F: CgroupFs>(
    fs: &F,
    runs: &F::Dir,
    leaf: &F::Dir,
    clock: &dyn Clock,
    child: &mut impl ChildOps,
) -> Observation {
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
        || observe().is_err()
        || !child.live()
        || child
            .remaining()
            .is_none_or(|d| d <= Duration::from_secs(6))
    {
        return Observation {
            preconditions: false,
            outcome: None,
            accepted: false,
        };
    }
    // A5's production setup/evidence/fresh S9/literal-absence assertions apply
    // unchanged. This is the only setup call. No fixture kill on this path.
    let (outcome, accepted) = a5::finish_observed(fs, clock, &Cancellation::default());
    Observation {
        preconditions: true,
        outcome,
        accepted: accepted && child.killed(),
    }
}

#[test]
fn source_and_image_pins_are_reviewed_linux_image() {
    use sha2::{Digest, Sha256};
    let source = include_bytes!("../../../../../tools/unix-exec-validation/a4-helper/helper.rs");
    assert_eq!(hex::encode(Sha256::digest(source)), SOURCE_HASH);
    let pin: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../../tools/unix-exec-validation/a4-helper/linux-image-pin.json"
    ))
    .unwrap();
    assert_eq!(pin["image_sha256"], IMAGE_HASH);
    assert_eq!(pin["image_size"], IMAGE_SIZE);
    assert_eq!(pin["source_sha256"], SOURCE_HASH);
    assert!(
        pin["compiler_identity"]
            .as_str()
            .unwrap()
            .contains("host: x86_64-unknown-linux-gnu")
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

#[test]
fn omitted_s7_observes_real_setup_timeout_without_reap_or_removal() {
    for case in 0..16 {
        let (fs, r, l, mut child) = fixture();
        let runs = Dir(r);
        let leaf = Dir(l);
        let omitted = OmitS7::new(&fs).unwrap();
        let (runs, leaf) = omitted.held(&runs, &leaf);
        let clock = Manual::default();
        let mut observation = finish_observed(&omitted, &runs, &leaf, &clock, &mut child);
        assert!(observation.preconditions);
        assert_eq!(
            observation.outcome,
            Some(SetupOutcome::Unavailable(SetupFailure::KillTimeout))
        );
        assert!(!observation.accepted);
        assert_eq!(clock.now(), Duration::from_secs(5));
        assert_eq!(child.reaps, 0);
        assert_eq!(omitted.attempts.get(), 1);
        assert_eq!(omitted.omitted.get(), 1);
        fs.no_removal();
        assert_eq!(
            fs.0.borrow()
                .log
                .iter()
                .filter(|o| o.kind == "mkdir")
                .count(),
            1
        );
        assert!(
            !fs.0
                .borrow()
                .log
                .iter()
                .any(|o| o.kind == "write" && o.name == b"cgroup.kill")
        );
        let panicked = assertion_rejected(observation.accepted);
        match case {
            1 => observation.preconditions = false,
            2 => observation.outcome = Some(SetupOutcome::Unavailable(SetupFailure::KillInterface)),
            3 => {
                observation.outcome = Some(SetupOutcome::Ready(SetupEvidence {
                    leaves_removed: 1,
                    runs_created: false,
                    unit_written: true,
                    runs_written: true,
                }))
            }
            4 => observation.accepted = true,
            5 => omitted.attempts.set(0),
            6 => {
                let _ = omitted.write(&runs, "cgroup.kill", b"1");
            }
            7 => fs.0.borrow_mut().clear(l),
            8 => child.live = false,
            9 => child.remaining = None,
            10 => omitted.removals.set(1),
            12 => {
                fs.0.borrow_mut().child(r, b"unknown");
            }
            13 => {
                fs.0.borrow_mut().child(l, b"arx-1-0");
            }
            14 => child.remaining = Some(Duration::ZERO),
            15 => omitted.forwarded.set(1),
            _ => (),
        }
        assert_eq!(
            rejection(
                &omitted,
                &runs,
                &leaf,
                &observation,
                panicked && case != 11,
                &mut child
            ),
            case == 0,
            "case {case}"
        );
        assert_eq!(child.reaps, 0);
    }
    assert!(!assertion_rejected(true));
}

#[test]
fn omitted_s7_rejects_wrong_targets_bytes_and_duplicate_attempts() {
    for case in 0..3 {
        let (fs, r, l, _) = fixture();
        let runs = Dir(r);
        let leaf = Dir(l);
        let omitted = OmitS7::new(&fs).unwrap();
        let (runs, leaf) = omitted.held(&runs, &leaf);
        if case == 2 {
            omitted.write(&runs, "cgroup.kill", b"1").unwrap();
        }
        assert!(
            match case {
                0 => omitted.write(&leaf, "cgroup.kill", b"1"),
                1 => omitted.write(&runs, "cgroup.kill", b"0"),
                _ => omitted.write(&runs, "cgroup.kill", b"1"),
            }
            .is_err()
        );
        assert!(omitted.invalid.get());
        assert!(
            !fs.0
                .borrow()
                .log
                .iter()
                .any(|o| o.kind == "write" && o.name == b"cgroup.kill")
        );
    }
}
