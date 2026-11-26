//! Identity-before-write custody through the real ledger (ADR 0072).
use super::*;
use crate::store::vm::{
    VmStore,
    native::{self, Leaf, Origin},
};
use crate::vm::BuildSlots;
use std::cell::{Cell, RefCell};

const PATH: NativeRefusal = NativeRefusal::PathCustodyUnverified;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::vm::windows::qualification) enum Fault {
    /// The checkpoint is refused before it commits.
    Before(&'static str),
    /// The checkpoint commits and its acknowledgement is lost.
    After(&'static str),
}

/// Real ledger checkpoints that log what the filesystem holds at each
/// commit and can inject one fault around a named checkpoint.
pub(in crate::vm::windows::qualification) struct Recorder<'a> {
    ledger: LedgerCheckpoints<'a>,
    directory: std::path::PathBuf,
    pub fault: Cell<Option<Fault>>,
    pub log: RefCell<Vec<(&'static str, Option<u64>)>>,
}

impl<'a> Recorder<'a> {
    pub(in crate::vm::windows::qualification) fn new(
        store: &'a VmStore,
        production: &LocalPath,
        generation: Id,
    ) -> Self {
        Self {
            ledger: LedgerCheckpoints {
                store,
                origin: Origin::Build,
            },
            directory: std::path::Path::new(production.as_str()).join(hex::encode(generation)),
            fault: Cell::new(None),
            log: RefCell::new(Vec::new()),
        }
    }

    fn step(
        &self,
        name: &'static str,
        object: Option<String>,
        commit: impl FnOnce() -> Result<(), NativeRefusal>,
    ) -> Result<(), NativeRefusal> {
        let observed = object.and_then(|leaf| {
            std::fs::metadata(self.directory.join(leaf))
                .ok()
                .map(|metadata| metadata.len())
        });
        self.log.borrow_mut().push((name, observed));
        if self.fault.get() == Some(Fault::Before(name)) {
            return Err(PATH);
        }
        commit()?;
        if self.fault.get() == Some(Fault::After(name)) {
            return Err(PATH);
        }
        Ok(())
    }

    pub(in crate::vm::windows::qualification) fn names(&self) -> Vec<&'static str> {
        self.log.borrow().iter().map(|(name, _)| *name).collect()
    }
}

fn leaf_name(slot: ArtifactSlot) -> Option<String> {
    Some(format!("{}-{}", slot.role, hex::encode(slot.key)))
}

impl CustodyCheckpoints for Recorder<'_> {
    fn intend_workspace(
        &self,
        generation: Id,
        production: FileIdentity,
    ) -> Result<(), NativeRefusal> {
        self.step("intend_workspace", Some(String::new()), || {
            self.ledger.intend_workspace(generation, production)
        })
    }

    fn workspace_directory(
        &self,
        generation: Id,
        directory: FileIdentity,
    ) -> Result<(), NativeRefusal> {
        self.step("workspace_directory", Some(GENERATION_LEASE.into()), || {
            self.ledger.workspace_directory(generation, directory)
        })
    }

    fn workspace_bound(&self, binding: WorkspaceBinding) -> Result<(), NativeRefusal> {
        self.step("workspace_bound", Some(GENERATION_LEASE.into()), || {
            self.ledger.workspace_bound(binding)
        })
    }

    fn confirm_bound(&self, binding: WorkspaceBinding) -> Result<(), NativeRefusal> {
        self.step("confirm_bound", None, || self.ledger.confirm_bound(binding))
    }

    fn workspace_intent_settled(
        &self,
        generation: Id,
        present: bool,
    ) -> Result<(), NativeRefusal> {
        self.step("workspace_intent_settled", None, || {
            self.ledger.workspace_intent_settled(generation, present)
        })
    }

    fn intend_slot(
        &self,
        binding: WorkspaceBinding,
        slot: ArtifactSlot,
    ) -> Result<(), NativeRefusal> {
        self.step("intend_slot", leaf_name(slot), || {
            self.ledger.intend_slot(binding, slot)
        })
    }

    fn slot_identified(
        &self,
        binding: WorkspaceBinding,
        slot: ArtifactSlot,
        identity: FileIdentity,
    ) -> Result<(), NativeRefusal> {
        self.step("slot_identified", leaf_name(slot), || {
            self.ledger.slot_identified(binding, slot, identity)
        })
    }

    fn slot_verified(
        &self,
        binding: WorkspaceBinding,
        slot: ArtifactSlot,
        evidence: ArtifactEvidence,
    ) -> Result<(), NativeRefusal> {
        self.step("slot_verified", leaf_name(slot), || {
            self.ledger.slot_verified(binding, slot, evidence)
        })
    }

    fn slot_intent_settled(
        &self,
        binding: WorkspaceBinding,
        slot: ArtifactSlot,
        present: bool,
    ) -> Result<(), NativeRefusal> {
        self.step("slot_intent_settled", None, || {
            self.ledger.slot_intent_settled(binding, slot, present)
        })
    }
}

