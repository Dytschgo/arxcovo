//! Matched positives run the production issuer bodies against a real ledger
//! with a fake held selection. Every negative has its positive control.

use super::*;
use crate::{
    service::application::tests::private_temp,
    store::vm as store,
    vm::{
        Admission, BuildAdmission, BuildRequest, BuildSlots, Capacity, DiskClaim, HostObservation,
        Operation, OperationRef, Request, Resources, VmPolicy,
        paths::Artifact,
        qualification_record::{Instant2, Selection},
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
const GIB: u64 = 1 << 30;
const VM: Id = [41; 16];
const TEMPLATE: Id = [42; 16];
const ACTIVE: Id = [43; 16];
const START_EPOCH: Id = [44; 16];

#[derive(Clone)]
struct TestClock(Arc<Mutex<ClockReading>>);

impl Clock for TestClock {
    fn now(&self) -> ClockReading {
        *self.0.lock().unwrap()
    }
}

fn recipes() -> TestRecipes {
    TestRecipes {
        ubuntu: RecipeIdentity {
            recipe_id: [0x11; 16],
            recipe_sha256: [0x12; 32],
        },
        windows: RecipeIdentity {
            recipe_id: VmController::<TestClock>::windows_recipe_id(),
            recipe_sha256: [0x22; 32],
        },
    }
}

fn config() -> TestQualificationSelection {
    TestQualificationSelection {
        enrollment: LocalPath::parse("D:/private/enrollment.json").unwrap(),
        dossier: LocalPath::parse("D:/private/dossier").unwrap(),
        selection: Selection {
            seal_sha256: SEAL,
            scope: SCOPE,
        },
    }
}

/// The held selection, with a scripted refusal and boot marker.
struct FakeSource {
    seal: Hash,
    class: Class,
    scope: Hash,
    recipes: TestRecipes,
    origin: Instant,
    marker: Mutex<Hash>,
    fault: Mutex<Option<Invalidation>>,
}

impl FakeSource {
    fn new(origin: Instant) -> Self {
        Self {
            seal: SEAL,
            class: Class::Test,
            scope: SCOPE,
            recipes: recipes(),
            origin,
            marker: Mutex::new(MARKER),
            fault: Mutex::default(),
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

    fn sample(
        &self,
        advance: &dyn Fn() -> Result<ClockReading, Invalidation>,
    ) -> Result<Sample, Invalidation> {
        let captured = Instant::now();
        if let Some(fault) = *self.fault.lock().unwrap() {
            return Err(fault);
        }
        let reading = advance()?;
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
    _root: tempfile::TempDir,
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
            _root: root,
        }
    }

    fn issuer(&self) -> TestIssuer<'_, &FakeSource, TestClock> {
        TestIssuer::open(&Installation::fixture(), &self.ctl, &self.source, &config()).unwrap()
    }

    /// Both host clocks move together.
    fn advance(&self, seconds: u64) {
        let mut reading = self.clock.0.lock().unwrap();
        reading.utc_ms += i64::try_from(seconds * 1_000).unwrap();
        reading.monotonic += Duration::from_secs(seconds);
    }

    fn fault(&self, fault: Option<Invalidation>) {
        *self.source.fault.lock().unwrap() = fault;
    }

    fn build(&self, number: u8) -> (BuildSlots, PendingOperation, OperationRef) {
        let BuildAdmission::Fresh(slots, worker, receipt) = self
            .ctl
            .admit_build(build_request(number), host(), resources(), |_, _| None)
            .unwrap()
            .unwrap()
        else {
            panic!("fresh build required");
        };
        (*slots, worker, receipt)
    }

    /// A running TEST VM with an unresolved start writer epoch, and an
    /// admitted script on it.
    fn script(&self) -> (PendingOperation, Id) {
        self.ctl
            .store_for_test()
            .transact(|db| {
                db.execute(
                    "INSERT INTO vm_templates(id,guest,recipe_digest,disk_digest,vars_digest,bytes)
                     VALUES(?1,1,?2,?2,?2,1)",
                    params![TEMPLATE, [1u8; 32]],
                )?;
                db.execute(
                    "INSERT INTO vm_instances(id,template,principal,expires_ms,state)
                     VALUES(?1,?2,?3,?4,2)",
                    params![VM, TEMPLATE, OWNER, NOW + 3_600_000],
                )?;
                db.execute(
                    "INSERT INTO vm_generations(id,vm_id,generation,active) VALUES(?1,?2,1,1)",
                    params![ACTIVE, VM],
                )?;
                store::snapshot::invalidate_release(db, VM, START_EPOCH)
            })
            .unwrap();
        let request = Request {
            principal: OWNER,
            request_id: key(9),
            vm: VM,
            operation: Operation::Script {
                script: "Get-Date",
                timeout_seconds: 30,
            },
            disk: DiskClaim {
                staging: 0,
                images: 0,
                metadata: 0,
            },
        };
        let Admission::Admitted(worker) = self.ctl.admit(request, host(), resources()).unwrap()
        else {
            panic!("fresh script required");
        };
        let operation = self.operation(&worker);
        (worker, operation)
    }

    fn operation(&self, worker: &PendingOperation) -> Id {
        self.ctl
            .store_for_test()
            .transact(|db| {
                Ok(db.query_row(
                    "SELECT operation_id FROM vm_operations WHERE activity_id=?1",
                    [worker.activity_id],
                    |row| row.get(0),
                )?)
            })
            .unwrap()
    }
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
        recipe: "e03a3012c18d20b8691490b580ab41e9",
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

fn build_effect(slots: &BuildSlots, receipt: &OperationRef, number: u8) -> NativeEffect<'static> {
    NativeEffect {
        principal: OWNER,
        request: key(number),
        operation: receipt.id,
        action: crate::vm::BUILD_ACTION,
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

/// An unbacked qcow2 v3 header for the Info plan builder.
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

fn target<'a>(plan: &'a CommandPlan<'a>, cwd: &'a LocalPath) -> LaunchTarget<'a, 'a> {
    LaunchTarget {
        plan,
        cwd,
        pair: None,
        installer: None,
        recipe: None,
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
fn assert_admission_closed() {
    let mut profile = serde_json::json!({
        "enabled": true, "owner_sid": "S-1-12-1-1-2-3-4",
        "supervisor_sid": "S-1-12-1-1-2-3-4", "bundle": "E:/pinned bundle",
        "volume": "D:/", "state": "D:/test_vms/state",
        "disk_ceiling_bytes": 10_737_418_240_u64, "manifest": "qemu-11.1.3-windows-x64",
        "execution_profile": "current_user",
    });
    profile["test_qualification"] = serde_json::json!({
        "enrollment": "D:/private/enrollment.json", "dossier": "D:/private/dossier",
        "sha256": hex::encode(SEAL), "scope": hex::encode(SCOPE),
    });
    let profile =
        crate::config::VmRuntimeProfile::parse(&serde_json::to_vec(&profile).unwrap()).unwrap();
    assert!(profile.test_qualification().is_some());
    let runtime = CandidateRuntime::new(Some(&profile));
    assert!(matches!(
        runtime.capability(),
        NativeCapability::Unavailable(NativeRefusal::QualificationMissing)
    ));
    assert!(runtime.dispatch(NativeMutation::Boot, |_| ()).is_err());
}

/// T01/T02/I01: the held seal, scope, class and recipe pair must match the
/// configured selection exactly.
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
    let mutations: [(&str, Mutate); 8] = [
        ("seal", |s, _| s.seal = [0xa2; 32]),
        ("scope", |s, _| s.scope = [0xb3; 32]),
        ("static class", |s, _| s.class = Class::StaticOrigin),
        ("docker class", |s, _| s.class = Class::Docker),
        ("windows recipe", |s, _| {
            s.recipes.windows.recipe_id = [0x33; 16]
        }),
        ("ubuntu as windows", |s, w| s.recipes.ubuntu.recipe_id = w),
        ("ubuntu as web", |s, _| {
            s.recipes.ubuntu.recipe_id = WEB_RECIPE_ID
        }),
        ("unset ubuntu", |s, _| s.recipes.ubuntu.recipe_id = [0; 16]),
    ];
    for (name, mutate) in mutations {
        let mut source = FakeSource::new(fixture.source.origin);
        mutate(&mut source, windows);
        assert_eq!(open(&source), Some(REFUSED), "{name}");
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
        LocalPath::parse(&path.to_str().unwrap().replace('\\', "/")).unwrap()
    };
    let configured = TestQualificationSelection {
        enrollment: path("enrollment.json"),
        dossier: path("dossier"),
        ..config()
    };
    assert!(
        TestIssuer::open_configured(&Installation::fixture(), &fixture.ctl, &configured).is_err()
    );
    assert!(
        TestIssuer::open(
            &Installation::fixture(),
            &fixture.ctl,
            &fixture.source,
            &configured
        )
        .is_ok()
    );
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
                {"guest": "ubuntu", "recipe_id": "11".repeat(16), "recipe_sha256": "12".repeat(32)},
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

/// I05/T07/T08: an intent needs the worker's exact pending admission. A
/// replay gets no worker, a worker mints once, and a finished operation's
/// worker mints nothing.
#[test]
fn intent_follows_durable_admission_and_replay_mints_nothing() {
    let fixture = Fixture::new();
    let issuer = fixture.issuer();
    let (slots, worker, receipt) = fixture.build(1);
    let exact = build_effect(&slots, &receipt, 1);
    let cancel = &fixture.cancel;
    for (name, effect) in [
        (
            "principal",
            NativeEffect {
                principal: "bob",
                ..exact
            },
        ),
        (
            "request",
            NativeEffect {
                request: key(2),
                ..exact
            },
        ),
        (
            "operation",
            NativeEffect {
                operation: [1; 16],
                ..exact
            },
        ),
        ("action", NativeEffect { action: 2, ..exact }),
        (
            "generation",
            NativeEffect {
                generation: [9; 16],
                ..exact
            },
        ),
        (
            "writer epoch",
            NativeEffect {
                writer_epoch: Some(receipt.id),
                ..exact
            },
        ),
    ] {
        assert!(issuer.intent(&worker, effect, cancel).is_err(), "{name}");
    }
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

    // A finished operation's worker mints nothing, and neither does a
    // cancelled one; each has its own matched positive.
    for (name, finish, cancelled) in [
        ("exact", false, false),
        ("finished", true, false),
        ("cancelled", false, true),
    ] {
        let fixture = Fixture::new();
        let issuer = fixture.issuer();
        let (slots, worker, receipt) = fixture.build(1);
        if finish {
            fixture.ctl.fail_build(&worker, slots, false).unwrap();
        }
        if cancelled {
            fixture.cancel.cancel();
        }
        let minted = issuer.intent(&worker, build_effect(&slots, &receipt, 1), &fixture.cancel);
        assert_eq!(minted.is_ok(), !finish && !cancelled, "{name}");
    }
    assert_admission_closed();
}

/// I06/T11/T12: a launch permit names one exact plan, and consumption
/// refuses a substituted input, a stale epoch, cancellation, a finished
/// operation or an expired worker.
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
        let (slots, worker, receipt) = fixture.build(1);
        let effect = build_effect(&slots, &receipt, 1);
        let intent = issuer.intent(&worker, effect, &fixture.cancel).unwrap();
        let ws = workspace(effect.generation);
        let permit = intent.launch(ws, target(&plan, &cwd), &[]).unwrap();
        // One permit per durable effect.
        assert!(
            intent.launch(ws, target(&plan, &cwd), &[]).is_err(),
            "{name}"
        );
        if let Some(drift) = drift {
            drift(&fixture, &slots, &worker);
            assert!(permit.consume(target(&plan, &cwd)).is_err(), "{name}");
        } else {
            let (binding, _authority) = permit.consume(target(&plan, &cwd)).unwrap();
            assert_eq!(
                binding,
                LaunchPermitBinding {
                    workspace: ws,
                    seal_sha256: SEAL,
                    kind: PlanKind::Inspect,
                }
            );
        }
    }

    let fixture = Fixture::new();
    let issuer = fixture.issuer();
    let (slots, worker, receipt) = fixture.build(1);
    let effect = build_effect(&slots, &receipt, 1);
    let intent = issuer.intent(&worker, effect, &fixture.cancel).unwrap();
    let ws = workspace(effect.generation);
    let wrong_ws = workspace(slots.verification.disk.generation);
    assert!(intent.launch(wrong_ws, target(&plan, &cwd), &[]).is_err());
    let foreign = RecipeIdentity {
        recipe_id: [0x55; 16],
        recipe_sha256: [0x56; 32],
    };
    let unselected_target = LaunchTarget {
        recipe: Some(foreign),
        ..target(&plan, &cwd)
    };
    assert!(intent.launch(ws, unselected_target, &[]).is_err());
    let permit = intent.launch(ws, target(&plan, &cwd), &[]).unwrap();
    assert!(permit.consume(target(&other, &cwd)).is_err());
    assert_admission_closed();
}

/// T11/T12/T19: a staged write names its slot, content, recipe phase and
/// deadline. The ledger refuses an altered role, and consumption refuses
/// substituted content or a stale epoch.
#[test]
fn stage_permit_binds_slot_content_phase_and_deadline() {
    let setup = |fixture: &Fixture| {
        let (slots, worker, receipt) = fixture.build(1);
        let effect = NativeEffect {
            generation: slots.seed.generation,
            ..build_effect(&slots, &receipt, 1)
        };
        (slots, worker, effect)
    };
    let fixture = Fixture::new();
    let issuer = fixture.issuer();
    let (slots, worker, effect) = setup(&fixture);
    let intent = issuer.intent(&worker, effect, &fixture.cancel).unwrap();
    let exact = stage_target(slots.seed);
    let mut refused = Vec::new();
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
    refused.push(("recipe", recipe));
    let mut expired = exact;
    expired.deadline = Instant::now();
    refused.push(("deadline", expired));
    for (name, target) in refused {
        assert!(intent.stage(target).is_err(), "{name}");
    }
    let permit = intent.stage(exact).unwrap();
    assert!(intent.stage(exact).is_err());
    let mut content = exact;
    content.content.sha256 = digest(b"swap");
    assert!(permit.consume(content).is_err());

    // Exact consumption passes; a transient refusal at the write refuses.
    for fault in [None, Some(Transient(REFUSED))] {
        let fixture = Fixture::new();
        let issuer = fixture.issuer();
        let (slots, worker, effect) = setup(&fixture);
        let intent = issuer.intent(&worker, effect, &fixture.cancel).unwrap();
        let exact = stage_target(slots.seed);
        let permit = intent.stage(exact).unwrap();
        fixture.fault(fault);
        assert_eq!(permit.consume(exact).is_ok(), fault.is_none());
        assert!(!issuer.epoch().effects_revoked());
    }
    assert_admission_closed();
}

/// I06/T11: a script permit binds the exact bytes, timeout, generation and
/// unresolved writer epoch, and refreshes at channel dispatch.
#[test]
fn script_permit_binds_bytes_timeout_and_writer_epoch() {
    let fixture = Fixture::new();
    let issuer = fixture.issuer();
    let (worker, operation) = fixture.script();
    let effect = NativeEffect {
        principal: OWNER,
        request: key(9),
        operation,
        action: 2,
        generation: ACTIVE,
        writer_epoch: Some(START_EPOCH),
    };
    assert!(
        issuer
            .intent(
                &worker,
                NativeEffect {
                    writer_epoch: Some(operation),
                    ..effect
                },
                &fixture.cancel
            )
            .is_err()
    );
    assert!(
        issuer
            .intent(
                &worker,
                NativeEffect {
                    writer_epoch: None,
                    ..effect
                },
                &fixture.cancel
            )
            .is_err()
    );
    let intent = issuer.intent(&worker, effect, &fixture.cancel).unwrap();
    let permit = intent.script("Get-Date", 30).unwrap();
    assert!(intent.script("Get-Date", 30).is_err());
    assert!(
        permit
            .consume(ACTIVE, START_EPOCH, "Get-Date ", 30)
            .is_err()
    );
    // (generation, writer epoch is the start's, script, timeout, accepted)
    for (generation, start_epoch, script, timeout, accepted) in [
        (ACTIVE, true, "Get-Date", 31, false),
        (ACTIVE, false, "Get-Date", 30, false),
        ([45; 16], true, "Get-Date", 30, false),
        (ACTIVE, true, "Get-Date", 30, true),
    ] {
        let fixture = Fixture::new();
        let issuer = fixture.issuer();
        let (worker, operation) = fixture.script();
        let effect = NativeEffect {
            operation,
            ..effect
        };
        let intent = issuer.intent(&worker, effect, &fixture.cancel).unwrap();
        let permit = intent.script("Get-Date", 30).unwrap();
        let epoch = if start_epoch { START_EPOCH } else { operation };
        let consumed = permit.consume(generation, epoch, script, timeout);
        assert_eq!(consumed.is_ok(), accepted);
    }
    // A finished script cannot mint, though its VM, generation and writer
    // epoch all stay live: only the operation's own status differs.
    let fixture = Fixture::new();
    let issuer = fixture.issuer();
    let (worker, operation) = fixture.script();
    fixture
        .ctl
        .finish_script(&worker, crate::vm::ScriptResult::Exited(0))
        .unwrap();
    assert!(
        issuer
            .intent(
                &worker,
                NativeEffect {
                    operation,
                    ..effect
                },
                &fixture.cancel
            )
            .is_err()
    );

    // A writer epoch the stop path already released cannot run a script.
    let fixture = Fixture::new();
    let issuer = fixture.issuer();
    let (worker, operation) = fixture.script();
    let intent = issuer
        .intent(
            &worker,
            NativeEffect {
                operation,
                ..effect
            },
            &fixture.cancel,
        )
        .unwrap();
    let permit = intent.script("Get-Date", 30).unwrap();
    fixture
        .ctl
        .store_for_test()
        .transact(|db| {
            db.execute(
                "UPDATE vm_snapshot_release SET disposition='dirty' WHERE generation=?1",
                [ACTIVE],
            )?;
            Ok(())
        })
        .unwrap();
    assert!(permit.consume(ACTIVE, START_EPOCH, "Get-Date", 30).is_err());
    assert_admission_closed();
}

/// Recovery needs exact durable custody and its writer epoch, not a fresh
/// selection, and carries no launch authority.
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
    fixture
        .ctl
        .store_for_test()
        .transact(|db| {
            // Native custody is reserved while the generation is staged.
            db.execute("UPDATE vm_generations SET active=0 WHERE id=?1", [ACTIVE])?;
            db.execute(
                "INSERT INTO vm_native_generations
                 (generation,host,origin,protocol,volume,production,state,directory,lease)
                 VALUES(?1,(SELECT id FROM vm_state_host WHERE singleton=1),'clone',1,?2,?3,
                        'bound',?4,?5)",
                params![
                    ACTIVE,
                    1u64.to_be_bytes(),
                    scope.production.file,
                    scope.workspace.file,
                    scope.lease.file
                ],
            )?;
            db.execute("UPDATE vm_generations SET active=1 WHERE id=?1", [ACTIVE])?;
            Ok(())
        })
        .unwrap();
    // Selection revocation does not strand recovery.
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
    assert_admission_closed();
}
