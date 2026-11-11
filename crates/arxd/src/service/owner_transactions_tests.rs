use super::*;
use std::{cell::RefCell, rc::Rc};
const SID: &str = "S-1-5-21-1-2-3-1002";
const OTHER: &str = "S-1-5-21-1-2-3-1003";
fn record(state: RightState) -> OwnerRecord {
    OwnerRecord {
        version: 1,
        sid: SID.into(),
        state,
    }
}
fn request(present: bool, recorded_grant: bool) -> Configure {
    Configure {
        bytes: b"new".to_vec(),
        expected: None,
        sid: SID.into(),
        add: !present,
        batch: BatchObservation {
            present,
            recorded_grant,
        },
    }
}
#[derive(Clone, Default)]
struct World {
    bytes: Option<Vec<u8>>,
    present: bool,
    calls: Vec<&'static str>,
    // Independent ghost history: acknowledges effects, not engine transitions.
    last_written: u8,
    external_unowned: bool,
    unsafe_revoke: bool,
    record_history: u8,
    lsa_history: u8,
    restored_grant: bool,
    bad_characterization: bool,
    limit: Option<usize>,
    mode: u8,
    fail: Option<&'static str>,
    effect: bool,
    published: bool,
    config: Option<Vec<u8>>,
    staged: Option<Vec<u8>>,
    mutation: u8,
}
impl World {
    fn state(&self) -> Option<RightState> {
        self.bytes
            .as_ref()
            .map(|b| OwnerRecord::decode(b).unwrap().state)
    }
    fn set(&mut self, state: RightState) {
        self.bytes = Some(record(state).encode().unwrap());
        self.last_written = code(state);
        self.record_history |= 1 << code(state);
    }
    fn call(&mut self, name: &'static str) -> Result<(), Failure> {
        if self.limit.is_some_and(|n| self.calls.len() >= n) {
            return Err(Failure::Io);
        }
        self.calls.push(name);
        if self.fail == Some(name) && (!self.effect || matches!(name, "record" | "observe")) {
            return Err(Failure::Io);
        }
        Ok(())
    }
    fn end(&self, name: &str) -> Result<(), Failure> {
        if self.fail == Some(name) {
            Err(Failure::Io)
        } else {
            Ok(())
        }
    }
}
fn code(s: RightState) -> u8 {
    match s {
        RightState::None => 1,
        RightState::Intent => 2,
        RightState::Granted => 3,
        RightState::Unproven => 4,
    }
}
fn state(c: u8) -> RightState {
    match c {
        1 => RightState::None,
        2 => RightState::Intent,
        3 => RightState::Granted,
        4 => RightState::Unproven,
        _ => unreachable!(),
    }
}
#[derive(Clone)]
struct Adapter(Rc<RefCell<World>>);
fn adapters(w: World) -> (Adapter, Adapter) {
    let a = Adapter(Rc::new(RefCell::new(w)));
    (a.clone(), a)
}
impl OwnerStore for Adapter {
    fn record(&mut self) -> Result<Option<(OwnerRecord, [u8; 32])>, Failure> {
        let mut w = self.0.borrow_mut();
        w.call("record")?;
        w.bytes
            .as_ref()
            .map(|b| Ok((OwnerRecord::decode(b)?, hash(b))))
            .transpose()
    }
    fn replace_record(
        &mut self,
        expected: Option<[u8; 32]>,
        next: &OwnerRecord,
    ) -> Result<[u8; 32], Failure> {
        let mut w = self.0.borrow_mut();
        let name = match next.state {
            RightState::Intent => "intent",
            RightState::Granted => "receipt",
            _ => "settle",
        };
        w.call(name)?;
        if w.bytes.as_deref().map(hash) != expected {
            return Err(Failure::Conflict);
        }
        let mut next = next.clone();
        // Seed faults at the receipt boundary, without changing the independent
        // ghost's attribution rule. These emulate the two forbidden decisions.
        if (w.mutation == 1
            && next.state == RightState::None
            && w.present
            && w.calls.contains(&"stage"))
            || (w.mutation == 2
                && next.state == RightState::Unproven
                && !w.calls.contains(&"stage"))
        {
            next.state = RightState::Granted;
        }
        let bytes = next.encode()?;
        let h = hash(&bytes);
        w.bytes = Some(bytes);
        // All completed writes remain visible. Even Err-with-effect is visible,
        // but only an acknowledged write updates last_written.
        w.record_history |= 1 << code(next.state);
        if w.fail != Some(name) {
            w.last_written = code(next.state);
            w.restored_grant = false;
        }
        w.end(name)?;
        Ok(h)
    }
    fn stage_config(&mut self, bytes: &[u8], expected: Option<[u8; 32]>) -> Result<(), Failure> {
        let mut w = self.0.borrow_mut();
        w.call("stage")?;
        if w.config.as_deref().map(hash) != expected {
            return Err(Failure::Conflict);
        }
        w.staged = Some(bytes.to_vec());
        w.end("stage")
    }
    fn publish_config(&mut self) -> Result<(), Failure> {
        let mut w = self.0.borrow_mut();
        w.call("publish")?;
        w.published = true;
        w.config = w.staged.clone();
        w.end("publish")
    }
}
impl BatchRight for Adapter {
    fn observe(&mut self, sid: &str) -> Result<bool, Failure> {
        assert!(sid == SID || sid == OTHER);
        let mut w = self.0.borrow_mut();
        w.call("observe")?;
        Ok(w.present)
    }
    fn grant(&mut self, sid: &str) -> Result<(), Failure> {
        assert!(sid == SID || sid == OTHER);
        let mut w = self.0.borrow_mut();
        w.call("grant")?;
        if w.mode != 1 {
            w.present = true;
            w.external_unowned = false;
            w.lsa_history |= 2;
        }
        if w.mode != 0 {
            Err(Failure::Io)
        } else {
            w.end("grant")
        }
    }
    fn revoke(&mut self, sid: &str) -> Result<(), Failure> {
        assert!(sid == SID || sid == OTHER);
        let mut w = self.0.borrow_mut();
        w.call("revoke")?;
        if w.mode != 1 {
            if w.present && w.external_unowned {
                w.unsafe_revoke = true;
                if !w.restored_grant {
                    w.bad_characterization = true;
                }
            }
            w.present = false;
            w.external_unowned = false;
            w.lsa_history |= 1;
        }
        if w.mode != 0 {
            Err(Failure::Io)
        } else {
            w.end("revoke")
        }
    }
}
#[test]
fn p1_to_p5_order_presence_staleness_and_unknown_grant_outcome() {
    let (mut s, mut r) = adapters(World::default());
    configure(&request(false, false), &mut s, &mut r).unwrap();
    assert_eq!(
        s.0.borrow().calls,
        [
            "record", "record", "observe", "stage", "intent", "grant", "receipt", "publish"
        ]
    );
    assert!(s.0.borrow().state() == Some(RightState::Granted));
    assert!(s.0.borrow().published);
    for old in [None, Some(RightState::None)] {
        let mut w = World {
            present: true,
            ..Default::default()
        };
        if let Some(old) = old {
            w.set(old);
        }
        let (mut s, mut r) = adapters(w);
        configure(&request(true, false), &mut s, &mut r).unwrap();
        assert!(s.0.borrow().state() == Some(RightState::None));
        assert_eq!(
            uninstall_right(&mut s, &mut r),
            Ok(RightReport::RetainedNotArxcovo)
        );
        assert!(!s.0.borrow().calls.contains(&"revoke"));
    }
    for present in [false, true] {
        let (mut s, mut r) = adapters(World {
            present,
            ..Default::default()
        });
        assert_eq!(
            configure(&request(!present, false), &mut s, &mut r),
            Err(Failure::Conflict)
        );
        assert!(!s.0.borrow().calls.contains(&"stage"));
    }
    for mode in [1, 2] {
        let (mut s, mut r) = adapters(World {
            mode,
            ..Default::default()
        });
        assert_eq!(
            configure(&request(false, false), &mut s, &mut r),
            Err(if mode == 1 {
                Failure::Io
            } else {
                Failure::RightUnproven
            })
        );
        assert!(!s.0.borrow().published);
        assert!(!s.0.borrow().calls.contains(&"revoke"));
        if mode == 2 {
            let req = Configure {
                expected: s.0.borrow().config.clone(),
                ..request(true, false)
            };
            configure(&req, &mut s, &mut r).unwrap();
            assert_eq!(
                uninstall_right(&mut s, &mut r),
                Ok(RightReport::RetainedUnproven)
            );
        }
    }
}
#[test]
fn p7_p8_receipt_and_publication_errors_with_and_without_effect() {
    for fail in ["receipt", "publish"] {
        for effect in [false, true] {
            let (mut s, mut r) = adapters(World {
                fail: Some(fail),
                effect,
                ..Default::default()
            });
            assert_eq!(
                configure(&request(false, false), &mut s, &mut r),
                Err(if fail == "receipt" {
                    Failure::RecoveryRequired
                } else {
                    Failure::Io
                })
            );
            s.0.borrow_mut().fail = None;
            assert_eq!(
                recover(&mut s, &mut r),
                Ok(if fail == "receipt" && !effect {
                    Recovery::Unproven
                } else {
                    Recovery::Clean
                })
            );
            assert!(s.0.borrow().present);
            assert!(!s.0.borrow().calls.contains(&"revoke"));
        }
    }
}
#[test]
fn p9_to_p13_uninstall_conflicts_recovery_and_cas() {
    for state in [
        RightState::None,
        RightState::Intent,
        RightState::Granted,
        RightState::Unproven,
    ] {
        for present in [false, true] {
            let mut w = World {
                present,
                ..Default::default()
            };
            w.set(state);
            let (mut s, mut r) = adapters(w.clone());
            let mut req = request(present, state == RightState::Granted && present);
            req.sid = OTHER.into();
            if state != RightState::None {
                assert_eq!(
                    configure(&req, &mut s, &mut r),
                    Err(if state == RightState::Intent {
                        Failure::RecoveryRequired
                    } else {
                        Failure::Conflict
                    })
                );
                assert_eq!(s.0.borrow().calls, ["record"]);
            } else {
                configure(&req, &mut s, &mut r).unwrap();
                assert_eq!(
                    OwnerRecord::decode(s.0.borrow().bytes.as_ref().unwrap())
                        .unwrap()
                        .sid,
                    OTHER
                );
            }
            let (mut s, mut r) = adapters(w);
            recover(&mut s, &mut r).unwrap();
            assert_eq!(
                uninstall_right(&mut s, &mut r),
                Ok(match (state, present) {
                    (RightState::Granted, true) => RightReport::Removed,
                    (RightState::Granted | RightState::Unproven, false) =>
                        RightReport::AlreadyAbsent,
                    (RightState::Unproven | RightState::Intent, true) =>
                        RightReport::RetainedUnproven,
                    (_, true) => RightReport::RetainedNotArxcovo,
                    _ => RightReport::NotRecorded,
                })
            );
            assert_eq!(
                s.0.borrow()
                    .calls
                    .iter()
                    .filter(|c| **c == "revoke")
                    .count(),
                usize::from(state == RightState::Granted && present)
            );
        }
    }
    for mode in [1, 2] {
        let mut w = World {
            present: true,
            mode,
            ..Default::default()
        };
        w.set(RightState::Granted);
        let (mut s, mut r) = adapters(w);
        assert_eq!(uninstall_right(&mut s, &mut r), Err(Failure::Io));
        assert!(s.0.borrow().state() == Some(RightState::Granted));
        s.0.borrow_mut().mode = 0;
        assert_eq!(
            uninstall_right(&mut s, &mut r),
            Ok(if mode == 1 {
                RightReport::Removed
            } else {
                RightReport::AlreadyAbsent
            })
        );
    }
    let (mut s, mut r) = adapters(World::default());
    assert_eq!(
        uninstall_right(&mut s, &mut r),
        Ok(RightReport::NotRecorded)
    );
    assert_eq!(
        s.replace_record(Some([7; 32]), &record(RightState::Granted)),
        Err(Failure::Conflict)
    );
    assert!(s.0.borrow().bytes.is_none());
}
#[test]
fn p14_strict_record_and_fixed_safe_errors() {
    let good = record(RightState::Granted).encode().unwrap();
    assert!(OwnerRecord::decode(&good).is_ok());
    for bytes in [
        vec![b' '; 513],
        [good.clone(), b" true".to_vec()].concat(),
        b"{}".to_vec(),
        b"null".to_vec(),
    ] {
        assert!(OwnerRecord::decode(&bytes).is_err());
    }
    let mut padded = good.clone();
    padded.resize(512, b' ');
    assert!(OwnerRecord::decode(&padded).is_ok());
    padded.push(b' ');
    assert!(OwnerRecord::decode(&padded).is_err());
    let original: serde_json::Value = serde_json::from_slice(&good).unwrap();
    for (key, value) in [
        ("version", serde_json::json!(2)),
        ("state", serde_json::json!("Granted")),
        ("unknown", serde_json::json!(true)),
    ] {
        let mut v = original.clone();
        v[key] = value;
        assert!(OwnerRecord::decode(&serde_json::to_vec(&v).unwrap()).is_err());
    }
    for sid in [
        "S-1-5-18",
        "S-1-5-19",
        "S-1-5-20",
        "S-1-5-32-544",
        "S-1-5-80-1-2-3",
        "S-1-5-21-1-2-3-01",
        "S-1-5-21-1-2-3-4294967296",
        "S-1-5-21-1-2-3-",
        "s-1-5-21-1-2-3-4",
    ] {
        let mut v = original.clone();
        v["sid"] = sid.into();
        assert!(OwnerRecord::decode(&serde_json::to_vec(&v).unwrap()).is_err());
    }
    let duplicate = String::from_utf8(good)
        .unwrap()
        .replace("\"version\":1", "\"version\":1,\"version\":1");
    assert!(OwnerRecord::decode(duplicate.as_bytes()).is_err());
    for (e, suffix) in [
        (Failure::InvalidPlan, "plan"),
        (Failure::Io, "io"),
        (Failure::Conflict, "conflict"),
        (Failure::Busy, "busy"),
        (Failure::RecoveryRequired, "recovery-required"),
        (Failure::RightUnproven, "right-unproven"),
        (Failure::Unsupported, "unsupported"),
    ] {
        assert_eq!(e.to_string(), format!("service-owner-{suffix}"));
        assert!(!e.to_string().contains(SID));
    }
}
#[test]
fn p16_regrant_and_no_revoke_in_configure_or_recover() {
    let mut w = World::default();
    w.set(RightState::Granted);
    let (mut s, mut r) = adapters(w);
    configure(&request(false, false), &mut s, &mut r).unwrap();
    assert!(s.0.borrow().calls.contains(&"intent"));
    assert!(s.0.borrow().calls.contains(&"grant"));
    let source = include_str!("owner_transactions.rs");
    let recovery = source
        .split("pub fn recover(")
        .nth(1)
        .unwrap()
        .split("pub enum RightReport")
        .next()
        .unwrap();
    assert!(!recovery.contains(".revoke("));
}