/// A private production root, a store with five pending builds, and the
/// owner SID that the reservation checks.
/// Fields drop in order: handles close before the directories go.
pub(in crate::vm::windows::qualification) struct Fixture {
    pub store: Option<VmStore>,
    pub production: PathPin,
    pub owner: Vec<usize>,
    pub builds: Vec<BuildSlots>,
    pub temp: tempfile::TempDir,
    pub state: tempfile::TempDir,
}

impl Fixture {
    pub(in crate::vm::windows::qualification) fn new() -> Self {
        let temp = tempfile::tempdir_in(std::env::temp_dir()).unwrap();
        let (owner, _, production) = private_dir(temp.path(), "production");
        let production = PathPin::open(&production, true, false).unwrap();
        let state = crate::service::application::tests::private_temp();
        let store = VmStore::open(state.path(), 100).unwrap();
        let builds = store
            .transact(|db| {
                (1..=5)
                    .map(|nonce| native::tests::build(db, nonce))
                    .collect()
            })
            .unwrap();
        Self {
            store: Some(store),
            production,
            owner,
            builds,
            temp,
            state,
        }
    }

    pub(in crate::vm::windows::qualification) fn store(&self) -> &VmStore {
        self.store.as_ref().unwrap()
    }

    pub(in crate::vm::windows::qualification) fn recorder(&self, generation: Id) -> Recorder<'_> {
        Recorder::new(self.store(), &self.production.path, generation)
    }

    pub(in crate::vm::windows::qualification) fn reserve(
        &self,
        ledger: &dyn CustodyCheckpoints,
        generation: Id,
    ) -> Result<ProductionWorkspace, NativeRefusal> {
        reserve_workspace_under(&self.production, sid_ptr(&self.owner), generation, ledger)
    }

    pub(in crate::vm::windows::qualification) fn reopen(&mut self) {
        drop(self.store.take());
        self.store = Some(VmStore::open(self.state.path(), 100).unwrap());
    }

    pub(in crate::vm::windows::qualification) fn leaf(&self, slot: ArtifactSlot) -> Leaf {
        self.store().transact(|db| native::leaf(db, slot)).unwrap()
    }

    fn generation_state(&self, generation: Id) -> Option<String> {
        self.store()
            .transact(|db| {
                use rusqlite::OptionalExtension;
                Ok(db
                    .query_row(
                        "SELECT state FROM vm_native_generations WHERE generation=?1",
                        [generation],
                        |row| row.get(0),
                    )
                    .optional()?)
            })
            .unwrap()
    }
}

