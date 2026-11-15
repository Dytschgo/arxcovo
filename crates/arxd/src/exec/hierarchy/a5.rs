//! Test-only A5 observations, shared by the dormant fixture and pure model.
use super::*;

fn require(ok: bool) -> FsResult<()> {
    if ok { Ok(()) } else { Err(FsError::Refused) }
}
fn trusted<F: CgroupFs>(fs: &F, dir: &F::Dir) -> FsResult<()> {
    let s = fs.stat(dir)?;
    require(
        s.uid == 0
            && s.mode & 0o022 == 0
            && s.magic == CGROUP_MAGIC
            && s.mode & 0o170000 == 0o040000,
    )
}
fn text<F: CgroupFs>(fs: &F, dir: &F::Dir, file: &str) -> FsResult<String> {
    let bytes = fs.read(dir, file, 4096)?;
    require(bytes.len() <= 4096)?;
    String::from_utf8(bytes).map_err(|_| FsError::Invalid)
}
pub(crate) fn empty<F: CgroupFs>(fs: &F, dir: &F::Dir) -> FsResult<()> {
    trusted(fs, dir)?;
    require(parse::pids(&text(fs, dir, "cgroup.procs")?) == Some(vec![]))?;
    require(parse::events(&text(fs, dir, "cgroup.events")?) == Some(false))?;
    let entries = fs.list(dir, 512)?;
    require(
        entries.len() <= 512
            && entries.iter().all(|e| {
                matches!(
                    parse::classify_entry(&e.name, e.d_type, e.fstatat_mode),
                    parse::Entry::Dot | parse::Entry::Control
                )
            }),
    )
}
/// Read-only copy of S0-S4 and unit/supervisor checks, before either mkdir.
pub(crate) fn preflight<F: CgroupFs>(fs: &F, location: &parse::Location) -> FsResult<F::Dir> {
    let status = fs.proc_status()?;
    require(
        status.magic == PROC_MAGIC && parse::hardening(&status.bytes) == parse::Hardening::Standard,
    )?;
    require(fs.identity()? == (0, 0))?;
    let proc = fs.proc_cgroup()?;
    require(
        proc.magic == PROC_MAGIC
            && parse::locate(&proc.bytes)
                .is_some_and(|l| l.unit_components() == location.unit_components()),
    )?;
    let mut unit = fs.open_root()?;
    trusted(fs, &unit)?;
    for component in location.unit_components() {
        unit = fs.open_child(&unit, component.as_bytes())?;
        trusted(fs, &unit)?;
    }
    require(fs.xattr_delegate(&unit)?.as_deref() == Some(b"1"))?;
    require(matches!(
        text(fs, &unit, "cgroup.type")?.as_str(),
        "domain" | "domain\n"
    ))?;
    require(parse::pids(&text(fs, &unit, "cgroup.procs")?) == Some(vec![]))?;
    let controllers = parse::controllers(&text(fs, &unit, "cgroup.controllers")?);
    require(controllers.memory && controllers.pids)?;
    let supervisor = fs.open_child(&unit, b"supervisor")?;
    trusted(fs, &supervisor)?;
    require(parse::pids(&text(fs, &supervisor, "cgroup.procs")?) == Some(vec![fs.pid()?]))?;
    Ok(unit)
}

pub(crate) fn finish<F: CgroupFs>(fs: &F, clock: &dyn Clock, token: &Cancellation) -> bool {
    finish_observed(fs, clock, token).1
}

pub(crate) fn finish_observed<F: CgroupFs>(
    fs: &F,
    clock: &dyn Clock,
    token: &Cancellation,
) -> (Option<SetupOutcome>, bool) {
    let Ok(proc) = fs.proc_cgroup() else {
        return (None, false);
    };
    if proc.magic != PROC_MAGIC {
        return (None, false);
    }
    let Some(location) = parse::locate(&proc.bytes) else {
        return (None, false);
    };
    let outcome = setup(fs, &location, clock, token);
    let accepted = post_setup(fs, &location, token, outcome);
    (Some(outcome), accepted)
}

