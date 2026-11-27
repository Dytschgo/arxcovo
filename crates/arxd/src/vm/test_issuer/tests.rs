//! Matched positives run the production issuer bodies against a real ledger
//! with a fake held selection. Every negative has its positive control.

use super::*;
use crate::{
    service::application::tests::private_temp,
    store::vm as store,
    vm::{
        Admission, BuildAdmission, BuildRequest, BuildSlots, Capacity, DiskClaim, HostObservation,
        Operation, OperationRef, Request, RequestId, Resources, ScriptResult, VmPolicy,
        paths::Artifact,
        qemu_args::{BootPurpose, BootRecipe, SerialName},
        qualification::consume_checked,
        qualification_record::Instant2,
        runtime::{CandidateRuntime, NativeCapability, NativeMutation},
    },
};
use Invalidation::{Confirmed, Indeterminate, Transient};
use rusqlite::params;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

const NOW: i64 = 1_800_000_000_000;
const BOOT_MS: u64 = 3_600_000;
const MARKER: Hash = [7; 32];
const SEAL: Hash = [0xa1; 32];
const SCOPE: Hash = [0xb2; 32];
const OWNER: &str = "alice";
const OWNER_SID: &str = "S-1-12-1-1-2-3-4";
/// The fixture build recipe; build 1 carries digest [1; 32].
const BUILD_RECIPE: &str = "e03a3012c18d20b8691490b580ab41e9";
const GIB: u64 = 1 << 30;
const VM: Id = [41; 16];
const TEMPLATE: Id = [42; 16];
const ACTIVE: Id = [43; 16];
const START_EPOCH: Id = [44; 16];
const DISK_KEY: Id = [51; 16];
const VARS_KEY: Id = [52; 16];
const SCRIPT: &str = "Get-Date";

#[derive(Clone)]
struct TestClock(Arc<Mutex<ClockReading>>);

impl Clock for TestClock {
    fn now(&self) -> ClockReading {
        *self.0.lock().unwrap()
    }
}

fn recipes() -> TestRecipes {
    let mut ubuntu = [0; 16];
    hex::decode_to_slice(BUILD_RECIPE, &mut ubuntu).unwrap();
    TestRecipes {
        ubuntu: RecipeIdentity {
            recipe_id: ubuntu,
            recipe_sha256: [1; 32],
        },
        windows: RecipeIdentity {
            recipe_id: VmController::<TestClock>::windows_recipe_id(),
            recipe_sha256: [0x22; 32],
        },
    }
}

fn profile(enrollment: &str, dossier: &str) -> serde_json::Value {
    serde_json::json!({
        "enabled": true, "owner_sid": OWNER_SID, "supervisor_sid": OWNER_SID,
        "bundle": "E:/pinned bundle", "volume": "D:/", "state": "D:/test_vms/state",
        "disk_ceiling_bytes": 10_737_418_240_u64, "manifest": "qemu-11.1.3-windows-x64",
        "execution_profile": "current_user",
        "test_qualification": {
            "enrollment": enrollment, "dossier": dossier,
            "sha256": hex::encode(SEAL), "scope": hex::encode(SCOPE),
        },
    })
}

/// The selection, only through the validated profile parser.
fn configured(enrollment: &str, dossier: &str) -> TestQualificationSelection {
    let bytes = serde_json::to_vec(&profile(enrollment, dossier)).unwrap();
    crate::config::VmRuntimeProfile::parse(&bytes)
        .unwrap()
        .test_qualification()
        .unwrap()
}

pub(crate) fn config() -> TestQualificationSelection {
    configured("D:/private/enrollment.json", "D:/private/dossier")
}

type Hook = Box<dyn FnOnce() + Send>;

/// The held selection, with a scripted refusal, boot marker and an action
/// taken during the next sample.
pub(crate) struct FakeSource {
    seal: Hash,
    class: Class,
    scope: Hash,
    recipes: TestRecipes,
    owner: &'static str,
    origin: Instant,
    marker: Mutex<Hash>,
    fault: Mutex<Option<Invalidation>>,
    during: Mutex<Option<Hook>>,
}

impl FakeSource {
    pub(crate) fn new(origin: Instant) -> Self {
        Self {
            seal: SEAL,
            class: Class::Test,
            scope: SCOPE,
            recipes: recipes(),
            owner: OWNER_SID,
            origin,
            marker: Mutex::new(MARKER),
            fault: Mutex::default(),
            during: Mutex::default(),
        }
    }
}

impl SelectedSource for &FakeSource {
    fn seal_sha256(&self) -> Hash {
        self.seal
    }

    fn class(&self) -> Class {
        self.class
    }

    fn scope(&self) -> Hash {
        self.scope
    }

    fn recipes(&self) -> TestRecipes {
        self.recipes
    }

    fn owner_matches(&self, configured: &str) -> bool {
        configured == self.owner
    }

    fn sample(
        &self,
        advance: &dyn Fn() -> Result<ClockReading, Invalidation>,
    ) -> Result<Sample, Invalidation> {
        let captured = Instant::now();
        if let Some(fault) = *self.fault.lock().unwrap() {
            return Err(fault);
        }
        let reading = advance()?;
        if let Some(during) = self.during.lock().unwrap().take() {
            during();
        }
        let elapsed = reading.monotonic.duration_since(self.origin).as_millis();
        Ok(Sample {
            captured,
            now: Instant2 {
                utc: u64::try_from(reading.utc_ms / 1_000).unwrap(),
                mono_ms: BOOT_MS + u64::try_from(elapsed).unwrap(),
            },
            boot_marker: *self.marker.lock().unwrap(),
            reading,
        })
    }
}