// Explore actual engine call prefixes, merging only identical future-relevant
// states. Ghost ownership is updated at effect boundaries, independently of the
// engine's decisions. No NTFS/registry behavior is inferred from these faults.
fn key(w: &World) -> (Option<Vec<u8>>, bool, u8, bool, u8, u8, bool, bool) {
    (
        w.bytes.clone(),
        w.present,
        w.last_written,
        w.external_unowned,
        w.record_history,
        w.lsa_history,
        w.restored_grant,
        w.config.is_some(),
    )
}
fn command(w: &World, uninstall: bool, limit: Option<usize>, mode: u8, mutation: u8) -> World {
    let mut w = w.clone();
    w.calls.clear();
    w.limit = limit;
    w.mode = mode;
    w.mutation = mutation;
    let (mut s, mut r) = adapters(w);
    let go = recover(&mut s, &mut r).is_ok();
    if go {
        if uninstall {
            let _ = uninstall_right(&mut s, &mut r);
        } else if let Ok(batch) = observe_batch(SID, &mut s, &mut r) {
            let req = Configure {
                expected: s.0.borrow().config.clone(),
                batch,
                ..request(batch.present, batch.recorded_grant)
            };
            let _ = configure(&req, &mut s, &mut r);
        }
    }
    s.0.borrow().clone()
}
fn explore(fault: u8, mutation: u8) -> (usize, usize) {
    use std::collections::BTreeMap;
    let initial = World {
        record_history: 1,
        lsa_history: 1,
        ..Default::default()
    };
    let mut frontier = BTreeMap::from([(key(&initial), initial)]);
    let mut seen = std::collections::BTreeSet::new();
    let mut unsafe_count = 0;
    let mut edges = 0;
    for _depth in 0..6 {
        let mut next = BTreeMap::new();
        for w in frontier.into_values() {
            let mut successors = Vec::new();
            for uninstall in [false, true] {
                for mode in 0..3 {
                    for limit in (0..16).map(Some).chain([None]) {
                        successors.push(command(&w, uninstall, limit, mode, mutation));
                    }
                }
            }
            for present in [false, true] {
                let mut v = w.clone();
                v.present = present;
                v.lsa_history |= 1 << u8::from(present);
                v.external_unowned = present && v.last_written != 3;
                successors.push(v);
            }
            if fault >= 1 {
                for present in [false, true] {
                    if w.lsa_history & (1 << u8::from(present)) != 0 {
                        let mut v = w.clone();
                        v.present = present;
                        successors.push(v);
                    }
                }
            }
            if fault == 2 {
                for c in 0..5 {
                    if w.record_history & (1 << c) != 0 {
                        let mut v = w.clone();
                        v.bytes = if c == 0 {
                            None
                        } else {
                            Some(record(state(c)).encode().unwrap())
                        };
                        v.restored_grant = c == 3 && v.last_written != 3;
                        successors.push(v);
                    }
                }
            }
            for mut v in successors {
                edges += 1;
                if v.unsafe_revoke {
                    unsafe_count += 1;
                    assert!(
                        fault == 2 || mutation != 0,
                        "unsafe revoke within guarantee"
                    );
                    if mutation == 0 {
                        assert!(
                            !v.bad_characterization,
                            "unsafe revoke without restored Granted"
                        );
                    }
                    v.unsafe_revoke = false;
                }
                v.calls.clear();
                v.limit = None;
                v.mode = 0;
                let k = key(&v);
                if seen.insert(k.clone()) {
                    next.insert(k, v);
                }
            }
        }
        frontier = next;
    }
    (edges, unsafe_count)
}
#[test]
fn p6_p6b_p6c_depth6_and_seeded_mutations() {
    for fault in 0..3 {
        let (edges, unsafe_count) = explore(fault, 0);
        eprintln!("fault={fault} depth=6 edges={edges} unsafe={unsafe_count}");
        if fault < 2 {
            assert_eq!(unsafe_count, 0);
        } else {
            assert!(unsafe_count > 0);
        }
    }
    for mutation in [1, 2] {
        let (edges, witnesses) = explore(0, mutation);
        eprintln!("mutation={mutation} edges={edges} witnesses={witnesses}");
        assert!(witnesses > 0, "mutation escaped P6");
    }
    // Decision 6's exact counterexample, independently of state-space merging.
    let (mut s, mut r) = adapters(World::default());
    configure(&request(false, false), &mut s, &mut r).unwrap();
    let stale = s.0.borrow().bytes.clone();
    assert_eq!(uninstall_right(&mut s, &mut r), Ok(RightReport::Removed));
    {
        let mut w = s.0.borrow_mut();
        w.present = true;
        w.external_unowned = true;
        w.bytes = stale;
        w.restored_grant = true;
    }
    assert_eq!(uninstall_right(&mut s, &mut r), Ok(RightReport::Removed));
    assert!(s.0.borrow().unsafe_revoke);
}