fn exists(path: &std::path::Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

#[test]
fn reserved_workspace_commits_each_identity_before_the_next_create() {
    let fixture = Fixture::new();
    let generation = fixture.builds[0].pair.disk.generation;
    let recorder = fixture.recorder(generation);
    let ws = fixture.reserve(&recorder, generation).unwrap();
    assert_eq!(
        *recorder.log.borrow(),
        [
            ("intend_workspace", None),
            ("workspace_directory", None),
            ("workspace_bound", Some(0)),
        ]
    );
    let binding = ws.binding();
    assert_eq!(binding.generation, generation);
    assert_eq!(binding.production, fixture.production.identity);
    assert!(recorder.ledger.confirm_bound(binding).is_ok());
    for substitute in [
        WorkspaceBinding {
            workspace: binding.production,
            ..binding
        },
        WorkspaceBinding {
            lease: binding.workspace,
            ..binding
        },
        WorkspaceBinding {
            generation: fixture.builds[1].pair.disk.generation,
            ..binding
        },
    ] {
        assert_eq!(recorder.ledger.confirm_bound(substitute), Err(PATH));
    }
    // The returned workspace owns the only lease lock.
    assert!(
        hold_workspace_under(&fixture.production, sid_ptr(&fixture.owner), binding).is_err()
    );
    // A repeated reservation never adopts the existing directory.
    assert!(fixture.reserve(&recorder, generation).is_err());
    assert_eq!(fixture.generation_state(generation).as_deref(), Some("bound"));
}

#[test]
fn workspace_faults_keep_an_exact_intent_or_quarantine_across_reopen() {
    let mut fixture = Fixture::new();
    let cases = [
        (Fault::Before("intend_workspace"), None, false, false),
        (
            Fault::Before("workspace_directory"),
            Some("quarantined"),
            true,
            false,
        ),
        (
            Fault::After("workspace_directory"),
            Some("quarantined"),
            true,
            false,
        ),
        (
            Fault::Before("workspace_bound"),
            Some("quarantined"),
            true,
            true,
        ),
        (Fault::After("workspace_bound"), Some("bound"), true, true),
    ];
    let generations: Vec<Id> = fixture
        .builds
        .iter()
        .map(|build| build.pair.disk.generation)
        .collect();
    for ((fault, state, directory, lease), generation) in cases.into_iter().zip(&generations) {
        let recorder = fixture.recorder(*generation);
        recorder.fault.set(Some(fault));
        assert!(fixture.reserve(&recorder, *generation).is_err(), "{fault:?}");
        let path = fixture
            .temp
            .path()
            .join("production")
            .join(hex::encode(generation));
        assert_eq!(exists(&path), directory, "{fault:?}");
        assert_eq!(exists(&path.join(GENERATION_LEASE)), lease, "{fault:?}");
        assert_eq!(
            fixture.generation_state(*generation).as_deref(),
            state,
            "{fault:?}"
        );
    }
    fixture.reopen();
    for ((_, state, ..), generation) in cases.into_iter().zip(&generations) {
        assert_eq!(fixture.generation_state(*generation).as_deref(), state);
    }
    // A lost bind acknowledgement leaves an exact scope the owner can hold.
    let generation = generations[4];
    let directory = fixture.production.path.join(&hex::encode(generation)).unwrap();
    let lease = directory.join(GENERATION_LEASE).unwrap();
    let binding = fixture_binding(&fixture.production, generation, &directory, &lease);
    let recorder = fixture.recorder(generation);
    assert!(recorder.ledger.confirm_bound(binding).is_ok());
    assert!(hold_workspace_under(&fixture.production, sid_ptr(&fixture.owner), binding).is_ok());
}

#[test]
fn slot_identity_is_durable_before_the_first_write() {
    let mut fixture = Fixture::new();
    let slots = fixture.builds[0];
    let generation = slots.pair.disk.generation;
    let ws = fixture.reserve(&fixture.recorder(generation), generation).unwrap();
    let recorder = fixture.recorder(generation);
    let mut writer = ws.create_slot(slots.seed, &recorder).unwrap();
    assert_eq!(
        *recorder.log.borrow(),
        [("intend_slot", None), ("slot_identified", Some(0))]
    );
    let id = writer.identity();
    assert_eq!(fixture.leaf(slots.seed), Leaf::Identified(id.file));
    writer.write_all(b"seed").unwrap();
    writer.sync().unwrap();
    drop(writer);

    // A refused intent never creates. A refused identity never writes and
    // leaves the new object quarantined. An occupied leaf is never adopted.
    recorder.fault.set(Some(Fault::Before("intend_slot")));
    assert!(ws.create_slot(slots.pair.disk, &recorder).is_err());
    let disk = slot_path(&ws.root.path, slots.pair.disk);
    assert!(!exists(std::path::Path::new(disk.as_str())));
    assert_eq!(fixture.leaf(slots.pair.disk), Leaf::NeverAttempted);
    recorder.fault.set(Some(Fault::Before("slot_identified")));
    assert!(ws.create_slot(slots.pair.disk, &recorder).is_err());
    assert_eq!(std::fs::read(disk.as_str()).unwrap(), b"");
    assert_eq!(fixture.leaf(slots.pair.disk), Leaf::Unidentified);
    recorder.fault.set(None);
    let vars = slot_path(&ws.root.path, slots.pair.vars);
    std::fs::write(vars.as_str(), b"foreign").unwrap();
    assert!(ws.create_slot(slots.pair.vars, &recorder).is_err());
    assert_eq!(std::fs::read(vars.as_str()).unwrap(), b"foreign");
    assert_eq!(fixture.leaf(slots.pair.vars), Leaf::Unidentified);

    // A lost intent acknowledgement stays charged until exact absence.
    let other = fixture.builds[1];
    let generation = other.pair.disk.generation;
    let second = fixture.reserve(&fixture.recorder(generation), generation).unwrap();
    let recorder = fixture.recorder(generation);
    recorder.fault.set(Some(Fault::After("intend_slot")));
    assert!(second.create_slot(other.seed, &recorder).is_err());
    assert_eq!(fixture.leaf(other.seed), Leaf::Intended);
    recorder.fault.set(None);
    fixture.reopen();
    assert_eq!(fixture.leaf(slots.seed), Leaf::Identified(id.file));
    assert_eq!(fixture.leaf(slots.pair.disk), Leaf::Unidentified);
    assert_eq!(fixture.leaf(other.seed), Leaf::Intended);
    let recorder = fixture.recorder(generation);
    second.settle_intended(other.seed, &recorder).unwrap();
    assert_eq!(fixture.leaf(other.seed), Leaf::Absent);
}

#[test]
fn helper_target_is_identified_and_empty_before_any_write() {
    let fixture = Fixture::new();
    let slots = fixture.builds[0];
    let generation = slots.pair.disk.generation;
    let ws = fixture.reserve(&fixture.recorder(generation), generation).unwrap();
    let recorder = fixture.recorder(generation);
    assert!(
        ws.precreate_helper_target(slots.pair.vars, &recorder)
            .is_err()
    );
    assert!(recorder.log.borrow().is_empty());
    let target = ws.precreate_helper_target(slots.pair.disk, &recorder).unwrap();
    assert_eq!(target.slot(), slots.pair.disk);
    assert_eq!(
        *recorder.log.borrow(),
        [("intend_slot", None), ("slot_identified", Some(0))]
    );
    assert_eq!(
        fixture.leaf(slots.pair.disk),
        Leaf::Identified(target.identity().file)
    );
    let path = slot_path(&ws.root.path, slots.pair.disk);
    // The held target admits the helper's writer but not replacement.
    assert!(
        std::fs::OpenOptions::new()
            .write(true)
            .open(path.as_str())
            .is_ok()
    );
    let moved = ws.root.path.join("moved").unwrap();
    assert!(std::fs::rename(path.as_str(), moved.as_str()).is_err());
    assert_eq!(std::fs::read(path.as_str()).unwrap(), b"");
}

#[test]
fn custody_checkpoints_leave_production_admission_closed() {
    use crate::vm::runtime::{CandidateRuntime, NativeMutation};
    let fixture = Fixture::new();
    let slots = fixture.builds[0];
    let generation = slots.pair.disk.generation;
    let recorder = fixture.recorder(generation);
    let ws = fixture.reserve(&recorder, generation).unwrap();
    drop(ws.precreate_helper_target(slots.pair.disk, &recorder).unwrap());
    let profile = crate::config::VmRuntimeProfile::parse(
        br#"{
        "enabled":true,"owner_sid":"S-1-5-21-1-2-3-1001","supervisor_sid":"S-1-5-21-1-2-3-1002",
        "bundle":"E:/pinned bundle","volume":"D:/","state":"D:/test_vms/state",
        "disk_ceiling_bytes":10737418240,"manifest":"qemu-11.1.3-windows-x64"
    }"#,
    )
    .unwrap();
    for mutation in [
        NativeMutation::CopyMedia,
        NativeMutation::Boot,
        NativeMutation::CreateImage,
    ] {
        assert_eq!(
            CandidateRuntime::new(Some(&profile)).dispatch::<()>(mutation, |_| {
                panic!("durable custody opened a production TEST effect")
            }),
            Err(NativeRefusal::QualificationMissing)
        );
    }
}