struct Fixture {
    ctl: VmController<TestClock>,
    clock: TestClock,
    source: FakeSource,
    cancel: Shutdown,
    root: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let root = private_temp();
        let origin = Instant::now();
        let clock = TestClock(Arc::new(Mutex::new(ClockReading {
            utc_ms: NOW,
            monotonic: origin,
        })));
        let ctl = VmController::open(root.path(), VmPolicy::default(), clock.clone()).unwrap();
        Self {
            ctl,
            clock,
            source: FakeSource::new(origin),
            cancel: Shutdown::default(),
            root,
        }
    }

    fn issuer(&self) -> TestIssuer<'_, &FakeSource, TestClock> {
        TestIssuer::open(&Installation::fixture(), &self.ctl, &self.source, &config()).unwrap()
    }

    /// Both host clocks move together.
    fn advance(&self, seconds: u64) {
        advance(&self.clock, seconds);
    }

    fn fault(&self, fault: Option<Invalidation>) {
        *self.source.fault.lock().unwrap() = fault;
    }

    fn during_next_sample(&self, hook: Hook) {
        *self.source.during.lock().unwrap() = Some(hook);
    }

    fn sql(&self, run: impl FnOnce(&rusqlite::Connection) -> anyhow::Result<()>) {
        self.ctl.store_for_test().transact(|db| run(db)).unwrap();
    }

    fn build(&self, number: u8) -> (BuildSlots, PendingOperation, OperationRef) {
        self.build_with(build_request(number))
    }

    fn build_with(
        &self,
        request: BuildRequest<'_>,
    ) -> (BuildSlots, PendingOperation, OperationRef) {
        let BuildAdmission::Fresh(slots, worker, receipt) = self
            .ctl
            .admit_build(request, host(), resources(), |_, _| None)
            .unwrap()
            .unwrap()
        else {
            panic!("fresh build required");
        };
        (*slots, worker, receipt)
    }

    /// A TEST VM whose active pair is live: stopped (1) with a clean
    /// initial release, or running (2) on the unresolved start epoch.
    fn instance(&self, state: i64) {
        self.sql(|db| {
            db.execute(
                "INSERT INTO vm_templates(id,guest,recipe_digest,disk_digest,vars_digest,bytes)
                 VALUES(?1,1,?2,?2,?2,1)",
                params![TEMPLATE, [1u8; 32]],
            )?;
            db.execute(
                "INSERT INTO vm_instances(id,template,principal,expires_ms,state)
                 VALUES(?1,?2,?3,?4,?5)",
                params![VM, TEMPLATE, OWNER, NOW + 3_600_000, state],
            )?;
            db.execute(
                "INSERT INTO vm_generations(id,vm_id,generation,active) VALUES(?1,?2,1,1)",
                params![ACTIVE, VM],
            )?;
            for (role, key, file) in [("disk", DISK_KEY, 61u8), ("vars", VARS_KEY, 62)] {
                db.execute(
                    "INSERT INTO vm_artifacts(generation_id,role,object_key,state,volume_id,
                        directory_id,file_id) VALUES(?1,?2,?3,'active',?4,?5,?6)",
                    params![ACTIVE, role, key, 1u64.to_be_bytes(), [2u8; 16], [file; 16]],
                )?;
            }
            if state == 1 {
                db.execute(
                    "INSERT INTO vm_snapshot_release(generation,epoch,disposition)
                     VALUES(?1,?1,'clean')",
                    [ACTIVE],
                )?;
            } else {
                store::snapshot::invalidate_release(db, VM, START_EPOCH)?;
            }
            Ok(())
        });
    }

    fn admit(&self, number: u8, operation: Operation<'_>) -> PendingOperation {
        let request = Request {
            principal: OWNER,
            request_id: key(number),
            vm: VM,
            operation,
            disk: DiskClaim::default(),
        };
        let Admission::Admitted(worker) = self.ctl.admit(request, host(), resources()).unwrap()
        else {
            panic!("fresh admission required");
        };
        worker
    }

    /// A running VM and an admitted script on its start epoch.
    fn script(&self) -> (PendingOperation, NativeTarget) {
        self.instance(2);
        let worker = self.admit(
            9,
            Operation::Script {
                script: SCRIPT,
                timeout_seconds: 30,
            },
        );
        let target = NativeTarget {
            generation: ACTIVE,
            writer_epoch: Some(START_EPOCH),
        };
        (worker, target)
    }

    /// A stopped VM and an admitted start; its writer epoch is its own
    /// operation.
    fn start(&self) -> (PendingOperation, NativeTarget) {
        self.instance(1);
        let worker = self.admit(8, Operation::Start);
        let operation = self.operation(&worker);
        let target = NativeTarget {
            generation: ACTIVE,
            writer_epoch: Some(operation),
        };
        (worker, target)
    }

    fn operation(&self, worker: &PendingOperation) -> Id {
        let mut operation = [0; 16];
        self.sql(|db| {
            operation = db.query_row(
                "SELECT operation_id FROM vm_operations WHERE activity_id=?1",
                [worker.activity_id()],
                |row| row.get(0),
            )?;
            Ok(())
        });
        operation
    }

    /// Bind native custody to a generation, as the staging protocol does.
    fn bind(&self, scope: WorkspaceBinding, origin: &str) {
        self.sql(|db| {
            let active: i64 = db.query_row(
                "SELECT active FROM vm_generations WHERE id=?1",
                [scope.generation],
                |row| row.get(0),
            )?;
            db.execute(
                "UPDATE vm_generations SET active=0 WHERE id=?1",
                [scope.generation],
            )?;
            db.execute(
                "INSERT INTO vm_native_generations
                 (generation,host,origin,protocol,volume,production,state,directory,lease)
                 VALUES(?1,(SELECT id FROM vm_state_host WHERE singleton=1),?2,1,?3,?4,
                        'bound',?5,?6)",
                params![
                    scope.generation,
                    origin,
                    1u64.to_be_bytes(),
                    scope.production.file,
                    scope.workspace.file,
                    scope.lease.file
                ],
            )?;
            db.execute(
                "UPDATE vm_generations SET active=?2 WHERE id=?1",
                params![scope.generation, active],
            )?;
            Ok(())
        });
    }
}

fn advance(clock: &TestClock, seconds: u64) {
    let mut reading = clock.0.lock().unwrap();
    reading.utc_ms += i64::try_from(seconds * 1_000).unwrap();
    reading.monotonic += Duration::from_secs(seconds);
}

fn key(number: u8) -> RequestId {
    RequestId::parse(&format!("{NOW:016x}{number:016x}")).unwrap()
}

fn build_request(number: u8) -> BuildRequest<'static> {
    let disk = DiskClaim {
        images: 1 << 20,
        metadata: 4096,
        staging: 0,
    };
    BuildRequest {
        principal: OWNER,
        key: key(number),
        recipe: BUILD_RECIPE,
        digest: [number; 32],
        retained: disk,
        transient: disk,
    }
}

fn host() -> HostObservation {
    HostObservation {
        owner_idle: Some(true),
        logical_cpus: Some(8),
        available_ram: Some(32 * GIB),
        capacity: Some(Capacity {
            free_bytes: 400 * GIB,
            managed_bytes: 0,
        }),
    }
}

fn resources() -> Resources {
    Resources {
        vcpus: 2,
        guest_ram: 4 * GIB,
        overhead_ram: 256 << 20,
    }
}

fn build_target(slots: &BuildSlots) -> NativeTarget {
    NativeTarget {
        generation: slots.pair.disk.generation,
        writer_epoch: None,
    }
}

fn workspace(generation: Id) -> WorkspaceBinding {
    let id = |n| FileIdentity {
        volume: 1,
        file: [n; 16],
    };
    WorkspaceBinding {
        generation,
        production: id(1),
        workspace: id(2),
        lease: id(3),
    }
}

fn pair() -> PairSlots {
    PairSlots {
        disk: ArtifactSlot {
            generation: ACTIVE,
            role: "disk",
            key: DISK_KEY,
        },
        vars: ArtifactSlot {
            generation: ACTIVE,
            role: "vars",
            key: VARS_KEY,
        },
    }
}

/// An unbacked qcow2 v3 header for the plan builders.
fn header() -> Vec<u8> {
    let mut cluster = vec![0u8; 65_536];
    cluster[..4].copy_from_slice(b"QFI\xfb");
    cluster[4..8].copy_from_slice(&3u32.to_be_bytes());
    cluster[20..24].copy_from_slice(&16u32.to_be_bytes());
    cluster[24..32].copy_from_slice(&(1u64 << 30).to_be_bytes());
    cluster[96..100].copy_from_slice(&4u32.to_be_bytes());
    cluster[100..104].copy_from_slice(&104u32.to_be_bytes());
    cluster
}