struct ResolverFixture;
impl Resolver for ResolverFixture {
    fn account(
        &self,
        n: &str,
    ) -> Result<super::super::config::Account, super::super::layout::Error> {
        use super::super::config::{Account, AccountKind};
        Ok(Account {
            identity: if n == "builder" { SID } else { n }.into(),
            gid: None,
            kind: AccountKind::User,
            administrative: false,
        })
    }
    fn workspace_safe(&self, _: &str) -> Result<bool, super::super::layout::Error> {
        Ok(true)
    }
}
fn windows_config(mirror: bool) -> (Layout, ServiceConfig) {
    let l = Layout::for_test(Platform::Windows);
    let mut v = super::super::config::tests::fixture();
    v["client"]["identity"] = "S-1-5-21-1-2-3-1001".into();
    v["build_account"]["identity"] = SID.into();
    v["build_account"]["gid"] = serde_json::Value::Null;
    v["workspace"] = r"D:\work".into();
    v["granted_batch_right"] = mirror.into();
    let c = ServiceConfig::parse(&serde_json::to_vec(&v).unwrap(), &l, &ResolverFixture).unwrap();
    (l, c)
}
#[test]
fn p15_p17_plans_mirrors_extraction_and_platform_fences() {
    use super::super::owner_plan::{
        Action, MissingNativeAdapter, OwnerContext, configure_plan, owner_plan, uninstall_plan,
    };
    for old_mirror in [false, true] {
        for state in [
            None,
            Some(RightState::None),
            Some(RightState::Granted),
            Some(RightState::Unproven),
        ] {
            for present in [false, true] {
                let (l, c) = windows_config(old_mirror);
                let ctx = OwnerContext {
                    admin: true,
                    running_binary: l.binary(),
                };
                let mut w = World {
                    present,
                    ..Default::default()
                };
                if let Some(state) = state {
                    w.set(state);
                }
                let (mut s, mut r) = adapters(w);
                let batch = observe_batch(SID, &mut s, &mut r).unwrap();
                assert!(configure_plan(&l, &ctx, &c, None, None, &ResolverFixture).is_err());
                let plan =
                    configure_plan(&l, &ctx, &c, Some(&c), Some(batch), &ResolverFixture).unwrap();
                assert!(plan.execute(&mut MissingNativeAdapter).is_err());
                let req = Configure::from_plan(&l, &plan, &ResolverFixture, batch).unwrap();
                assert_eq!(req.sid(), SID);
                assert_eq!(req.adds_right(), !present);
                assert_eq!(
                    req.expected(),
                    Some(c.serialized(old_mirror).unwrap().as_slice())
                );
                let parsed = ServiceConfig::parse(req.bytes(), &l, &ResolverFixture).unwrap();
                assert_eq!(
                    parsed.granted_batch_right(),
                    !present || state == Some(RightState::Granted)
                );
                // Opposite mirror authority must refuse, even when the config's old
                // mirror claims a grant. Test-only mutation of the extracted bytes.
                let mut bad_plan = plan;
                for op in bad_plan.test_operations_mut() {
                    if let Operation::AtomicConfig { bytes, .. } = op {
                        *bytes = parsed.serialized(!parsed.granted_batch_right()).unwrap();
                    }
                }
                assert!(matches!(
                    Configure::from_plan(&l, &bad_plan, &ResolverFixture, batch),
                    Err(Failure::InvalidPlan)
                ));
                s.0.borrow_mut().present = !present;
                assert_eq!(configure(&req, &mut s, &mut r), Err(Failure::Conflict));
                let uninstall = uninstall_plan(&l, &ctx, &c, &ResolverFixture).unwrap();
                assert!(
                    !uninstall
                        .operations()
                        .iter()
                        .any(|o| matches!(o, Operation::BatchRight { .. }))
                );
                assert!(uninstall.execute(&mut MissingNativeAdapter).is_err());
                for action in [
                    Action::Install,
                    Action::Start,
                    Action::Stop,
                    Action::Status,
                    Action::Uninstall,
                ] {
                    assert!(
                        Configure::from_plan(
                            &l,
                            &owner_plan(&l, &ctx, action).unwrap(),
                            &ResolverFixture,
                            batch
                        )
                        .is_err()
                    );
                }
            }
        }
    }
    // No config/account/workspace exists on this API path.
    let l = Layout::for_test(Platform::Windows);
    let ctx = OwnerContext {
        admin: true,
        running_binary: l.binary(),
    };
    assert!(owner_plan(&l, &ctx, Action::Uninstall).is_ok());
}
#[test]
fn store_and_observation_failures_do_not_leak_owner_data_or_compensate() {
    for fail in ["record", "observe", "stage", "intent", "settle"] {
        for effect in [false, true] {
            let present = fail == "settle";
            let (mut s, mut r) = adapters(World {
                present,
                fail: Some(fail),
                effect,
                ..Default::default()
            });
            assert!(configure(&request(present, false), &mut s, &mut r).is_err());
            assert!(!s.0.borrow().calls.contains(&"grant"));
            assert!(!s.0.borrow().calls.contains(&"revoke"));
        }
    }
    for present in [false, true] {
        let mut w = World {
            present,
            ..Default::default()
        };
        w.set(RightState::Intent);
        let (mut s, mut r) = adapters(w);
        assert_eq!(
            configure(&request(present, false), &mut s, &mut r),
            Err(Failure::RecoveryRequired)
        );
        assert_eq!(s.0.borrow().calls, ["record"]);
        assert_eq!(
            recover(&mut s, &mut r),
            Ok(if present {
                Recovery::Unproven
            } else {
                Recovery::NotGranted
            })
        );
    }
}

