//! Test-only kernel model. Handles identify objects, not host paths.
use super::{parse, setup::*};
use crate::development::Cancellation;
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};

#[derive(Clone, Copy, PartialEq, Eq)]
struct Dir(usize);
struct Node {
    parent: Option<usize>,
    children: BTreeMap<Vec<u8>, usize>,
    uid: u32,
    mode: u32,
    magic: u64,
    enabled: BTreeSet<String>,
    available: BTreeSet<String>,
    procs: Vec<u32>,
    frozen: bool,
    overrides: BTreeMap<String, Vec<u8>>,
    extra: Vec<RawEntry>,
    missing: BTreeSet<String>,
}
impl Node {
    fn new(parent: Option<usize>) -> Self {
        Self {
            parent,
            children: BTreeMap::new(),
            uid: 0,
            mode: 0o040700,
            magic: CGROUP_MAGIC,
            enabled: BTreeSet::new(),
            available: BTreeSet::new(),
            procs: vec![],
            frozen: false,
            overrides: BTreeMap::new(),
            extra: vec![],
            missing: BTreeSet::new(),
        }
    }
}
#[derive(Clone, PartialEq, Eq)]
struct Op {
    kind: &'static str,
    dir: usize,
    name: Vec<u8>,
    bytes: Vec<u8>,
}
type Hook = Box<dyn FnMut(&mut Model, &Op)>;
struct Model {
    nodes: Vec<Node>,
    log: Vec<Op>,
    faults: Vec<(Op, FsError)>,
    hook: Option<Hook>,
    status: ProcFile,
    cgroup: ProcFile,
    identity: (u32, u32),
    delegate: Option<Vec<u8>>,
    kill_delay: Option<usize>,
    killing: Option<(usize, usize)>,
    panic_remove: bool,
}
struct Fake(RefCell<Model>);
fn op(kind: &'static str, dir: usize, name: &[u8]) -> Op {
    Op {
        kind,
        dir,
        name: name.to_vec(),
        bytes: vec![],
    }
}
impl Model {
    fn child(&mut self, parent: usize, name: &[u8]) -> usize {
        let id = self.nodes.len();
        self.nodes.push(Node::new(Some(parent)));
        self.nodes[parent].children.insert(name.to_vec(), id);
        id
    }
    fn populated(&self, dir: usize) -> bool {
        !self.nodes[dir].procs.is_empty()
            || self.nodes[dir]
                .children
                .values()
                .any(|d| self.populated(*d))
    }
    fn clear(&mut self, dir: usize) {
        self.nodes[dir].procs.clear();
        let ids: Vec<_> = self.nodes[dir].children.values().copied().collect();
        for d in ids {
            self.clear(d);
        }
    }
    fn available(&self, dir: usize) -> BTreeSet<String> {
        self.nodes[dir].parent.map_or_else(
            || self.nodes[dir].available.clone(),
            |p| self.nodes[p].enabled.clone(),
        )
    }
    fn control_files(&self, dir: usize) -> Vec<String> {
        let mut files = [
            "cgroup.controllers",
            "cgroup.subtree_control",
            "cgroup.events",
            "cgroup.procs",
            "cgroup.type",
            "cgroup.kill",
        ]
        .map(str::to_owned)
        .to_vec();
        let available = self.available(dir);
        if available.contains("memory") {
            files.extend(["memory.max", "memory.swap.max"].map(str::to_owned));
        }
        if available.contains("pids") {
            files.push("pids.max".into());
        }
        files.retain(|f| !self.nodes[dir].missing.contains(f));
        files
    }
}
impl Fake {
    fn new() -> Self {
        let mut m = Model {
            nodes: vec![Node::new(None)],
            log: vec![],
            faults: vec![],
            hook: None,
            status: ProcFile {
                magic: PROC_MAGIC,
                bytes: b"NoNewPrivs:\t1\nCapBnd:\t00000000000000c1\n".to_vec(),
            },
            cgroup: ProcFile {
                magic: PROC_MAGIC,
                bytes: b"0::/arxd.service/supervisor\n".to_vec(),
            },
            identity: (0, 0),
            delegate: Some(b"1".to_vec()),
            kill_delay: Some(2),
            killing: None,
            panic_remove: false,
        };
        m.nodes[0].available = ["memory", "pids"].map(str::to_owned).into();
        m.nodes[0].enabled = m.nodes[0].available.clone();
        let unit = m.child(0, b"arxd.service");
        let supervisor = m.child(unit, b"supervisor");
        m.nodes[supervisor].procs = vec![42];
        Self(RefCell::new(m))
    }
    fn call<T>(&self, operation: Op, f: impl FnOnce(&mut Model) -> FsResult<T>) -> FsResult<T> {
        let mut m = self.0.borrow_mut();
        m.log.push(operation.clone());
        if let Some(mut hook) = m.hook.take() {
            hook(&mut m, &operation);
            m.hook = Some(hook);
        }
        if let Some((_, error)) = m.faults.iter().find(|(o, _)| *o == operation) {
            return Err(*error);
        }
        f(&mut m)
    }
    fn prepared(&self) -> usize {
        self.0.borrow_mut().child(1, b"runs")
    }
    fn leaf(&self, runs: usize, name: &[u8]) -> usize {
        let mut m = self.0.borrow_mut();
        let d = m.child(runs, name);
        m.nodes[d].procs = vec![17];
        d
    }
    fn run(&self, token: &Cancellation, clock: &Manual) -> SetupOutcome {
        setup(
            self,
            &parse::locate(b"0::/arxd.service/supervisor").unwrap(),
            clock,
            token,
        )
    }
    fn outcome(&self) -> SetupOutcome {
        self.run(&Cancellation::default(), &Manual::default())
    }
    fn fail(&self, f: SetupFailure) {
        assert_eq!(self.outcome(), SetupOutcome::Unavailable(f));
    }
    fn no_removal(&self) {
        assert!(!self.0.borrow().log.iter().any(|o| o.kind == "rmdir"));
    }
    fn no_mutation(&self) {
        assert!(
            !self
                .0
                .borrow()
                .log
                .iter()
                .any(|o| ["write", "mkdir"].contains(&o.kind))
        );
    }
}
impl CgroupFs for Fake {
    type Dir = Dir;
    fn proc_status(&self) -> FsResult<ProcFile> {
        self.call(op("status", 0, b""), |m| {
            Ok(ProcFile {
                magic: m.status.magic,
                bytes: m.status.bytes.clone(),
            })
        })
    }
    fn proc_cgroup(&self) -> FsResult<ProcFile> {
        self.call(op("cgroup", 0, b""), |m| {
            Ok(ProcFile {
                magic: m.cgroup.magic,
                bytes: m.cgroup.bytes.clone(),
            })
        })
    }
    fn identity(&self) -> FsResult<(u32, u32)> {
        self.call(op("identity", 0, b""), |m| Ok(m.identity))
    }
    fn pid(&self) -> FsResult<u32> {
        self.call(op("pid", 0, b""), |_| Ok(42))
    }
    fn open_root(&self) -> FsResult<Dir> {
        self.call(op("root", 0, b""), |_| Ok(Dir(0)))
    }
    fn open_child(&self, d: &Dir, name: &[u8]) -> FsResult<Dir> {
        self.call(op("child", d.0, name), |m| {
            m.nodes[d.0]
                .children
                .get(name)
                .copied()
                .map(Dir)
                .ok_or(FsError::Missing)
        })
    }
    fn stat(&self, d: &Dir) -> FsResult<Stat> {
        self.call(op("stat", d.0, b""), |m| {
            let n = &m.nodes[d.0];
            Ok(Stat {
                uid: n.uid,
                mode: n.mode,
                magic: n.magic,
            })
        })
    }
    fn xattr_delegate(&self, d: &Dir) -> FsResult<Option<Vec<u8>>> {
        self.call(op("xattr", d.0, b""), |m| Ok(m.delegate.clone()))
    }
    fn read(&self, d: &Dir, file: &str, max: usize) -> FsResult<Vec<u8>> {
        self.call(op("read", d.0, file.as_bytes()), |m| {
            if !m.control_files(d.0).iter().any(|f| f == file) {
                return Err(FsError::Missing);
            }
            if file == "cgroup.events"
                && let Some((root, polls)) = m.killing
                && root == d.0
                && let Some(delay) = m.kill_delay
            {
                if polls >= delay {
                    m.clear(root);
                    m.killing = None;
                } else {
                    m.killing = Some((root, polls + 1));
                }
            }
            let n = &m.nodes[d.0];
            let bytes = if let Some(v) = n.overrides.get(file) {
                v.clone()
            } else {
                match file {
                    "cgroup.controllers" => m
                        .available(d.0)
                        .into_iter()
                        .collect::<Vec<_>>()
                        .join(" ")
                        .into_bytes(),
                    "cgroup.subtree_control" => n
                        .enabled
                        .iter()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(" ")
                        .into_bytes(),
                    "cgroup.type" => b"domain\n".to_vec(),
                    "cgroup.procs" => n
                        .procs
                        .iter()
                        .map(|p| format!("{p}\n"))
                        .collect::<String>()
                        .into_bytes(),
                    "cgroup.events" => format!(
                        "populated {}\nfrozen {}\n",
                        u8::from(m.populated(d.0)),
                        u8::from(n.frozen)
                    )
                    .into_bytes(),
                    _ => b"max\n".to_vec(),
                }
            };
            if bytes.len() > max {
                Err(FsError::TooBig)
            } else {
                Ok(bytes)
            }
        })
    }
    fn write(&self, d: &Dir, file: &str, bytes: &[u8]) -> FsResult<()> {
        let mut operation = op("write", d.0, file.as_bytes());
        operation.bytes = bytes.to_vec();
        self.call(operation, |m| {
            if file == "cgroup.kill" && bytes == b"1" {
                m.killing = Some((d.0, 0));
                return Ok(());
            }
            if file != "cgroup.subtree_control" {
                return Err(FsError::Invalid);
            }
            if !m.nodes[d.0].procs.is_empty() {
                return Err(FsError::Busy);
            }
            let s = std::str::from_utf8(bytes).map_err(|_| FsError::Invalid)?;
            let tokens: Vec<_> = s.split_whitespace().collect();
            let available = m.available(d.0);
            if tokens
                .iter()
                .any(|t| !t.starts_with('+') || !available.contains(&t[1..]))
            {
                return Err(FsError::Invalid);
            }
            for t in tokens {
                m.nodes[d.0].enabled.insert(t[1..].to_owned());
            }
            Ok(())
        })
    }
    fn mkdir_runs(&self, d: &Dir) -> FsResult<Created> {
        self.call(op("mkdir", d.0, b"runs"), |m| {
            if m.nodes[d.0].children.contains_key(b"runs".as_slice()) {
                return Ok(Created::Exists);
            }
            m.child(d.0, b"runs");
            Ok(Created::Created)
        })
    }
    fn list(&self, d: &Dir, max: usize) -> FsResult<Vec<RawEntry>> {
        self.call(op("list", d.0, b""), |m| {
            let entry = |name: Vec<u8>, d_type| RawEntry {
                name,
                d_type,
                fstatat_mode: None,
            };
            let mut r = vec![entry(b".".to_vec(), 4), entry(b"..".to_vec(), 4)];
            r.extend(
                m.control_files(d.0)
                    .into_iter()
                    .map(|s| entry(s.into_bytes(), 8)),
            );
            r.extend(m.nodes[d.0].children.keys().cloned().map(|s| entry(s, 4)));
            r.extend(m.nodes[d.0].extra.iter().map(|e| RawEntry {
                name: e.name.clone(),
                d_type: e.d_type,
                fstatat_mode: e.fstatat_mode,
            }));
            if r.len() > max {
                Err(FsError::TooBig)
            } else {
                Ok(r)
            }
        })
    }
    fn rmdir(&self, d: &Dir, name: &[u8]) -> FsResult<()> {
        self.call(op("rmdir", d.0, name), |m| {
            if m.panic_remove {
                panic!("ARX-H15-FIXED");
            }
            let id = *m.nodes[d.0].children.get(name).ok_or(FsError::Missing)?;
            if m.populated(id) || !m.nodes[id].children.is_empty() {
                return Err(FsError::Busy);
            }
            m.nodes[d.0].children.remove(name);
            Ok(())
        })
    }
    fn open_ro(&self, d: &Dir, file: &str) -> FsResult<()> {
        self.call(op("ro", d.0, file.as_bytes()), |m| {
            if m.control_files(d.0).iter().any(|f| f == file) {
                Ok(())
            } else {
                Err(FsError::Missing)
            }
        })
    }
}
#[derive(Default)]
struct Manual {
    now: Cell<Duration>,
    cancel: Option<Cancellation>,
}
impl Clock for Manual {
    fn now(&self) -> Duration {
        self.now.get()
    }
    fn sleep(&self, duration: Duration) {
        assert_eq!(duration, Duration::from_millis(5));
        assert!(
            self.now.get() <= Duration::from_secs(5),
            "manual clock bound"
        );
        self.now.set(self.now.get() + duration);
        if let Some(c) = &self.cancel {
            c.cancel();
        }
    }
}
fn ready(outcome: SetupOutcome, removed: u8, created: bool) {
    assert_eq!(
        outcome,
        SetupOutcome::Ready(SetupEvidence {
            leaves_removed: removed,
            runs_created: created,
            unit_written: true,
            runs_written: true
        })
    );
}
fn extra(n: &mut Node, kind: u8) {
    n.extra.push(RawEntry {
        name: b"arx-1-2".to_vec(),
        d_type: kind,
        fstatat_mode: Some(Err(())),
    });
}