fn artifact(role: ArtifactRole, path: &str, id: u8) -> Artifact {
    Artifact::new(role, LocalPath::parse(path).unwrap())
        .unwrap()
        .with_identity(FileIdentity {
            volume: 1,
            file: [id; 16],
        })
        .with_pin(ArtifactPin {
            size: 64,
            sha256: [id; 32],
        })
}

/// The inputs of an installed boot.
struct BootInputs {
    qemu: Artifact,
    data: Artifact,
    firmware: Artifact,
    vars: Artifact,
    disk: Artifact,
    header: Vec<u8>,
    serial: SerialName,
}

impl BootInputs {
    fn new() -> Self {
        Self {
            qemu: artifact(ArtifactRole::Qemu, "E:/bundle/qemu.exe", 10),
            data: artifact(ArtifactRole::Data, "E:/bundle/share", 11),
            firmware: artifact(ArtifactRole::FirmwareCode, "E:/bundle/OVMF_CODE.fd", 12),
            vars: artifact(ArtifactRole::Vars, "D:/vms/work/vars.fd", 21),
            disk: artifact(ArtifactRole::GuestDisk, "D:/vms/work/disk.qcow2", 22),
            header: header(),
            serial: SerialName::generate().unwrap(),
        }
    }

    fn recipe(&self) -> BootRecipe<'_> {
        BootRecipe {
            image: &self.qemu,
            data: &self.data,
            firmware: &self.firmware,
            vars: &self.vars,
            disk: &self.disk,
            disk_header: &self.header,
            backing: None,
            backing_header: None,
            serial: &self.serial,
            boot: BootPurpose::Installed,
            ram_mib: 1024,
            vcpus: 2,
        }
    }
}

fn target<'a>(
    plan: &'a CommandPlan<'a>,
    cwd: &'a LocalPath,
    writer_epoch: Id,
) -> LaunchTarget<'a, 'a> {
    LaunchTarget {
        plan,
        writer_epoch,
        cwd,
        pair: None,
        installer: None,
        recipe: None,
        helper: None,
        memory: 1 << 30,
        cpu_rate: 2500,
    }
}

fn stage_target(slot: ArtifactSlot) -> StageTarget {
    StageTarget {
        workspace: workspace(slot.generation),
        seal_sha256: SEAL,
        recipe: recipes().ubuntu,
        phase: InstallerPhase::UbuntuInstall,
        slot,
        content: ArtifactPin {
            size: 4,
            sha256: digest(b"seed"),
        },
        deadline: Instant::now() + Duration::from_secs(600),
    }
}

/// The production boundary stays closed whatever the fixture issued.
pub(crate) fn assert_admission_closed() {
    let bytes =
        serde_json::to_vec(&profile("D:/private/enrollment.json", "D:/private/dossier")).unwrap();
    let profile = crate::config::VmRuntimeProfile::parse(&bytes).unwrap();
    assert!(profile.test_qualification().is_some());
    let runtime = CandidateRuntime::new(Some(&profile));
    assert!(matches!(
        runtime.capability(),
        NativeCapability::Unavailable(NativeRefusal::QualificationMissing)
    ));
    assert!(runtime.dispatch(NativeMutation::Boot, |_| ()).is_err());
}

/// T01/T02/I01: the held seal, scope, class, recipe pair and owner must
/// match the configured selection exactly.
#[test]
fn open_requires_the_exact_selection() {
    let fixture = Fixture::new();
    let issuer = fixture.issuer();
    assert!(!issuer.epoch().effects_revoked());
    let open = |source: &FakeSource| {
        TestIssuer::open(&Installation::fixture(), &fixture.ctl, source, &config()).err()
    };
    let windows = recipes().windows.recipe_id;
    type Mutate = fn(&mut FakeSource, Id);
    let mutations: [(&str, Mutate, NativeRefusal); 10] = [
        ("missing seal", |s, _| s.seal = [0; 32], REFUSED),
        ("seal", |s, _| s.seal = [0xa2; 32], REFUSED),
        ("scope", |s, _| s.scope = [0xb3; 32], REFUSED),
        (
            "static class",
            |s, _| s.class = Class::StaticOrigin,
            REFUSED,
        ),
        ("docker class", |s, _| s.class = Class::Docker, REFUSED),
        (
            "windows recipe",
            |s, _| s.recipes.windows.recipe_id = [0x33; 16],
            REFUSED,
        ),
        (
            "ubuntu as windows",
            |s, w| s.recipes.ubuntu.recipe_id = w,
            REFUSED,
        ),
        (
            "ubuntu as web",
            |s, _| s.recipes.ubuntu.recipe_id = WEB_RECIPE_ID,
            REFUSED,
        ),
        (
            "unset ubuntu",
            |s, _| s.recipes.ubuntu.recipe_id = [0; 16],
            REFUSED,
        ),
        (
            "owner",
            |s, _| s.owner = "S-1-12-1-1-2-3-5",
            NativeRefusal::IdentityUnverified,
        ),
    ];
    for (name, mutate, refusal) in mutations {
        let mut source = FakeSource::new(fixture.source.origin);
        mutate(&mut source, windows);
        assert_eq!(open(&source), Some(refusal), "{name}");
    }
    // A refused first observation opens no epoch, whatever its class.
    for fault in [
        Transient(REFUSED),
        Indeterminate(REFUSED),
        Confirmed(REFUSED),
    ] {
        let source = FakeSource::new(fixture.source.origin);
        *source.fault.lock().unwrap() = Some(fault);
        assert_eq!(open(&source), Some(REFUSED));
    }
    assert_admission_closed();
}

/// Startup parity: the same configured selection reaches the production
/// opener and is refused there without a held dossier, while the matched
/// fake-held source opens.
#[test]
fn configured_startup_refuses_without_held_evidence() {
    let fixture = Fixture::new();
    let missing = private_temp();
    let path = |leaf: &str| {
        let path = missing.path().join("absent").join(leaf);
        path.to_str().unwrap().replace('\\', "/")
    };
    let configured = configured(&path("enrollment.json"), &path("dossier"));
    assert!(
        TestIssuer::open(
            &Installation::fixture(),
            &fixture.ctl,
            &fixture.source,
            &configured
        )
        .is_ok()
    );
    assert!(TestIssuer::open_configured(Arc::new(fixture.ctl), &configured).is_err());
    assert_admission_closed();
}

/// T05/I04 and T03/T04: the catalog is the exact seal and two recipe pins.
/// A transient refusal keeps the epoch; confirmed drift revokes it.
#[test]
fn catalog_reports_exact_pins_and_follows_the_epoch() {
    let fixture = Fixture::new();
    let issuer = fixture.issuer();
    let expected = serde_json::json!({
        "available": true,
        "selected_test_qualification": {
            "sha256": "a1".repeat(32),
            "recipes": [
                {"guest": "ubuntu", "recipe_id": BUILD_RECIPE, "recipe_sha256": "01".repeat(32)},
                {"guest": "windows", "recipe_id": hex::encode(recipes().windows.recipe_id),
                    "recipe_sha256": "22".repeat(32)},
            ],
        }
    });
    assert_eq!(catalog(SEAL, recipes()), expected);
    assert_eq!(issuer.catalog(), Ok(expected.clone()));
    fixture.fault(Some(Transient(NativeRefusal::OwnerActivityUnverified)));
    assert_eq!(
        issuer.catalog(),
        Err(NativeRefusal::OwnerActivityUnverified)
    );
    fixture.fault(None);
    fixture.advance(1);
    assert_eq!(issuer.catalog(), Ok(expected));
    assert!(!issuer.epoch().effects_revoked());
    *fixture.source.marker.lock().unwrap() = [8; 32];
    assert_eq!(issuer.catalog(), Err(REFUSED));
    assert!(issuer.epoch().effects_revoked());
    *fixture.source.marker.lock().unwrap() = MARKER;
    assert_eq!(issuer.catalog(), Err(REFUSED));
    assert_admission_closed();
}