#[test]
fn record_only_staleness_config_hash_and_recovery_write_errors() {
    let mut w = World {
        present: true,
        ..Default::default()
    };
    w.set(RightState::Granted);
    let (mut s, mut r) = adapters(w);
    let batch = observe_batch(SID, &mut s, &mut r).unwrap();
    s.0.borrow_mut().set(RightState::None);
    assert_eq!(
        configure(
            &Configure {
                batch,
                ..request(true, true)
            },
            &mut s,
            &mut r
        ),
        Err(Failure::Conflict)
    );
    assert!(!s.0.borrow().calls.contains(&"stage"));
    let (mut s, mut r) = adapters(World {
        config: Some(b"concurrent".to_vec()),
        ..Default::default()
    });
    assert_eq!(
        configure(&request(false, false), &mut s, &mut r),
        Err(Failure::Conflict)
    );
    assert!(!s.0.borrow().calls.contains(&"grant"));
    for effect in [false, true] {
        let mut w = World {
            present: true,
            fail: Some("settle"),
            effect,
            ..Default::default()
        };
        w.set(RightState::Intent);
        let (mut s, mut r) = adapters(w);
        assert_eq!(recover(&mut s, &mut r), Err(Failure::Io));
        s.0.borrow_mut().fail = None;
        assert_eq!(
            recover(&mut s, &mut r),
            Ok(if effect {
                Recovery::Clean
            } else {
                Recovery::Unproven
            })
        );
        assert!(!s.0.borrow().calls.contains(&"revoke"));
    }
    for report in [
        RightReport::Removed,
        RightReport::AlreadyAbsent,
        RightReport::RetainedUnproven,
        RightReport::RetainedNotArxcovo,
        RightReport::NotRecorded,
    ] {
        assert!(report.to_string().starts_with("batch-right-"));
        assert!(!report.to_string().contains(SID));
    }
    let (mut s, mut r) = adapters(World::default());
    assert!(matches!(
        observe_batch("private-marker", &mut s, &mut r),
        Err(Failure::InvalidPlan)
    ));
    assert!(s.0.borrow().calls.is_empty());
}