#[test]
fn h1_fresh_top_down() {
    let f = Fake::new();
    ready(f.outcome(), 0, true);
    let m = f.0.borrow();
    let mutations: Vec<_> = m
        .log
        .iter()
        .filter(|o| ["write", "mkdir"].contains(&o.kind))
        .collect();
    assert_eq!(mutations.len(), 4);
    assert!(mutations[0].dir == 1 && mutations[0].bytes == b"+memory +pids");
    assert!(mutations[1].kind == "mkdir" && m.nodes[3].mode == 0o040700);
    assert!(
        mutations[2].dir == 3 && mutations[2].name == b"cgroup.kill" && mutations[2].bytes == b"1"
    );
    assert!(mutations[3].dir == 3 && mutations[3].bytes == b"+memory +pids");
}
#[test]
fn h2_prepared_idempotent() {
    let f = Fake::new();
    f.prepared();
    ready(f.outcome(), 0, false);
    ready(f.outcome(), 0, false);
    assert_eq!(
        f.0.borrow()
            .log
            .iter()
            .filter(|o| o.kind == "write")
            .count(),
        6
    );
}
#[test]
fn h3_leftovers_kill_wait_validate_remove() {
    let f = Fake::new();
    let r = f.prepared();
    f.leaf(r, b"arx-1-0");
    f.leaf(r, b"arx-2-0");
    ready(f.outcome(), 2, false);
    let m = f.0.borrow();
    let kill = m
        .log
        .iter()
        .position(|o| o.kind == "write" && o.name == b"cgroup.kill")
        .unwrap();
    assert_eq!(
        m.log
            .iter()
            .filter(|o| o.kind == "write" && o.name == b"cgroup.kill")
            .count(),
        1
    );
    assert!(m.log.iter().position(|o| o.kind == "list").unwrap() > kill);
    let first_remove = m.log.iter().position(|o| o.kind == "rmdir").unwrap();
    assert!(
        m.log
            .iter()
            .position(|o| o.kind == "read" && o.dir == 5 && o.name == b"cgroup.events")
            .unwrap()
            < first_remove
    );
    assert!(!m.populated(r));
}
#[test]
fn h4_deadline() {
    let f = Fake::new();
    let r = f.prepared();
    f.leaf(r, b"arx-1-0");
    f.0.borrow_mut().kill_delay = None;
    f.fail(SetupFailure::KillTimeout);
    f.no_removal();
    assert_eq!(SetupFailure::KillTimeout.cause(), SetupCause::Janitor);
}
#[test]
fn h5_removal_busy() {
    let f = Fake::new();
    let r = f.prepared();
    f.leaf(r, b"arx-1-0");
    f.0.borrow_mut()
        .faults
        .push((op("rmdir", r, b"arx-1-0"), FsError::Busy));
    f.fail(SetupFailure::LeafRemoval);
}
#[test]
fn h6_all_names_before_removal() {
    for name in [b"zzz".as_slice(), b"arx-01-0", b"arx-9-00"] {
        let f = Fake::new();
        let r = f.prepared();
        f.leaf(r, b"arx-1-0");
        f.leaf(r, name);
        f.fail(SetupFailure::LeafStructure);
        f.no_removal();
        assert!(!f.0.borrow().populated(r));
    }
}
#[test]
fn h7_nested() {
    let f = Fake::new();
    let r = f.prepared();
    let l = f.leaf(r, b"arx-1-0");
    f.leaf(l, b"arx-2-0");
    f.fail(SetupFailure::LeafStructure);
    f.no_removal();
    assert!(!f.0.borrow().populated(r));
}
#[test]
fn h8_types_and_trust() {
    for leaf in [false, true] {
        for kind in [10, 2, 6, 1, 12, 8, 0] {
            let f = Fake::new();
            let r = f.prepared();
            let l = f.leaf(r, b"arx-1-0");
            extra(&mut f.0.borrow_mut().nodes[if leaf { l } else { r }], kind);
            f.fail(SetupFailure::LeafStructure);
            f.no_removal();
        }
    }
    for field in 0..4 {
        let f = Fake::new();
        let r = f.prepared();
        let l = f.leaf(r, b"arx-1-0");
        {
            let mut m = f.0.borrow_mut();
            match field {
                0 => m.nodes[l].magic = PROC_MAGIC,
                1 => m.nodes[l].uid = 1,
                2 => m.nodes[l].mode |= 0o020,
                _ => m.nodes[l].mode |= 0o002,
            }
        }
        f.fail(SetupFailure::LeafStructure);
        f.no_removal();
    }
}
#[test]
fn h9_bounds() {
    let f = Fake::new();
    let r = f.prepared();
    for i in 0..65 {
        f.leaf(r, format!("arx-1-{i}").as_bytes());
    }
    f.fail(SetupFailure::LeafStructure);
    f.no_removal();
    for count in [512, 513] {
        let f = Fake::new();
        let r = f.prepared();
        {
            let mut m = f.0.borrow_mut();
            for i in 0..count - 11 {
                m.nodes[r].extra.push(RawEntry {
                    name: format!("future.k{i}").into_bytes(),
                    d_type: 8,
                    fstatat_mode: None,
                });
            }
        }
        if count == 512 {
            ready(f.outcome(), 0, false);
        } else {
            f.fail(SetupFailure::LeafStructure);
        }
    }
    let f = Fake::new();
    f.0.borrow_mut().nodes[1]
        .overrides
        .insert("cgroup.type".into(), vec![b'x'; 4097]);
    f.fail(SetupFailure::UnitState);
    f.no_mutation();
}
#[test]
fn h10_valid_foreign_location_refused_before_mutation() {
    let f = Fake::new();
    let bytes = b"0::/other.slice/arxd.service/supervisor";
    assert!(parse::locate(bytes).is_some());
    f.0.borrow_mut().cgroup.bytes = bytes.to_vec();
    f.fail(SetupFailure::Locate);
    f.no_mutation();
}
#[test]
fn h10_anchors_zero_mutations() {
    use SetupFailure::*;
    for case in 0..21 {
        let f = Fake::new();
        let expected;
        {
            let mut m = f.0.borrow_mut();
            expected = match case {
                0 => {
                    m.identity = (1, 0);
                    Identity
                }
                1 => {
                    m.identity = (0, 1);
                    Identity
                }
                2 => {
                    m.cgroup.magic = CGROUP_MAGIC;
                    Locate
                }
                3 => {
                    m.cgroup.bytes = vec![b'x'; 4097];
                    Locate
                }
                4 => {
                    m.cgroup.bytes = b"0::/other.service/supervisor".to_vec();
                    Locate
                }
                5 => {
                    m.nodes[0].mode |= 0o020;
                    Root
                }
                6 => {
                    m.nodes[0].uid = 1;
                    Root
                }
                7 => {
                    m.nodes[0].magic = 0;
                    Root
                }
                8 => {
                    m.faults
                        .push((op("child", 0, b"arxd.service"), FsError::Refused));
                    Unit
                }
                9 => {
                    m.nodes[1].magic = 0;
                    Unit
                }
                10 => {
                    m.nodes[1].uid = 1;
                    Unit
                }
                11 => {
                    m.delegate = None;
                    Delegation
                }
                12 => {
                    m.delegate = Some(b"0".to_vec());
                    Delegation
                }
                13 => {
                    m.delegate = Some(b"11".to_vec());
                    Delegation
                }
                14 => {
                    m.nodes[1]
                        .overrides
                        .insert("cgroup.type".into(), b"threaded\n".to_vec());
                    UnitState
                }
                15 => {
                    m.nodes[1].procs = vec![17];
                    UnitState
                }
                16 => {
                    m.nodes[0].enabled.remove("pids");
                    UnitState
                }
                17 => {
                    m.nodes[2].procs.clear();
                    Supervisor
                }
                18 => {
                    m.nodes[2].procs = vec![17];
                    Supervisor
                }
                19 => {
                    m.nodes[2].procs = vec![42, 17];
                    Supervisor
                }
                _ => {
                    m.nodes[2].mode |= 0o002;
                    Supervisor
                }
            };
        }
        f.fail(expected);
        f.no_mutation();
    }
}
#[test]
fn h10b_hardening_first() {
    let f = Fake::new();
    {
        let mut m = f.0.borrow_mut();
        m.status.bytes.clear();
        m.identity = (1, 1);
    }
    f.fail(SetupFailure::Hardening);
    f.no_mutation();
    assert!(!f.0.borrow().log.iter().any(|o| o.kind == "identity"));
}
#[test]
fn h11_enable_unit_reread() {
    for rejected in [true, false] {
        let f = Fake::new();
        {
            let mut m = f.0.borrow_mut();
            if rejected {
                let mut o = op("write", 1, b"cgroup.subtree_control");
                o.bytes = b"+memory +pids".to_vec();
                m.faults.push((o, FsError::Invalid));
            } else {
                m.nodes[1]
                    .overrides
                    .insert("cgroup.subtree_control".into(), b"memory".to_vec());
            }
        }
        f.fail(SetupFailure::EnableUnit);
        assert!(
            f.0.borrow()
                .log
                .iter()
                .filter(|o| o.kind == "write")
                .all(|o| !o.bytes.contains(&b'-'))
        );
    }
}
#[test]
fn h12_runs() {
    for case in 0..4 {
        let f = Fake::new();
        let r = f.prepared();
        {
            let mut m = f.0.borrow_mut();
            match case {
                0 => {
                    m.nodes[r]
                        .overrides
                        .insert("cgroup.type".into(), b"threaded".to_vec());
                }
                1 => m.nodes[r].uid = 1,
                2 => m.nodes[r].mode |= 0o020,
                _ => m.nodes[r].procs = vec![17],
            }
        }
        f.fail(SetupFailure::Runs);
        f.no_removal();
    }
}
#[test]
fn h13_swap() {
    let f = Fake::new();
    let r = f.prepared();
    f.0.borrow_mut().nodes[r]
        .missing
        .insert("memory.swap.max".into());
    f.fail(SetupFailure::Postcondition);
}
#[test]
fn h14_shutdown_boundaries() {
    let f = Fake::new();
    let r = f.prepared();
    f.leaf(r, b"arx-1-0");
    f.0.borrow_mut().kill_delay = None;
    let token = Cancellation::default();
    let clock = Manual {
        cancel: Some(token.clone()),
        ..Manual::default()
    };
    assert_eq!(
        f.run(&token, &clock),
        SetupOutcome::Unavailable(SetupFailure::Stopped)
    );
    f.no_removal();
    for last in [false, true] {
        let f = Fake::new();
        let token = Cancellation::default();
        let c = token.clone();
        let mut lists = 0;
        f.0.borrow_mut().hook = Some(Box::new(move |_, o| {
            if last {
                if o.kind == "list" {
                    lists += 1;
                    if lists == 3 {
                        c.cancel();
                    }
                }
            } else if o.kind == "read" && o.dir == 1 && o.name == b"cgroup.subtree_control" {
                c.cancel();
            }
        }));
        assert_eq!(
            f.run(&token, &Manual::default()),
            SetupOutcome::Unavailable(SetupFailure::Stopped)
        );
        if !last {
            assert!(!f.0.borrow().log.iter().any(|o| o.kind == "mkdir"));
        }
    }
}
#[test]
fn h15_panic_payload_subprocess() {
    const FLAG: &str = "ARX_E2_PANIC_HELPER";
    if std::env::var_os(FLAG).is_some() {
        let f = Fake::new();
        let r = f.prepared();
        f.leaf(r, b"arx-1-0");
        {
            let mut m = f.0.borrow_mut();
            m.panic_remove = true;
            m.nodes[r]
                .overrides
                .insert("private.marker".into(), b"ARX-STATE-SECRET".to_vec());
        }
        f.fail(SetupFailure::Panicked);
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "exec::hierarchy::fake::h15_panic_payload_subprocess",
            "--nocapture",
        ])
        .env(FLAG, "1")
        .output()
        .unwrap();
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stderr.matches("ARX-H15-FIXED").count() + stdout.matches("ARX-H15-FIXED").count(),
        1
    );
    assert!(!stderr.contains("ARX-STATE-SECRET") && !stdout.contains("ARX-STATE-SECRET"));
}
#[test]
fn h16_own_pid_reuse() {
    let f = Fake::new();
    let r = f.prepared();
    f.leaf(r, b"arx-42-0");
    ready(f.outcome(), 1, false);
}
#[test]
fn h17_frozen_recursive_kill() {
    let f = Fake::new();
    let r = f.prepared();
    let l = f.leaf(r, b"arx-1-0");
    f.0.borrow_mut().nodes[l].frozen = true;
    ready(f.outcome(), 1, false);
    assert!(f.0.borrow().nodes[l].procs.is_empty());
}
#[test]
fn h18_verify_fresh() {
    let f = Fake::new();
    let r = f.prepared();
    f.leaf(r, b"arx-1-0");
    let mut removed = false;
    f.0.borrow_mut().hook = Some(Box::new(move |m, o| {
        if o.kind == "rmdir" {
            removed = true;
        }
        if removed && o.kind == "list" && o.dir == r {
            m.child(r, b"arx-2-0");
            removed = false;
        }
    }));
    f.fail(SetupFailure::VerifyFailed);
    assert_eq!(SetupFailure::VerifyFailed.cause(), SetupCause::Janitor);
}
#[test]
fn h19_verify_populated() {
    let f = Fake::new();
    let r = f.prepared();
    let mut lists = 0;
    f.0.borrow_mut().hook = Some(Box::new(move |m, o| {
        if o.kind == "list" && o.dir == r {
            lists += 1;
            if lists == 2 {
                m.nodes[r].procs.push(17);
            }
        }
    }));
    f.fail(SetupFailure::VerifyFailed);
}
#[test]
fn h20_postconditions_fresh() {
    for child in [true, false] {
        let f = Fake::new();
        let r = f.prepared();
        let mut lists = 0;
        f.0.borrow_mut().hook = Some(Box::new(move |m, o| {
            if o.kind == "list" && o.dir == r {
                lists += 1;
                if lists == 3 {
                    if child {
                        m.child(r, b"arx-2-0");
                    } else {
                        extra(&mut m.nodes[r], 10);
                    }
                }
            }
        }));
        f.fail(SetupFailure::Postcondition);
    }
}
#[test]
fn model_kernel_contracts() {
    let f = Fake::new();
    let r = f.prepared();
    assert!(f.read(&Dir(r), "memory.max", 4096).is_err());
    assert_eq!(
        f.write(&Dir(r), "cgroup.subtree_control", b"+memory +pids"),
        Err(FsError::Invalid)
    );
    assert!(f.0.borrow().nodes[r].enabled.is_empty());
    f.write(&Dir(1), "cgroup.subtree_control", b"+memory +pids")
        .unwrap();
    assert!(f.read(&Dir(r), "memory.max", 4096).is_ok());
    let l = f.leaf(r, b"arx-1-0");
    assert_eq!(
        f.write(&Dir(l), "cgroup.subtree_control", b"+memory"),
        Err(FsError::Busy)
    );
    assert_eq!(f.rmdir(&Dir(r), b"arx-1-0"), Err(FsError::Busy));
    f.0.borrow_mut().clear(l);
    f.0.borrow_mut().child(l, b"nested");
    assert_eq!(f.rmdir(&Dir(r), b"arx-1-0"), Err(FsError::Busy));
}