/// I05/T07/T08: an intent needs the worker's admitted request and its
/// pending row. A replay gets no worker, a worker mints once, and a finished,
/// cancelled or unadmitted worker mints nothing.
#[test]
fn intent_follows_durable_admission_and_replay_mints_nothing() {
    let fixture = Fixture::new();
    let issuer = fixture.issuer();
    let (slots, worker, receipt) = fixture.build(1);
    let exact = build_target(&slots);
    let cancel = &fixture.cancel;
    for (name, target) in [
        (
            "generation",
            NativeTarget {
                generation: [9; 16],
                ..exact
            },
        ),
        (
            "writer epoch",
            NativeTarget {
                writer_epoch: Some(receipt.id),
                ..exact
            },
        ),
    ] {
        assert!(issuer.intent(&worker, target, cancel).is_err(), "{name}");
    }
    // A detached worker carries no admitted request.
    let detached = PendingOperation::for_test(worker.activity_id(), worker.deadline());
    assert!(issuer.intent(&detached, exact, cancel).is_err());
    // Refusals mint nothing, so the exact intent still mints, once.
    let intent = issuer.intent(&worker, exact, cancel).unwrap();
    assert!(issuer.intent(&worker, exact, cancel).is_err());
    drop(intent);
    assert!(issuer.intent(&worker, exact, cancel).is_err());
    // A replay returns the original receipt and no worker to mint with.
    let BuildAdmission::Existing(replay) = fixture
        .ctl
        .admit_build(build_request(1), host(), resources(), |_, _| None)
        .unwrap()
        .unwrap()
    else {
        panic!("the same key must replay the original receipt");
    };
    assert_eq!(replay, receipt);

    // A finished, cancelled or unselected-recipe build mints nothing; each
    // has its matched positive.
    for (name, number, finish, cancelled) in [
        ("exact", 1, false, false),
        ("finished", 1, true, false),
        ("cancelled", 1, false, true),
        ("unselected recipe", 3, false, false),
    ] {
        let fixture = Fixture::new();
        let issuer = fixture.issuer();
        let (slots, worker, _) = fixture.build(number);
        if finish {
            fixture.ctl.fail_build(&worker, slots, false).unwrap();
        }
        if cancelled {
            fixture.cancel.cancel();
        }
        let minted = issuer.intent(&worker, build_target(&slots), &fixture.cancel);
        assert_eq!(minted.is_ok(), name == "exact", "{name}");
    }
    assert_admission_closed();
}

/// Review 7: the one-intent latch never resets, and only the controller
/// mints workers with an admitted request.
#[test]
fn the_intent_latch_is_claimed_once() {
    let worker = PendingOperation::for_test(1, Instant::now() + Duration::from_secs(60));
    assert!(worker.admitted().is_none());
    assert!(worker.claim_intent());
    assert!(!worker.claim_intent());
    assert!(!worker.claim_intent());
}

/// Review 3: a refresh that outlives cancellation, the worker deadline or
/// the pending admission refuses, though its own observation succeeds.
#[test]
fn a_refresh_that_outlives_the_worker_refuses() {
    type Drift = fn(&Fixture) -> Option<Hook>;
    let cases: [(&str, Drift); 4] = [
        ("exact", |_| None),
        ("cancelled", |f| {
            let cancel = f.cancel.clone();
            Some(Box::new(move || cancel.cancel()))
        }),
        ("expired", |f| {
            let clock = f.clock.clone();
            Some(Box::new(move || advance(&clock, 30)))
        }),
        ("lease lost", |f| {
            let database = f.root.path().join("activity.sqlite3");
            Some(Box::new(move || {
                let db = rusqlite::Connection::open(database).unwrap();
                db.execute("UPDATE vm_instances SET expires_ms=0 WHERE id=?1", [VM])
                    .unwrap();
            }))
        }),
    ];
    for (name, drift) in cases {
        let fixture = Fixture::new();
        let issuer = fixture.issuer();
        let (worker, target) = fixture.script();
        if let Some(hook) = drift(&fixture) {
            fixture.during_next_sample(hook);
        }
        let minted = issuer.intent(&worker, target, &fixture.cancel);
        assert_eq!(minted.is_ok(), name == "exact", "{name}");
    }

    // The same holds at consumption.
    for cancel in [false, true] {
        let fixture = Fixture::new();
        let issuer = fixture.issuer();
        let (worker, target) = fixture.script();
        let intent = issuer.intent(&worker, target, &fixture.cancel).unwrap();
        let permit = intent.script(SCRIPT, 30).unwrap();
        if cancel {
            let cancel = fixture.cancel.clone();
            fixture.during_next_sample(Box::new(move || cancel.cancel()));
        }
        let consumed = permit.consume(ACTIVE, START_EPOCH, SCRIPT, 30);
        assert_eq!(consumed.is_ok(), !cancel);
    }
    assert_admission_closed();
}