#[test]
fn windows_uninstall_skips_dead_account_workspace_and_direct_execution() {
    use super::super::{
        layout::Error,
        owner_plan::{OsManager, OwnerContext, uninstall_plan},
    };
    struct Unavailable;
    impl Resolver for Unavailable {
        fn account(&self, _: &str) -> Result<super::super::config::Account, Error> {
            panic!("uninstall must not resolve account")
        }
        fn workspace_safe(&self, _: &str) -> Result<bool, Error> {
            panic!("uninstall must not open workspace")
        }
    }
    struct ForbiddenManager;
    impl OsManager for ForbiddenManager {
        fn apply(&mut self, _: &Operation) -> Result<(), Error> {
            panic!("Windows plan must use transaction engine")
        }
    }
    let (l, c) = windows_config(true);
    let ctx = OwnerContext {
        admin: true,
        running_binary: l.binary(),
    };
    let plan = uninstall_plan(&l, &ctx, &c, &Unavailable).unwrap();
    assert_eq!(
        plan.execute(&mut ForbiddenManager),
        Err(Error::NativeAdapterMissing)
    );
    let (mut s, mut r) = adapters(World::default());
    let batch = observe_batch(SID, &mut s, &mut r).unwrap();
    let plan =
        super::super::owner_plan::configure_plan(&l, &ctx, &c, None, Some(batch), &ResolverFixture)
            .unwrap();
    assert_eq!(
        plan.execute(&mut ForbiddenManager),
        Err(Error::NativeAdapterMissing)
    );
    let req = Configure::from_plan(&l, &plan, &ResolverFixture, batch).unwrap();
    configure(&req, &mut s, &mut r).unwrap();
    assert_eq!(s.0.borrow().config.as_deref(), Some(req.bytes()));
    // The record alone names the SID; no service config is passed to uninstall.
    s.0.borrow_mut().config = None;
    assert_eq!(uninstall_right(&mut s, &mut r), Ok(RightReport::Removed));
}