fn post_setup<F: CgroupFs>(
    fs: &F,
    location: &parse::Location,
    token: &Cancellation,
    outcome: SetupOutcome,
) -> bool {
    let SetupOutcome::Ready(evidence) = outcome else {
        return false;
    };
    if evidence.runs_created
        || !evidence.unit_written
        || !evidence.runs_written
        || evidence.leaves_removed != 1
    {
        return false;
    }
    let Ok(mut unit) = fs.open_root() else {
        return false;
    };
    for component in location.unit_components() {
        let Ok(next) = fs.open_child(&unit, component.as_bytes()) else {
            return false;
        };
        unit = next;
    }
    let Ok(runs) = fs.open_child(&unit, b"runs") else {
        return false;
    };
    let Ok(info) = fs.stat(&runs) else {
        return false;
    };
    if info.uid != 0 || info.mode & 0o777 != 0o700 || info.magic != CGROUP_MAGIC {
        return false;
    }
    for dir in [&unit, &runs] {
        let Ok(bytes) = fs.read(dir, "cgroup.subtree_control", 4096) else {
            return false;
        };
        let Ok(text) = std::str::from_utf8(&bytes) else {
            return false;
        };
        let controllers = parse::controllers(text);
        if !controllers.memory || !controllers.pids {
            return false;
        }
    }
    let Ok(procs) = fs.read(&runs, "cgroup.procs", 4096) else {
        return false;
    };
    if !std::str::from_utf8(&procs)
        .ok()
        .and_then(parse::pids)
        .is_some_and(|p| p.is_empty())
    {
        return false;
    }
    let Ok(events) = fs.read(&runs, "cgroup.events", 4096) else {
        return false;
    };
    if std::str::from_utf8(&events).ok().and_then(parse::events) != Some(false) {
        return false;
    }
    let Ok(entries) = fs.list(&runs, 512) else {
        return false;
    };
    if entries.len() > 512
        || entries.iter().any(|e| {
            !matches!(
                parse::classify_entry(&e.name, e.d_type, e.fstatat_mode),
                parse::Entry::Dot | parse::Entry::Control
            )
        })
    {
        return false;
    }
    for file in ["memory.max", "memory.swap.max", "pids.max"] {
        if fs.open_ro(&runs, file).is_err() {
            return false;
        }
    }
    matches!(fs.open_child(&runs, b"arx-0-0"), Err(FsError::Missing)) && !token.is_cancelled()
}

#[test]
fn empty_orphan_removed_before_acceptance() {
    let fs = Fake::new();
    let runs = fs.prepared();
    fs.0.borrow_mut().child(runs, b"arx-0-0");
    empty(&fs, &Dir(runs + 1)).unwrap();
    assert!(finish(&fs, &Manual::default(), &Cancellation::default()));
    let m = fs.0.borrow();
    assert!(m.nodes[runs].children.is_empty());
    assert_eq!(m.log.iter().filter(|o| o.kind == "rmdir").count(), 1);
}

#[test]
fn preflight_refuses_before_mutation() {
    for case in 0..14 {
        let fs = Fake::new();
        {
            let mut m = fs.0.borrow_mut();
            match case {
                0 => m.status.bytes.clear(),
                1 => m.status.magic = 0,
                2 => m.identity = (0, 1),
                3 => m.cgroup.magic = 0,
                4 => m.cgroup.bytes = b"0::/other.service/supervisor".to_vec(),
                5 => m.nodes[0].uid = 1,
                6 => m.nodes[1].mode |= 0o020,
                7 => m.nodes[1].magic = 0,
                8 => m.delegate = None,
                9 => {
                    m.nodes[1]
                        .overrides
                        .insert("cgroup.type".into(), b"threaded".to_vec());
                }
                10 => m.nodes[1].procs.push(17),
                11 => {
                    m.nodes[0].enabled.remove("pids");
                }
                12 => m.nodes[2].uid = 1,
                _ => m.nodes[2].procs.push(17),
            }
        }
        let location = parse::locate(b"0::/arxd.service/supervisor").unwrap();
        assert!(preflight(&fs, &location).is_err(), "preflight case {case}");
        fs.no_mutation();
        fs.no_removal();
    }
    let fs = Fake::new();
    let location = parse::locate(b"0::/arxd.service/supervisor").unwrap();
    assert!(preflight(&fs, &location).is_ok());
    fs.no_mutation();
}

#[test]
fn seed_observations_refuse_population_nested_and_foreign() {
    for case in 0..5 {
        let fs = Fake::new();
        let runs = fs.prepared();
        let mut m = fs.0.borrow_mut();
        match case {
            0 => m.nodes[runs].procs.push(17),
            1 => {
                m.child(runs, b"nested");
            }
            2 => m.nodes[runs].uid = 1,
            3 => {
                m.nodes[runs].overrides.insert(
                    "cgroup.events".into(),
                    b"populated 0\npopulated 1\n".to_vec(),
                );
            }
            _ => extra(&mut m.nodes[runs], 10),
        }
        drop(m);
        assert!(empty(&fs, &Dir(runs)).is_err(), "seed case {case}");
        fs.no_mutation();
        fs.no_removal();
    }
}