/// I06/T11/T12: a helper permit names one exact plan; consumption refuses a
/// substituted input, a stale epoch, cancellation, a finished operation or
/// an expired worker.
#[test]
fn launch_permit_binds_the_exact_plan_and_refreshes_at_consumption() {
    let image = artifact(ArtifactRole::QemuImg, "E:/bundle/qemu-img.exe", 10);
    let golden = artifact(ArtifactRole::Golden, "D:/vms/golden/disk.qcow2", 11);
    let swapped = artifact(ArtifactRole::Golden, "D:/vms/golden/disk.qcow2", 12);
    let header = header();
    let plan = crate::vm::qemu_img::golden_info(&image, &golden, &header).unwrap();
    let other = crate::vm::qemu_img::golden_info(&image, &swapped, &header).unwrap();
    let cwd = LocalPath::parse("D:/vms/work").unwrap();

    type Drift = fn(&Fixture, &BuildSlots, &PendingOperation);
    let stale: [(&str, Option<Drift>); 5] = [
        ("exact", None),
        (
            "confirmed drift",
            Some(|f, _, _| *f.source.marker.lock().unwrap() = [8; 32]),
        ),
        ("cancelled", Some(|f, _, _| f.cancel.cancel())),
        (
            "finished",
            Some(|f, s, w| {
                f.ctl.fail_build(w, *s, false).unwrap();
            }),
        ),
        ("expired", Some(|f, _, _| f.advance(3_900))),
    ];
    for (name, drift) in stale {
        let fixture = Fixture::new();
        let issuer = fixture.issuer();
        let (slots, worker, _) = fixture.build(1);
        let target_gen = build_target(&slots);
        let epoch = target_gen.generation;
        let intent = issuer.intent(&worker, target_gen, &fixture.cancel).unwrap();
        let ws = workspace(target_gen.generation);
        let permit = intent.launch(ws, target(&plan, &cwd, epoch)).unwrap();
        // One permit per durable effect; memory is not part of its identity.
        let resized = LaunchTarget {
            memory: 2 << 30,
            ..target(&plan, &cwd, epoch)
        };
        assert!(intent.launch(ws, resized).is_err(), "{name}");
        if let Some(drift) = drift {
            drift(&fixture, &slots, &worker);
            assert!(
                permit.consume(target(&plan, &cwd, epoch)).is_err(),
                "{name}"
            );
        } else {
            let authority = permit.consume(target(&plan, &cwd, epoch)).unwrap();
            let binding = authority.binding();
            assert_eq!(
                binding,
                target(&plan, &cwd, epoch).binding(ws, SEAL).unwrap()
            );
            assert_eq!(
                (binding.workspace, binding.seal_sha256, binding.kind),
                (ws, SEAL, PlanKind::Inspect)
            );
            assert_eq!(
                (binding.writer_epoch, binding.pair, binding.helper),
                (epoch, None, None)
            );
        }
    }

    let fixture = Fixture::new();
    let issuer = fixture.issuer();
    let (slots, worker, _) = fixture.build(1);
    let target_gen = build_target(&slots);
    let epoch = target_gen.generation;
    let intent = issuer.intent(&worker, target_gen, &fixture.cancel).unwrap();
    let ws = workspace(target_gen.generation);
    let wrong_ws = workspace(slots.verification.disk.generation);
    assert!(intent.launch(wrong_ws, target(&plan, &cwd, epoch)).is_err());
    assert!(intent.launch(ws, target(&plan, &cwd, [9; 16])).is_err());
    // A helper carries no recipe, and an inspection names no slot.
    let with_recipe = LaunchTarget {
        recipe: Some(recipes().ubuntu),
        ..target(&plan, &cwd, epoch)
    };
    assert!(intent.launch(ws, with_recipe).is_err());
    let with_slot = LaunchTarget {
        helper: Some(slots.pair.disk),
        ..target(&plan, &cwd, epoch)
    };
    assert!(intent.launch(ws, with_slot).is_err());
    let permit = intent.launch(ws, target(&plan, &cwd, epoch)).unwrap();
    assert!(permit.consume(target(&other, &cwd, epoch)).is_err());
    assert_admission_closed();
}

/// Reviews 1 and 2: a start boots only its own live pair, derived from the
/// target, with a selected recipe, and its admission permits no other
/// effect.
#[test]
fn start_boot_derives_its_pair_and_admits_only_a_boot() {
    let inputs = BootInputs::new();
    let recipe = inputs.recipe();
    let plan = recipe.plan().unwrap();
    let cwd = LocalPath::parse("D:/vms/work").unwrap();
    let image = artifact(ArtifactRole::QemuImg, "E:/bundle/qemu-img.exe", 10);
    let golden = artifact(ArtifactRole::Golden, "D:/vms/golden/disk.qcow2", 11);
    let header = header();
    let inspect = crate::vm::qemu_img::golden_info(&image, &golden, &header).unwrap();

    let fixture = Fixture::new();
    let issuer = fixture.issuer();
    let (worker, target_gen) = fixture.start();
    let epoch = target_gen.writer_epoch.unwrap();
    let boot = |pair, recipe| LaunchTarget {
        pair: Some(pair),
        recipe,
        ..target(&plan, &cwd, epoch)
    };
    let ws = workspace(ACTIVE);
    // A start's writer epoch is its own operation.
    let stale = NativeTarget {
        writer_epoch: Some(START_EPOCH),
        ..target_gen
    };
    assert!(issuer.intent(&worker, stale, &fixture.cancel).is_err());
    let intent = issuer.intent(&worker, target_gen, &fixture.cancel).unwrap();
    let windows = Some(recipes().windows);
    let mut foreign = pair();
    foreign.disk.key = [9; 16];
    let mut swapped = pair();
    swapped.disk.role = "vars";
    swapped.vars.role = "disk";
    let mut unrelated = pair();
    unrelated.vars = ArtifactSlot {
        generation: ACTIVE,
        role: "seed",
        key: VARS_KEY,
    };
    let foreign_recipe = Some(RecipeIdentity {
        recipe_id: [0x55; 16],
        recipe_sha256: [0x56; 32],
    });
    for (name, target) in [
        ("foreign slot", boot(foreign, windows)),
        ("swapped roles", boot(swapped, windows)),
        ("unrelated slot", boot(unrelated, windows)),
        ("unselected recipe", boot(pair(), foreign_recipe)),
        ("no recipe", boot(pair(), None)),
        (
            "wrong writer epoch",
            LaunchTarget {
                writer_epoch: START_EPOCH,
                ..boot(pair(), windows)
            },
        ),
    ] {
        assert!(intent.launch(ws, target).is_err(), "{name}");
    }
    assert!(intent.launch(ws, target(&inspect, &cwd, epoch)).is_err());
    assert!(intent.script(SCRIPT, 30).is_err());
    assert!(intent.stage(stage_target(pair().disk)).is_err());
    // An installed boot carries no installer stage.
    let stage =
        crate::vm::runtime::InstallerBudget::start(crate::vm::runtime::InstallerRecipe::Windows)
            .stage(InstallerPhase::WindowsSetup, Duration::from_secs(60))
            .unwrap();
    let with_stage = LaunchTarget {
        installer: Some(stage),
        ..boot(pair(), windows)
    };
    assert!(intent.launch(ws, with_stage).is_err());
    let permit = intent.launch(ws, boot(pair(), windows)).unwrap();
    assert!(intent.launch(ws, boot(pair(), windows)).is_err());
    // Varying inputs cannot mint a second boot permit for the same pair.
    let mut altered_inputs = BootInputs::new();
    altered_inputs.disk = artifact(ArtifactRole::GuestDisk, "D:/vms/work/disk.qcow2", 29);
    let altered_recipe = altered_inputs.recipe();
    let altered_plan = altered_recipe.plan().unwrap();
    let altered_boot = LaunchTarget {
        pair: Some(pair()),
        recipe: windows,
        ..target(&altered_plan, &cwd, epoch)
    };
    assert!(intent.launch(ws, altered_boot).is_err());
    let authority = permit.consume(boot(pair(), windows)).unwrap();
    assert_eq!(authority.binding().kind, PlanKind::Boot);
    assert_admission_closed();
}