#[test]
fn unix_planning_needs_no_windows_observation_and_preserves_validation() {
    use super::super::{
        config::tests::{FakeResolver, fixture},
        owner_plan::{MissingNativeAdapter, OwnerContext, configure_plan, uninstall_plan},
    };
    for platform in [Platform::Linux, Platform::Macos] {
        let l = Layout::for_test(platform);
        let resolver = FakeResolver::default();
        let config =
            ServiceConfig::parse(&serde_json::to_vec(&fixture()).unwrap(), &l, &resolver).unwrap();
        let ctx = OwnerContext {
            admin: true,
            running_binary: l.binary(),
        };
        let plan = configure_plan(&l, &ctx, &config, None, None, &resolver).unwrap();
        assert!(
            !plan
                .operations()
                .iter()
                .any(|o| matches!(o, Operation::BatchRight { .. }))
        );
        assert!(
            plan.operations()
                .iter()
                .any(|o| matches!(o, Operation::AtomicConfig { .. }))
        );
        assert!(uninstall_plan(&l, &ctx, &config, &resolver).is_ok());
        assert!(
            uninstall_plan(
                &l,
                &ctx,
                &config,
                &FakeResolver {
                    changed: true,
                    ..Default::default()
                }
            )
            .is_err()
        );
        let (mut s, mut r) = adapters(World::default());
        let batch = observe_batch(SID, &mut s, &mut r).unwrap();
        assert!(configure_plan(&l, &ctx, &config, None, Some(batch), &resolver).is_err());
        assert!(matches!(
            Configure::from_plan(&l, &plan, &resolver, batch),
            Err(Failure::Unsupported)
        ));
        assert!(plan.execute(&mut MissingNativeAdapter).is_err());
    }
}
#[test]
fn windows_config_still_rejects_reserved_identities_and_groups() {
    use super::super::{
        config::{Account, AccountKind},
        layout::Error,
    };
    let (l, c) = windows_config(false);
    let bytes = c.serialized(false).unwrap();
    let original: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    for sid in ["S-1-5-18", "S-1-5-19", "S-1-5-20", "S-1-5-80-1-2-3"] {
        let mut v = original.clone();
        v["build_account"]["identity"] = sid.into();
        assert!(
            ServiceConfig::parse(&serde_json::to_vec(&v).unwrap(), &l, &ResolverFixture).is_err()
        );
    }
    struct Group;
    impl Resolver for Group {
        fn account(&self, n: &str) -> Result<Account, Error> {
            let mut a = ResolverFixture.account(n)?;
            if n == "builder" {
                a.kind = AccountKind::Group;
            }
            Ok(a)
        }
        fn workspace_safe(&self, _: &str) -> Result<bool, Error> {
            Ok(true)
        }
    }
    assert!(ServiceConfig::parse(&bytes, &l, &Group).is_err());
}

#[test]
fn uninstall_uses_explicit_record_sid_without_resolving_it() {
    struct ExactSid;
    impl BatchRight for ExactSid {
        fn observe(&mut self, sid: &str) -> Result<bool, Failure> {
            assert_eq!(sid, OTHER);
            Ok(true)
        }
        fn grant(&mut self, _: &str) -> Result<(), Failure> {
            panic!("uninstall cannot grant")
        }
        fn revoke(&mut self, sid: &str) -> Result<(), Failure> {
            assert_eq!(sid, OTHER);
            Ok(())
        }
    }
    let mut rec = record(RightState::Granted);
    rec.sid = OTHER.into();
    let (mut s, _) = adapters(World {
        bytes: Some(rec.encode().unwrap()),
        ..Default::default()
    });
    assert_eq!(
        uninstall_right(&mut s, &mut ExactSid),
        Ok(RightReport::Removed)
    );
    let bytes = s.0.borrow().bytes.clone().unwrap();
    let rec = OwnerRecord::decode(&bytes).unwrap();
    assert_eq!(rec.sid, OTHER);
    assert!(rec.state == RightState::None);
}