#[test]
fn step_failures_are_fixed() {
    use SetupFailure::*;
    for (kind, dir, name, bytes, failure) in [
        ("status", 0, b"".as_slice(), b"".as_slice(), Hardening),
        ("cgroup", 0, b"", b"", Locate),
        ("root", 0, b"", b"", Root),
        ("xattr", 1, b"", b"", Delegation),
        ("mkdir", 1, b"runs", b"", Runs),
        ("write", 3, b"cgroup.kill", b"1", KillInterface),
        (
            "write",
            3,
            b"cgroup.subtree_control",
            b"+memory +pids",
            EnableRuns,
        ),
        ("ro", 3, b"memory.max", b"", Postcondition),
    ] {
        let f = Fake::new();
        let mut operation = op(kind, dir, name);
        operation.bytes = bytes.to_vec();
        f.0.borrow_mut().faults.push((operation, FsError::Io));
        f.fail(failure);
    }
    for file in ["cgroup.events", "cgroup.procs", "cgroup.subtree_control"] {
        let f = Fake::new();
        let r = f.prepared();
        f.0.borrow_mut().nodes[r]
            .overrides
            .insert(file.into(), vec![b'x'; 4097]);
        f.fail(match file {
            "cgroup.procs" => Runs,
            "cgroup.events" => Postcondition,
            _ => EnableRuns,
        });
    }
    for bytes in [
        b"populated 0\npopulated 1\n".as_slice(),
        b"frozen 0\n",
        b"populated 0\nfrozen 0\nfrozen 0\n",
    ] {
        let f = Fake::new();
        let r = f.prepared();
        f.0.borrow_mut().nodes[r]
            .overrides
            .insert("cgroup.events".into(), bytes.to_vec());
        f.fail(Postcondition);
        f.no_removal();
    }
    let f = Fake::new();
    f.0.borrow_mut().status.bytes = vec![b'x'; 8193];
    f.fail(Hardening);
    f.no_mutation();
    let f = Fake::new();
    f.0.borrow_mut().status.magic = CGROUP_MAGIC;
    f.fail(Hardening);
    f.no_mutation();
}