/// Review 2: the checked launch adapter recomputes the full issuance
/// binding from its own target, so an authority issued for another target
/// with the same workspace, seal and plan kind cannot pass the handoff.
#[test]
fn checked_launch_adapter_refuses_an_authority_for_another_target() {
    /// A crate-implementable permit that returns an authority it already
    /// holds, whatever target it is asked to consume.
    struct Reused(LaunchAuthority);

    impl LaunchPermit for Reused {
        fn consume(self, _: LaunchTarget<'_, '_>) -> Result<LaunchAuthority, NativeRefusal> {
            Ok(self.0)
        }
    }

    let inputs = BootInputs::new();
    let recipe = inputs.recipe();
    let plan = recipe.plan().unwrap();
    let mut altered = BootInputs::new();
    altered.disk = artifact(ArtifactRole::GuestDisk, "D:/vms/work/disk.qcow2", 29);
    let altered_recipe = altered.recipe();
    let altered_plan = altered_recipe.plan().unwrap();
    let cwd = LocalPath::parse("D:/vms/work").unwrap();
    let other_cwd = LocalPath::parse("D:/vms/other").unwrap();
    let ws = workspace(ACTIVE);
    fn boot<'a>(
        plan: &'a CommandPlan<'a>,
        cwd: &'a LocalPath,
        pair: PairSlots,
        epoch: Id,
    ) -> LaunchTarget<'a, 'a> {
        LaunchTarget {
            pair: Some(pair),
            recipe: Some(recipes().windows),
            ..target(plan, cwd, epoch)
        }
    }
    // One real authority, issued for the exact boot of a fresh start.
    let issued = || {
        let fixture = Fixture::new();
        let issuer = fixture.issuer();
        let (worker, target_gen) = fixture.start();
        let epoch = target_gen.writer_epoch.unwrap();
        let intent = issuer.intent(&worker, target_gen, &fixture.cancel).unwrap();
        let permit = intent.launch(ws, boot(&plan, &cwd, pair(), epoch)).unwrap();
        let authority = permit.consume(boot(&plan, &cwd, pair(), epoch)).unwrap();
        (authority, epoch)
    };

    let (authority, epoch) = issued();
    let exact = consume_checked(
        Reused(authority),
        boot(&plan, &cwd, pair(), epoch),
        ws,
        SEAL,
    );
    assert_eq!(exact.unwrap().binding().kind, PlanKind::Boot);

    let mut foreign = pair();
    foreign.disk.key = [9; 16];
    for name in [
        "plan",
        "slots",
        "writer epoch",
        "working directory",
        "limits",
    ] {
        let (authority, epoch) = issued();
        let target = match name {
            "plan" => boot(&altered_plan, &cwd, pair(), epoch),
            "slots" => boot(&plan, &cwd, foreign, epoch),
            "writer epoch" => boot(&plan, &cwd, pair(), START_EPOCH),
            "working directory" => boot(&plan, &other_cwd, pair(), epoch),
            _ => LaunchTarget {
                memory: 2 << 30,
                ..boot(&plan, &cwd, pair(), epoch)
            },
        };
        let binding = target.binding(ws, SEAL).unwrap();
        assert_eq!(
            (binding.workspace, binding.seal_sha256, binding.kind),
            (ws, SEAL, PlanKind::Boot),
            "{name}"
        );
        assert_eq!(
            consume_checked(Reused(authority), target, ws, SEAL).err(),
            Some(REFUSED),
            "{name}"
        );
    }
    assert_admission_closed();
}

/// T11/T12/T19: a staged write names its slot, content, the admitted build
/// recipe and its phase, and a deadline. Consumption refuses substituted
/// content or a stale epoch.
#[test]
fn stage_permit_binds_slot_content_phase_and_deadline() {
    let setup = |fixture: &Fixture| {
        let (slots, worker, _) = fixture.build(1);
        let target = NativeTarget {
            generation: slots.seed.generation,
            writer_epoch: None,
        };
        (slots, worker, target)
    };
    let fixture = Fixture::new();
    let issuer = fixture.issuer();
    let (slots, worker, target) = setup(&fixture);
    let intent = issuer.intent(&worker, target, &fixture.cancel).unwrap();
    let exact = stage_target(slots.seed);
    let mut refused = Vec::new();
    // Review 2: the build's live guest disk is never a staging slot.
    refused.push(("build guest disk", stage_target(slots.pair.disk)));
    let mut role = exact;
    role.slot.role = "vars";
    refused.push(("role", role));
    let mut key = exact;
    key.slot.key = [7; 16];
    refused.push(("key", key));
    let mut seal = exact;
    seal.seal_sha256 = [0xa2; 32];
    refused.push(("seal", seal));
    let mut phase = exact;
    phase.phase = InstallerPhase::WindowsSetup;
    refused.push(("phase", phase));
    let mut recipe = exact;
    recipe.recipe = recipes().windows;
    recipe.phase = InstallerPhase::WindowsSetup;
    refused.push(("another selected recipe", recipe));
    let mut expired = exact;
    expired.deadline = Instant::now();
    refused.push(("deadline", expired));
    for (name, target) in refused {
        assert!(intent.stage(target).is_err(), "{name}");
    }
    assert!(intent.script(SCRIPT, 30).is_err());
    let permit = intent.stage(exact).unwrap();
    assert!(intent.stage(exact).is_err());
    let mut content = exact;
    content.content.sha256 = digest(b"swap");
    assert!(permit.consume(content).is_err());

    // Exact consumption passes; a transient refusal at the write refuses.
    for fault in [None, Some(Transient(REFUSED))] {
        let fixture = Fixture::new();
        let issuer = fixture.issuer();
        let (slots, worker, target) = setup(&fixture);
        let intent = issuer.intent(&worker, target, &fixture.cancel).unwrap();
        let exact = stage_target(slots.seed);
        let permit = intent.stage(exact).unwrap();
        fixture.fault(fault);
        let consumed = permit.consume(exact);
        assert_eq!(consumed.is_ok(), fault.is_none());
        if let Ok(authority) = consumed {
            assert_eq!(authority.target(), &exact);
        }
        assert!(!issuer.epoch().effects_revoked());
    }

    // Review 2: an admitted Windows build stages only its seed and VARS in
    // Setup. Its guest disk and the finalization phase are refused at
    // issuance even though the recipe is selected for that phase.
    let fixture = Fixture::new();
    let issuer = fixture.issuer();
    let windows_id = hex::encode(VmController::<TestClock>::windows_recipe_id());
    let (slots, worker, _) = fixture.build_with(BuildRequest {
        recipe: &windows_id,
        digest: recipes().windows.recipe_sha256,
        ..build_request(2)
    });
    let target = NativeTarget {
        generation: slots.seed.generation,
        writer_epoch: None,
    };
    let intent = issuer.intent(&worker, target, &fixture.cancel).unwrap();
    let windows = |slot, phase| StageTarget {
        recipe: recipes().windows,
        phase,
        ..stage_target(slot)
    };
    for (name, target) in [
        (
            "guest disk",
            windows(slots.pair.disk, InstallerPhase::WindowsSetup),
        ),
        (
            "finalization VARS",
            windows(slots.pair.vars, InstallerPhase::WindowsFinalize),
        ),
        (
            "finalization seed",
            windows(slots.seed, InstallerPhase::WindowsFinalize),
        ),
    ] {
        assert!(intent.stage(target).is_err(), "{name}");
    }
    let setup = windows(slots.pair.vars, InstallerPhase::WindowsSetup);
    let authority = intent.stage(setup).unwrap().consume(setup).unwrap();
    assert_eq!(authority.target(), &setup);
    assert_admission_closed();
}

