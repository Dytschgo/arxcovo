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

    fn step<T>(
        &self,
        name: &'static str,
        object: Option<String>,
        commit: impl FnOnce() -> Result<T, NativeRefusal>,
    ) -> Result<T, NativeRefusal> {
        let observed = object.and_then(|leaf| {
            std::fs::metadata(self.directory.join(leaf))
                .ok()
                .map(|metadata| metadata.len())
        });
        self.log.borrow_mut().push((name, observed));
        if self.fault.get() == Some(Fault::Before(name)) {
            return Err(PATH);
        }
        let committed = commit()?;
        if self.fault.get() == Some(Fault::After(name)) {
            return Err(PATH);
        }
        Ok(committed)
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
    ) -> Result<Option<FileIdentity>, NativeRefusal> {
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
        leaf: native::WorkspaceLeaf,
        presence: native::Presence,
    ) -> Result<(), NativeRefusal> {
        self.step("workspace_intent_settled", None, || {
            self.ledger
                .workspace_intent_settled(generation, leaf, presence)
        })
    }

    fn confirm_start_pair(
        &self,
        binding: WorkspaceBinding,
        pair: StartPair,
    ) -> Result<(), NativeRefusal> {
        self.step("confirm_start_pair", None, || {
            self.ledger.confirm_start_pair(binding, pair)
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

    fn confirm_identified(
        &self,
        binding: WorkspaceBinding,
        slot: ArtifactSlot,
        identity: FileIdentity,
    ) -> Result<(), NativeRefusal> {
        self.step("confirm_identified", None, || {
            self.ledger.confirm_identified(binding, slot, identity)
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
    assert!(hold_workspace_under(&fixture.production, sid_ptr(&fixture.owner), binding).is_err());
    // A repeated reservation never adopts the existing directory.
    assert!(fixture.reserve(&recorder, generation).is_err());
    assert_eq!(
        fixture.generation_state(generation).as_deref(),
        Some("bound")
    );
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
        // A lost directory acknowledgement keeps the known directory.
        (
            Fault::After("workspace_directory"),
            Some("directory"),
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
        assert!(
            fixture.reserve(&recorder, *generation).is_err(),
            "{fault:?}"
        );
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
    let directory = fixture
        .production
        .path
        .join(&hex::encode(generation))
        .unwrap();
    let lease = directory.join(GENERATION_LEASE).unwrap();
    let binding = fixture_binding(&fixture.production, generation, &directory, &lease);
    let recorder = fixture.recorder(generation);
    assert!(recorder.ledger.confirm_bound(binding).is_ok());
    assert!(hold_workspace_under(&fixture.production, sid_ptr(&fixture.owner), binding).is_ok());

    // After the reopen the known directory keeps its checkpoint with the
    // lease absent. The interrupted build no longer owns the generation, so
    // a stale retry is refused before any filesystem effect.
    let generation = generations[2];
    let path = fixture
        .temp
        .path()
        .join("production")
        .join(hex::encode(generation));
    assert!(
        fixture
            .reserve(&fixture.recorder(generation), generation)
            .is_err()
    );
    assert!(!exists(&path.join(GENERATION_LEASE)));
    assert_eq!(
        fixture.generation_state(generation).as_deref(),
        Some("directory")
    );
    // A settled intent never resumes.
    let generation = generations[1];
    assert!(
        fixture
            .reserve(&fixture.recorder(generation), generation)
            .is_err()
    );
    assert_eq!(
        fixture.generation_state(generation).as_deref(),
        Some("quarantined")
    );
}

/// T14: a lost directory acknowledgement resumes at the lease for the live
/// owner. Nothing is created twice and nothing is quarantined.
#[test]
fn lost_directory_acknowledgement_resumes_at_the_lease() {
    let fixture = Fixture::new();
    let generation = fixture.builds[0].verification.disk.generation;
    let path = fixture
        .temp
        .path()
        .join("production")
        .join(hex::encode(generation));
    let recorder = fixture.recorder(generation);
    recorder
        .fault
        .set(Some(Fault::After("workspace_directory")));
    assert!(fixture.reserve(&recorder, generation).is_err());
    assert_eq!(
        recorder.names(),
        [
            "intend_workspace",
            "workspace_directory",
            "workspace_intent_settled"
        ]
    );
    assert!(!exists(&path.join(GENERATION_LEASE)));
    assert_eq!(
        fixture.generation_state(generation).as_deref(),
        Some("directory")
    );
    let directory = PathPin::open(
        &fixture
            .production
            .path
            .join(&hex::encode(generation))
            .unwrap(),
        true,
        false,
    )
    .unwrap()
    .identity;
    let recorder = fixture.recorder(generation);
    let ws = fixture.reserve(&recorder, generation).unwrap();
    assert_eq!(recorder.names(), ["intend_workspace", "workspace_bound"]);
    assert_eq!(ws.binding().workspace, directory);
    assert_eq!(
        fixture.generation_state(generation).as_deref(),
        Some("bound")
    );
}

#[test]
fn slot_identity_is_durable_before_the_first_write() {
    let mut fixture = Fixture::new();
    let slots = fixture.builds[0];
    let generation = slots.pair.disk.generation;
    let ws = fixture
        .reserve(&fixture.recorder(generation), generation)
        .unwrap();
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
    // Occupancy by the other kind is known occupancy, not a pending intent.
    let other = fixture.builds[2].pair.disk;
    let wrong_kind = fixture.builds[2].seed;
    let third = fixture
        .reserve(&fixture.recorder(other.generation), other.generation)
        .unwrap();
    std::fs::create_dir(slot_path(&third.root.path, wrong_kind).as_str()).unwrap();
    assert!(third.create_slot(wrong_kind, &recorder).is_err());
    assert_eq!(fixture.leaf(wrong_kind), Leaf::Unidentified);
    assert!(std::path::Path::new(slot_path(&third.root.path, wrong_kind).as_str()).is_dir());

    // A lost intent acknowledgement stays charged until exact absence.
    let other = fixture.builds[1];
    let generation = other.pair.disk.generation;
    let second = fixture
        .reserve(&fixture.recorder(generation), generation)
        .unwrap();
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
    let ws = fixture
        .reserve(&fixture.recorder(generation), generation)
        .unwrap();
    let recorder = fixture.recorder(generation);
    assert!(
        ws.precreate_helper_target(slots.pair.vars, &recorder)
            .is_err()
    );
    assert!(recorder.log.borrow().is_empty());
    let target = ws
        .precreate_helper_target(slots.pair.disk, &recorder)
        .unwrap();
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
    drop(
        ws.precreate_helper_target(slots.pair.disk, &recorder)
            .unwrap(),
    );
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

/// Chunk 04's exact start pair resolves only to the durable slots it names.
#[test]
fn start_pair_must_name_the_durable_slot_identities() {
    let fixture = Fixture::new();
    let slots = fixture.builds[0];
    let generation = slots.pair.disk.generation;
    let recorder = fixture.recorder(generation);
    let ws = fixture.reserve(&recorder, generation).unwrap();
    let disk = ws
        .precreate_helper_target(slots.pair.disk, &recorder)
        .unwrap();
    let mut vars = ws.create_slot(slots.pair.vars, &recorder).unwrap();
    vars.write_all(b"vars").unwrap();
    let vars_id = vars.identity();
    drop(vars);
    let mut seed = ws.create_slot(slots.seed, &recorder).unwrap();
    seed.write_all(b"seed").unwrap();
    let seed_id = seed.identity();
    drop(seed);
    let binding = ws.binding();
    let evidence = |identity: FileIdentity| ArtifactEvidence {
        identity,
        directory: binding.workspace.file,
        pin: ArtifactPin {
            size: 4,
            sha256: [1; 32],
        },
    };
    let pair = StartPair {
        slots: slots.pair,
        evidence: [evidence(disk.identity()), evidence(vars_id)],
    };
    assert_eq!(recorder.confirm_start_pair(binding, pair), Ok(()));
    let swapped = StartPair {
        evidence: [pair.evidence[1], pair.evidence[0]],
        ..pair
    };
    let mut moved = pair;
    moved.evidence[0].directory = binding.production.file;
    let foreign = StartPair {
        slots: fixture.builds[1].pair,
        ..pair
    };
    // Both the slots and their evidence swapped: each slot still names its
    // own durable object, but VARS stands in for the disk.
    let reversed = StartPair {
        slots: PairSlots {
            disk: slots.pair.vars,
            vars: slots.pair.disk,
        },
        evidence: [pair.evidence[1], pair.evidence[0]],
    };
    // An identified seed in place of the disk.
    let seeded = StartPair {
        slots: PairSlots {
            disk: slots.seed,
            vars: slots.pair.vars,
        },
        evidence: [evidence(seed_id), pair.evidence[1]],
    };
    for wrong in [swapped, moved, foreign, reversed, seeded] {
        assert_eq!(recorder.confirm_start_pair(binding, wrong), Err(PATH));
    }
    let substitute = WorkspaceBinding {
        lease: binding.workspace,
        ..binding
    };
    assert_eq!(recorder.confirm_start_pair(substitute, pair), Err(PATH));
}

/// I15/T19: a checked Mutate launch accepts only the exact held helper target
/// that the ledger still records as identified and unverified.
#[test]
fn mutate_dispatch_requires_the_durable_helper_target() {
    let fixture = Fixture::new();
    let slots = fixture.builds[0];
    let generation = slots.pair.disk.generation;
    let recorder = fixture.recorder(generation);
    let ws = fixture.reserve(&recorder, generation).unwrap();
    let target = ws
        .precreate_helper_target(slots.pair.disk, &recorder)
        .unwrap();
    let image = synthetic_artifact(
        ArtifactRole::QemuImg,
        LocalPath::parse(r"D:\fixture\qemu-img.exe").unwrap(),
        9,
    );
    let staging = |path: LocalPath, identity: FileIdentity| {
        Artifact::new(ArtifactRole::Staging, path)
            .unwrap()
            .with_identity(identity)
    };
    let target_path = slot_path(&ws.root.path, slots.pair.disk);
    let inputs = |artifact: Artifact| {
        vec![
            (ArtifactRole::QemuImg, image.clone()),
            (ArtifactRole::Staging, artifact),
        ]
    };
    let exact = inputs(staging(target_path.clone(), target.identity()));
    assert_eq!(
        check_helper_target(PlanKind::Mutate, &exact, &ws, Some(&target)),
        Ok(())
    );
    // The target is required for Mutate and refused for every other kind.
    assert_eq!(
        check_helper_target(PlanKind::Mutate, &exact, &ws, None),
        Err(PATH)
    );
    for kind in [PlanKind::InspectWritable, PlanKind::Inspect, PlanKind::Boot] {
        assert_eq!(
            check_helper_target(kind, &exact, &ws, Some(&target)),
            Err(PATH)
        );
        assert_eq!(check_helper_target(kind, &exact, &ws, None), Ok(()));
    }
    // A substituted empty in-workspace file, by its own identity or under
    // the target's identity, is refused.
    let decoy = ws
        .root
        .path
        .join(&format!("disk-{}", hex::encode([0x5a; 16])))
        .unwrap();
    std::fs::write(decoy.as_str(), b"").unwrap();
    let decoy_id = PathPin::open(&decoy, false, false).unwrap().identity;
    for wrong in [
        inputs(staging(decoy.clone(), decoy_id)),
        inputs(staging(decoy.clone(), target.identity())),
        inputs(staging(target_path.clone(), decoy_id)),
    ] {
        assert_eq!(
            check_helper_target(PlanKind::Mutate, &wrong, &ws, Some(&target)),
            Err(PATH)
        );
    }
    let mut doubled = exact.clone();
    doubled.push((ArtifactRole::Scratch, staging(decoy.clone(), decoy_id)));
    assert_eq!(
        check_helper_target(PlanKind::Mutate, &doubled, &ws, Some(&target)),
        Err(PATH)
    );
    // A target from another workspace is refused.
    let other = fixture.builds[1];
    let other_recorder = fixture.recorder(other.pair.disk.generation);
    let other_ws = fixture
        .reserve(&other_recorder, other.pair.disk.generation)
        .unwrap();
    let foreign = other_ws
        .precreate_helper_target(other.pair.disk, &other_recorder)
        .unwrap();
    assert_eq!(
        check_helper_target(PlanKind::Mutate, &exact, &ws, Some(&foreign)),
        Err(PATH)
    );
    // Once the ledger no longer records an unverified target, dispatch stops.
    recorder
        .slot_verified(
            ws.binding(),
            slots.pair.disk,
            ArtifactEvidence {
                identity: target.identity(),
                directory: ws.binding().workspace.file,
                pin: ArtifactPin {
                    size: 1,
                    sha256: [7; 32],
                },
            },
        )
        .unwrap();
    assert_eq!(
        check_helper_target(PlanKind::Mutate, &exact, &ws, Some(&target)),
        Err(PATH)
    );
}