// Pure model entry point for the inactive application wiring tests.
pub(crate) fn startup_setup(
    location: &parse::Location,
    token: &Cancellation,
    failure: bool,
    waiting: bool,
    barrier: std::sync::Arc<dyn Fn(&'static str) + Send + Sync>,
) -> SetupOutcome {
    let fs = Fake::new();
    if failure {
        fs.0.borrow_mut().status.bytes.clear();
    }
    let runs = fs.prepared();
    if waiting {
        fs.leaf(runs, b"arx-17-1");
        fs.0.borrow_mut().kill_delay = None;
    }
    let seam = barrier.clone();
    fs.0.borrow_mut().hook = Some(Box::new(move |model, operation| {
        if operation.kind == "list"
            && operation.dir == runs
            && model.nodes[runs].enabled.contains("memory")
        {
            seam("s9");
        }
    }));
    struct ClockSeam {
        now: Cell<Duration>,
        barrier: std::sync::Arc<dyn Fn(&'static str) + Send + Sync>,
    }
    impl Clock for ClockSeam {
        fn now(&self) -> Duration {
            self.now.get()
        }
        fn sleep(&self, duration: Duration) {
            (self.barrier)("wait");
            self.now.set(self.now.get() + duration);
        }
    }
    let clock = ClockSeam {
        now: Cell::new(Duration::ZERO),
        barrier,
    };
    setup(&fs, location, &clock, token)
}