/// I06/T11 and review 2: a script permit binds the admitted bytes and
/// timeout, the generation and the unresolved writer epoch, and refreshes
/// at channel dispatch.
#[test]
fn script_permit_binds_admitted_bytes_timeout_and_writer_epoch() {
    let fixture = Fixture::new();
    let issuer = fixture.issuer();
    let (worker, target_gen) = fixture.script();
    for writer_epoch in [None, Some([46; 16])] {
        let target = NativeTarget {
            writer_epoch,
            ..target_gen
        };
        assert!(issuer.intent(&worker, target, &fixture.cancel).is_err());
    }
    let intent = issuer.intent(&worker, target_gen, &fixture.cancel).unwrap();
    // Only the admitted request's bytes and timeout mint.
    assert!(intent.script("Remove-Item C:/", 30).is_err());
    assert!(intent.script(SCRIPT, 31).is_err());
    // A script admission permits no other effect.
    let image = artifact(ArtifactRole::QemuImg, "E:/bundle/qemu-img.exe", 10);
    let golden = artifact(ArtifactRole::Golden, "D:/vms/golden/disk.qcow2", 11);
    let header = header();
    let inspect = crate::vm::qemu_img::golden_info(&image, &golden, &header).unwrap();
    let cwd = LocalPath::parse("D:/vms/work").unwrap();
    assert!(
        intent
            .launch(workspace(ACTIVE), target(&inspect, &cwd, START_EPOCH))
            .is_err()
    );
    assert!(intent.stage(stage_target(pair().disk)).is_err());
    let permit = intent.script(SCRIPT, 30).unwrap();
    assert!(intent.script(SCRIPT, 30).is_err());
    assert!(
        permit
            .consume(ACTIVE, START_EPOCH, "Get-Date ", 30)
            .is_err()
    );
    for (generation, epoch, script, timeout, accepted) in [
        (ACTIVE, START_EPOCH, SCRIPT, 31, false),
        (ACTIVE, [46; 16], SCRIPT, 30, false),
        ([45; 16], START_EPOCH, SCRIPT, 30, false),
        (ACTIVE, START_EPOCH, SCRIPT, 30, true),
    ] {
        let fixture = Fixture::new();
        let issuer = fixture.issuer();
        let (worker, target_gen) = fixture.script();
        let intent = issuer.intent(&worker, target_gen, &fixture.cancel).unwrap();
        let permit = intent.script(SCRIPT, 30).unwrap();
        let consumed = permit.consume(generation, epoch, script, timeout);
        assert_eq!(consumed.is_ok(), accepted);
        if let Ok(authority) = consumed {
            assert!(authority.names(ACTIVE, START_EPOCH, SCRIPT, 30));
            assert!(!authority.names(ACTIVE, START_EPOCH, SCRIPT, 31));
        }
    }
    // A finished script cannot mint, though its VM, generation and writer
    // epoch all stay live: only the operation's own status differs.
    let fixture = Fixture::new();
    let issuer = fixture.issuer();
    let (worker, target_gen) = fixture.script();
    fixture
        .ctl
        .finish_script(&worker, ScriptResult::Exited(0))
        .unwrap();
    assert!(issuer.intent(&worker, target_gen, &fixture.cancel).is_err());

    // A writer epoch the stop path already released cannot run a script.
    let fixture = Fixture::new();
    let issuer = fixture.issuer();
    let (worker, target_gen) = fixture.script();
    let intent = issuer.intent(&worker, target_gen, &fixture.cancel).unwrap();
    let permit = intent.script(SCRIPT, 30).unwrap();
    fixture.sql(|db| {
        db.execute(
            "UPDATE vm_snapshot_release SET disposition='dirty' WHERE generation=?1",
            [ACTIVE],
        )?;
        Ok(())
    });
    assert!(permit.consume(ACTIVE, START_EPOCH, SCRIPT, 30).is_err());
    assert_admission_closed();
}

/// A stop admission carries no native effect, though its operation is
/// pending against the running generation and epoch.
#[test]
fn stop_admission_mints_no_intent() {
    let fixture = Fixture::new();
    let issuer = fixture.issuer();
    fixture.instance(2);
    let worker = fixture.admit(7, Operation::Stop);
    let target = NativeTarget {
        generation: ACTIVE,
        writer_epoch: Some(START_EPOCH),
    };
    assert!(issuer.intent(&worker, target, &fixture.cancel).is_err());
    assert_admission_closed();
}

/// Review 6: recovery needs exact durable custody and, for a generation
/// with writer history, its exact unresolved epoch. Only a staged
/// generation that never had a writer recovers without one. Selection
/// revocation does not strand it, and it carries no launch authority.
#[test]
fn recovery_authority_needs_exact_custody_not_selection() {
    let fixture = Fixture::new();
    let issuer = fixture.issuer();
    fixture.script();
    let scope = workspace(ACTIVE);
    let installation = Installation::fixture();
    let mint = |scope, epoch| RecoveryAuthority::mint(&installation, &fixture.ctl, scope, epoch);
    // Without a bound native workspace there is no recovery authority.
    assert!(mint(scope, Some(START_EPOCH)).is_err());
    fixture.bind(scope, "clone");
    *fixture.source.marker.lock().unwrap() = [8; 32];
    assert!(issuer.catalog().is_err());
    assert!(issuer.epoch().effects_revoked());
    let authority = mint(scope, Some(START_EPOCH)).unwrap();
    assert_eq!(authority.scope(), scope);
    assert_eq!(authority.writer_epoch(), Some(START_EPOCH));
    let mut moved = scope;
    moved.lease.file = [9; 16];
    assert!(mint(moved, Some(START_EPOCH)).is_err());
    assert!(mint(scope, Some([46; 16])).is_err());
    assert!(mint(scope, None).is_err());
    // After the epoch is released, neither it nor no epoch recovers.
    fixture.sql(|db| {
        db.execute(
            "UPDATE vm_snapshot_release SET disposition='dirty' WHERE generation=?1",
            [ACTIVE],
        )?;
        Ok(())
    });
    assert!(mint(scope, None).is_err());
    assert!(mint(scope, Some(START_EPOCH)).is_err());

    // A staged build generation that never had a writer recovers without
    // an epoch, and refuses a made-up one.
    let fixture = Fixture::new();
    let (slots, _worker, _) = fixture.build(1);
    let staged = workspace(slots.pair.disk.generation);
    fixture.bind(staged, "build");
    let mint = |epoch| RecoveryAuthority::mint(&installation, &fixture.ctl, staged, epoch);
    assert!(mint(None).is_ok());
    assert!(mint(Some([46; 16])).is_err());
    // An untouched reservation has no writer epoch to name.
    assert!(mint(Some(staged.generation)).is_err());

    // Review 2: the checked creation edge records staged writer history, so
    // epochless recovery refuses and only the generation's own epoch, with
    // its recorded origin, recovers.
    let origin = crate::store::vm::native::recovery::Origin {
        boot: [5; 32],
        database: FileIdentity {
            volume: 1,
            file: [70; 16],
        },
        lock: FileIdentity {
            volume: 1,
            file: [71; 16],
        },
    };
    let durable = Scope {
        generation: staged.generation,
        production: staged.production,
        workspace: staged.workspace,
        lease: staged.lease,
    };
    fixture.sql(|db| {
        crate::store::vm::native::recovery::record_job(db, durable, staged.generation, origin)
    });
    assert!(
        mint(None).is_err(),
        "epochless recovery after a staged checked-create record"
    );
    let authority = mint(Some(staged.generation)).unwrap();
    assert_eq!(authority.writer_epoch(), Some(staged.generation));
    assert!(mint(Some([46; 16])).is_err());

    // A missing reservation row is not proof of an untouched reservation.
    let fixture = Fixture::new();
    let (slots, _worker, _) = fixture.build(1);
    let staged = workspace(slots.pair.disk.generation);
    fixture.bind(staged, "build");
    fixture.sql(|db| {
        db.execute(
            "DELETE FROM vm_native_writer WHERE generation=?1",
            [staged.generation],
        )?;
        Ok(())
    });
    let mint = |epoch| RecoveryAuthority::mint(&installation, &fixture.ctl, staged, epoch);
    assert!(mint(None).is_err());
    assert!(mint(Some(staged.generation)).is_err());
    assert_admission_closed();
}