#[test]
fn populated_and_empty_ordering() {
    let fs = Fake::new();
    let runs = fs.prepared();
    let populated = fs.leaf(runs, b"arx-1-0");
    let empty = fs.0.borrow_mut().child(runs, b"arx-0-0");
    let clock = Manual::default();
    ready(fs.run(&Cancellation::default(), &clock), 2, false);
    assert_eq!(
        clock.now(),
        Duration::from_millis(10),
        "wait for kill completion"
    );
    let m = fs.0.borrow();
    let position = |kind, dir, name: &[u8]| {
        m.log
            .iter()
            .position(|o| o.kind == kind && o.dir == dir && o.name == name)
            .unwrap()
    };
    let kill = position("write", runs, b"cgroup.kill");
    let first_remove = m.log.iter().position(|o| o.kind == "rmdir").unwrap();
    assert!(kill < position("list", runs, b""));
    for leaf in [populated, empty] {
        assert!(position("read", leaf, b"cgroup.events") < first_remove);
        assert!(position("list", leaf, b"") < first_remove);
    }
    assert!(m.nodes[runs].children.is_empty());
    assert!(!m.populated(runs));
    assert_eq!(m.log.iter().filter(|o| o.kind == "rmdir").count(), 2);
    let last_remove = m.log.iter().rposition(|o| o.kind == "rmdir").unwrap();
    assert!(
        m.log
            .iter()
            .enumerate()
            .any(|(i, o)| i > last_remove && o.kind == "list" && o.dir == runs)
    );
    assert!(position("ro", runs, b"memory.swap.max") > last_remove);
}

#[test]
fn refusal_never_accepts_residue_or_wrong_evidence() {
    for case in 0..8 {
        let fs = Fake::new();
        if case != 0 {
            let runs = fs.prepared();
            let leaf = fs.0.borrow_mut().child(runs, b"arx-0-0");
            match case {
                1 => {
                    fs.0.borrow_mut().child(leaf, b"nested");
                }
                2 => {
                    fs.0.borrow_mut()
                        .faults
                        .push((op("rmdir", runs, b"arx-0-0"), FsError::Busy))
                }
                3 => {
                    fs.0.borrow_mut().nodes[runs]
                        .missing
                        .insert("memory.swap.max".into());
                }
                4 => {
                    fs.0.borrow_mut().nodes[runs].children.clear();
                }
                5 => {
                    fs.0.borrow_mut().child(runs, b"unknown");
                }
                6 => {
                    fs.0.borrow_mut().nodes[leaf].procs.push(17);
                    fs.0.borrow_mut().kill_delay = None;
                }
                _ => {
                    fs.0.borrow_mut().hook = Some(Box::new(move |m, o| {
                        if o.kind == "rmdir" {
                            m.child(runs, b"arx-2-0");
                        }
                    }));
                }
            }
        }
        assert!(
            !finish(&fs, &Manual::default(), &Cancellation::default()),
            "case {case}"
        );
    }
    let fs = Fake::new();
    let token = Cancellation::default();
    token.cancel();
    assert!(!finish(&fs, &Manual::default(), &token));
    fs.no_mutation();
}

/// Existing StartupDomain tests observe Pending at each janitor/S9 operation.
pub(crate) fn startup(
    location: &parse::Location,
    token: &Cancellation,
    failure: bool,
    observe: std::sync::Arc<dyn Fn(&'static str) + Send + Sync>,
) -> SetupOutcome {
    let fs = Fake::new();
    let runs = fs.prepared();
    fs.leaf(runs, b"arx-1-0");
    fs.0.borrow_mut().child(runs, b"arx-0-0");
    if failure {
        fs.0.borrow_mut().kill_delay = None;
    }
    fs.0.borrow_mut().hook = Some(Box::new(move |_, op| {
        observe(match (op.kind, op.name.as_slice()) {
            ("write", b"cgroup.kill") => "kill",
            ("read", b"cgroup.events") => "events",
            ("rmdir", _) => "remove",
            ("ro", _) => "s9",
            _ => "other",
        });
    }));
    setup(&fs, location, &Manual::default(), token)
}