#[test]
fn launch_slots_come_from_the_target() {
    let start = AdmittedEffect::Start;
    let clone = AdmittedEffect::Clone {
        template: TEMPLATE,
        golden: ACTIVE,
    };
    let build = AdmittedEffect::Build {
        recipe_id: recipes().ubuntu.recipe_id,
        digest: recipes().ubuntu.recipe_sha256,
    };
    let helper = pair().disk;
    let mut overlay = pair();
    overlay.disk.role = "verification_overlay";
    let mut seed = helper;
    seed.role = "seed";
    let mut swapped = pair();
    swapped.disk.role = "vars";
    swapped.vars.role = "disk";
    let mut seed_pair = pair();
    seed_pair.disk.role = "seed";
    let mut same_key = pair();
    same_key.vars.key = same_key.disk.key;
    let ok = |effect, kind, pair, helper| launch_slots(effect, kind, pair, helper).ok();
    assert_eq!(
        ok(start, PlanKind::Boot, Some(pair()), None),
        Some(vec![pair().disk, pair().vars])
    );
    assert_eq!(
        ok(build, PlanKind::Boot, Some(overlay), None),
        Some(vec![overlay.disk, overlay.vars])
    );
    assert_eq!(
        ok(
            build,
            PlanKind::InstallerBoot(InstallerPhase::WindowsSetup),
            Some(pair()),
            None
        ),
        Some(vec![pair().disk, pair().vars])
    );
    assert_eq!(
        ok(clone, PlanKind::Mutate, None, Some(helper)),
        Some(vec![helper])
    );
    assert_eq!(
        ok(build, PlanKind::Mutate, None, Some(overlay.disk)),
        Some(vec![overlay.disk])
    );
    assert_eq!(ok(build, PlanKind::Inspect, None, None), Some(vec![]));
    assert_eq!(
        ok(build, PlanKind::InspectWritable, None, None),
        Some(vec![])
    );
    for (effect, kind, pair, helper) in [
        (start, PlanKind::Boot, None, None),
        (start, PlanKind::Boot, Some(pair()), Some(helper)),
        (start, PlanKind::Boot, Some(overlay), None),
        (start, PlanKind::Boot, Some(swapped), None),
        (start, PlanKind::Boot, Some(seed_pair), None),
        (start, PlanKind::Boot, Some(same_key), None),
        (
            build,
            PlanKind::InstallerBoot(InstallerPhase::WindowsSetup),
            Some(overlay),
            None,
        ),
        (
            build,
            PlanKind::InstallerBoot(InstallerPhase::WindowsSetup),
            Some(swapped),
            None,
        ),
        (build, PlanKind::Mutate, None, None),
        (build, PlanKind::Mutate, Some(pair()), Some(helper)),
        (build, PlanKind::Mutate, None, Some(seed)),
        (clone, PlanKind::Mutate, None, Some(overlay.disk)),
        (build, PlanKind::Inspect, Some(pair()), None),
        (build, PlanKind::Inspect, None, Some(helper)),
        (start, PlanKind::StaticWebBoot, Some(pair()), None),
        (start, PlanKind::PublicEgressBoot, Some(pair()), None),
    ] {
        assert_eq!(ok(effect, kind, pair, helper), None, "{effect:?} {kind:?}");
    }
}

/// Build verification boots accept `verification_overlay`, and installer boots
/// require their matching live `InstallerStage` at mint and consumption.
#[test]
fn build_verification_boot_and_installer_stage_are_bound() {
    let fixture = Fixture::new();
    let issuer = fixture.issuer();
    let (slots, worker, _) = fixture.build(1);
    let verify_target = NativeTarget {
        generation: slots.verification.disk.generation,
        writer_epoch: None,
    };
    let intent = issuer
        .intent(&worker, verify_target, &fixture.cancel)
        .unwrap();
    let ws = workspace(slots.verification.disk.generation);
    let inputs = BootInputs::new();
    let recipe = inputs.recipe();
    let plan = recipe.plan().unwrap();
    let cwd = LocalPath::parse("D:/vms/work").unwrap();
    let epoch = slots.verification.disk.generation;
    let boot = || LaunchTarget {
        pair: Some(slots.verification),
        recipe: Some(recipes().ubuntu),
        ..target(&plan, &cwd, epoch)
    };
    let permit = intent.launch(ws, boot()).unwrap();
    // Second boot permit on the same verification pair is refused.
    assert!(intent.launch(ws, boot()).is_err());
    let authority = permit.consume(boot()).unwrap();
    assert_eq!(authority.binding().kind, PlanKind::Boot);

    // Installer stage checks: matching live stage passes; missing, mismatched
    // phase, or expired deadline is refused.
    let live =
        crate::vm::runtime::InstallerBudget::start(crate::vm::runtime::InstallerRecipe::Ubuntu)
            .stage(InstallerPhase::UbuntuInstall, Duration::from_secs(60))
            .unwrap();
    let wrong_phase =
        crate::vm::runtime::InstallerBudget::start(crate::vm::runtime::InstallerRecipe::Windows)
            .stage(InstallerPhase::WindowsSetup, Duration::from_secs(60))
            .unwrap();
    let expired =
        crate::vm::runtime::InstallerBudget::start(crate::vm::runtime::InstallerRecipe::Ubuntu)
            .stage(InstallerPhase::UbuntuInstall, Duration::ZERO)
            .unwrap();
    assert!(
        check_installer(
            PlanKind::InstallerBoot(InstallerPhase::UbuntuInstall),
            Some((live.phase(), live.deadline())),
        )
        .is_ok()
    );
    assert!(check_installer(PlanKind::InstallerBoot(InstallerPhase::UbuntuInstall), None).is_err());
    assert!(
        check_installer(
            PlanKind::InstallerBoot(InstallerPhase::UbuntuInstall),
            Some((wrong_phase.phase(), wrong_phase.deadline())),
        )
        .is_err()
    );
    assert!(
        check_installer(
            PlanKind::InstallerBoot(InstallerPhase::UbuntuInstall),
            Some((expired.phase(), expired.deadline())),
        )
        .is_err()
    );
    assert!(check_installer(PlanKind::Boot, Some((live.phase(), live.deadline()))).is_err());
    assert_admission_closed();
}
