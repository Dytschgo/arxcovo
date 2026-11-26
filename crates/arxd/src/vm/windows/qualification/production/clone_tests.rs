//! Linked clone staging through real NTFS custody, the VM ledger, the
//! production issuer bodies and the shared disposal route (7c 11: I05–I10,
//! I15, I18, I20, I24, T09–T12, T19–T22, T24–T27, T35). A fixture stands in
//! for the qemu-img child only; every other edge is the production code.

use super::super::disposal::tests::{CLAIM, NOW, SweepFixture, build_request, exists};
use super::super::helper::{HELPER_BUDGET, check_helper_lease, helper_deadline};
use super::super::tests::unbacked_qcow2_header;
use super::*;
use crate::service::Shutdown;
use crate::store::vm::{
    self as store, Binding,
    native::{Leaf, Origin, disposal::GenerationState},
};
use crate::vm::reaper::CleanupWork;
use crate::vm::runtime::{OfflineLease, Quiesced, ReaperRuntime};
use crate::vm::test_issuer::tests::{FakeSource, assert_admission_closed, config};
use crate::vm::test_issuer::{Installation, Intent, NativeTarget, TestIssuer};
use crate::vm::{
    BuildRequest, Capacity, CloneAdmission, DiskClaim, Guest, HostObservation, Operation,
    OperationRef, PendingOperation, Refusal, Request, RequestId, Resources, Subject,
};
use std::cell::Cell;

type FxClock = super::super::disposal::tests::FakeClock;
type Issuer<'f> = TestIssuer<'f, &'f FakeSource, FxClock>;

const SIZE: u64 = 1 << 30;
const TTL: u64 = 3_600;
const GOLDEN_VARS: &[u8] = b"sealed golden vars";

#[derive(Clone, Copy)]
struct Golden {
    template: Id,
    scope: WorkspaceBinding,
    slots: PairSlots,
    evidence: [ArtifactEvidence; 2],
}

/// A sealed native golden through the real build ledger and transient
/// disposal: a valid unbacked qcow2 disk and private VARS.
fn seal_golden(fx: &SweepFixture, nonce: u8) -> Golden {
    let claim = DiskClaim {
        images: 1 << 20,
        metadata: 0,
        staging: 0,
    };
    let request = BuildRequest {
        retained: claim,
        transient: claim,
        ..build_request(nonce)
    };
    let (activity, slots) = fx
        .store()
        .transact(|db| {
            let operation = Binding {
                principal: request.principal,
                key: request.key,
                subject: Subject::Recipe(request.recipe, Some(request.digest)),
                action: 11,
            }
            .record(db, NOW, None, None)?;
            let activity = operation.receipt.activity_id;
            Ok((
                activity,
                store::reserve_build(db, request, operation, 2 << 20)?,
            ))
        })
        .unwrap();
    let ledger = LedgerCheckpoints {
        store: fx.store(),
        origin: Origin::Build,
    };
    let owner = fx.owner_sid().0;
    let primary =
        reserve_workspace_under(&fx.production, owner, slots.pair.disk.generation, &ledger)
            .unwrap();
    let disk = fx.stage_slot(
        &primary,
        &ledger,
        slots.pair.disk,
        &unbacked_qcow2_header(SIZE),
    );
    let vars = fx.stage_slot(&primary, &ledger, slots.pair.vars, GOLDEN_VARS);
    fx.stage_slot(&primary, &ledger, slots.seed, b"seed");
    drop(primary);
    let verify = reserve_workspace_under(
        &fx.production,
        owner,
        slots.verification.disk.generation,
        &ledger,
    )
    .unwrap();
    fx.stage_slot(&verify, &ledger, slots.verification.disk, b"verify-overlay");
    fx.stage_slot(&verify, &ledger, slots.verification.vars, b"verify-vars");
    drop(verify);
    let guards = GenerationGuards::default();
    fx.disposal(&LedgerDisposal { store: fx.store() }, &guards)
        .dispose_build_transient(activity, slots, Instant::now() + Duration::from_secs(30))
        .unwrap();
    fx.store()
        .transact(|db| {
            store::publish_build(
                db,
                activity,
                slots,
                request,
                Guest::UbuntuX86_64,
                [disk, vars],
                NOW,
            )
        })
        .unwrap();
    let generation = slots.pair.disk.generation;
    let (template, state) = fx
        .store()
        .transact(|db| {
            let template = db.query_row(
                "SELECT template_id FROM vm_build_intents WHERE id=?1",
                [slots.id],
                |row| row.get(0),
            )?;
            Ok((
                template,
                store::native::disposal::generation_state(db, generation)?,
            ))
        })
        .unwrap();
    let GenerationState::Bound { scope, .. } = state else {
        panic!("sealed golden keeps its bound native custody");
    };
    Golden {
        template,
        scope: super::super::disposal::binding_from_scope(scope),
        slots: slots.pair,
        evidence: [disk, vars],
    }
}

/// A request key stamped with the fixture's UTC time.
fn fresh_key(number: u32) -> RequestId {
    RequestId::parse(&format!("{NOW:016x}{number:016x}")).unwrap()
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

fn clone_request(golden: &Golden, key: u16, ttl: u64) -> Request<'static> {
    Request {
        principal: "alice",
        request_id: fresh_key(0x9000 + u32::from(key)),
        vm: [0; 16],
        operation: Operation::Clone {
            template: golden.template,
            lease_seconds: Some(ttl),
        },
        disk: DiskClaim {
            images: CLAIM,
            metadata: 0,
            staging: 0,
        },
    }
}

struct Admitted {
    vm: Id,
    slots: PairSlots,
    worker: PendingOperation,
    operation: OperationRef,
}

fn admit(fx: &SweepFixture, golden: &Golden, key: u16, ttl: u64) -> Admitted {
    match fx
        .ctl()
        .admit_linked_clone(
            clone_request(golden, key, ttl),
            host(),
            resources(),
            golden.slots,
            golden.evidence,
        )
        .unwrap()
        .unwrap()
    {
        CloneAdmission::Fresh(vm, slots, worker, operation) => Admitted {
            vm,
            slots,
            worker,
            operation,
        },
        CloneAdmission::Existing(_) => panic!("fresh clone admission required"),
    }
}

/// I05/T07: a same-request replay returns the first receipt and no worker.
fn replay(fx: &SweepFixture, golden: &Golden, key: u16, ttl: u64) -> OperationRef {
    match fx
        .ctl()
        .admit_linked_clone(
            clone_request(golden, key, ttl),
            host(),
            resources(),
            golden.slots,
            golden.evidence,
        )
        .unwrap()
        .unwrap()
    {
        CloneAdmission::Existing(operation) => operation,
        CloneAdmission::Fresh(..) => panic!("a replay must not admit fresh work"),
    }
}

fn hold(fx: &SweepFixture, golden: &Golden) -> Result<HeldGolden, Q> {
    let ledger = LedgerCheckpoints {
        store: fx.store(),
        origin: Origin::Clone,
    };
    HeldGolden::hold(
        &fx.production,
        fx.owner_sid().0,
        &ledger,
        golden.scope,
        golden.slots,
        golden.evidence,
        Instant::now() + Duration::from_secs(30),
        &|| false,
    )
}

/// An unbacked header plus the qcow2 backing-format extension and the
/// backing name, as the pinned qemu-img create writes it.
fn backed_header(size: u64, backing: &str) -> Vec<u8> {
    let mut cluster = unbacked_qcow2_header(size);
    cluster[104..108].copy_from_slice(&0xe279_2acau32.to_be_bytes());
    cluster[108..112].copy_from_slice(&5u32.to_be_bytes());
    cluster[112..117].copy_from_slice(b"qcow2");
    let name = backing.as_bytes();
    cluster[8..16].copy_from_slice(&128u64.to_be_bytes());
    cluster[16..20].copy_from_slice(&(name.len() as u32).to_be_bytes());
    cluster[128..128 + name.len()].copy_from_slice(name);
    cluster
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Mode {
    Clean,
    /// The helper ran and released, but exited non-zero.
    ExitNonzero,
    /// Output, but the release was never proven.
    NoRelease,
    /// A release proof for other inputs than the planned ones.
    WrongInputs,
    /// Cancellation lands after a valid release sample.
    CancelAfterRelease,
    /// The written overlay names another backing file.
    WrongBacking,
    /// The checked creation consumes a plan with a substituted backing.
    SubstitutedBacking,
}

/// The real issuer for the fixture's controller and the admitted worker.
struct Run<'f> {
    image: Artifact,
    cwd: LocalPath,
    _dir: tempfile::TempDir,
    issuer: Issuer<'f>,
    cancel: Shutdown,
}

impl<'f> Run<'f> {
    fn new(fx: &'f SweepFixture, source: &'f FakeSource) -> Self {
        let dir = tempfile::tempdir_in(std::env::temp_dir()).unwrap();
        let file = dir.path().join("qemu-img.exe");
        std::fs::write(&file, b"pinned qemu-img").unwrap();
        let path = LocalPath::parse(file.to_str().unwrap()).unwrap();
        let pin = PathPin::open(&path, false, false).unwrap();
        let image = launch::artifact(ArtifactRole::QemuImg, &pin).unwrap();
        let cwd = LocalPath::parse(dir.path().to_str().unwrap()).unwrap();
        let issuer =
            TestIssuer::open(&Installation::fixture(), fx.ctl(), source, &config()).unwrap();
        Self {
            image,
            cwd,
            _dir: dir,
            issuer,
            cancel: Shutdown::default(),
        }
    }

    fn intent<'r>(&'r self, admitted: &'r Admitted) -> Intent<'r, &'f FakeSource, FxClock> {
        self.issuer
            .intent(
                &admitted.worker,
                NativeTarget {
                    generation: admitted.slots.disk.generation,
                    writer_epoch: None,
                },
                &self.cancel,
            )
            .unwrap()
    }
}

/// Stands in for the qemu-img child behind the checked creation: it mints
/// through the real intent, consumes at create, writes only after a
/// consumed permit, and reports the release the core would.
struct FixtureOverlay<'f, 'i> {
    run: &'f Run<'f>,
    intent: &'f Intent<'i, &'f FakeSource, FxClock>,
    mode: Mode,
    writes: &'f Cell<u32>,
    cancelled: &'f Cell<bool>,
    before_consume: &'f dyn Fn(),
}

fn held_release<'a>(
    workspace: &'a ProductionWorkspace,
    slot: ArtifactSlot,
    identity: FileIdentity,
    inputs: Vec<(ArtifactRole, FileIdentity)>,
) -> ReleasedGeneration<'a> {
    let pin = workspace.pin_slot(slot, identity, true).unwrap();
    let guard = pin.reopen_without_writer().unwrap();
    ReleasedGeneration::checked(
        workspace.binding(),
        ReleaseTargets::Helper {
            kind: PlanKind::Mutate,
            inputs,
        },
        WorkspaceCustody::production(&workspace.root, &workspace.lease).unwrap(),
        (vec![pin].into(), vec![guard]),
    )
    .unwrap()
}

impl OverlayHelper for FixtureOverlay<'_, '_> {
    fn image(&self) -> FileIdentity {
        self.run.image.identity().unwrap()
    }

    async fn create<'a>(
        self,
        prepared: &'a PreparedHelper<'a, 'a>,
        golden: &'a HeldGolden,
        size: u64,
    ) -> HelperSettlement<'a>
    where
        Self: 'a,
    {
        let backing = golden.backing().unwrap();
        let header = golden.header().unwrap();
        let staging = prepared.staging().unwrap();
        let plan = crate::vm::qemu_img::create(
            prepared.lease(),
            &self.run.image,
            &staging,
            Some((&backing, &header)),
            size,
            size + GIB,
        )
        .unwrap();
        let binding = prepared.workspace().binding();
        let permit = match self
            .intent
            .launch(binding, prepared.launch_target(&plan, &self.run.cwd))
        {
            Ok(permit) => permit,
            Err(reason) => return HelperSettlement::quarantined(Err(reason)),
        };
        (self.before_consume)();
        let other = Artifact::new(
            ArtifactRole::Golden,
            staging.path_for(ArtifactRole::Staging).unwrap().clone(),
        )
        .unwrap()
        .with_identity(prepared.identity())
        .with_pin(golden.evidence[0].pin);
        let substituted = crate::vm::qemu_img::create(
            prepared.lease(),
            &self.run.image,
            &staging,
            Some((&other, &header)),
            size,
            size + GIB,
        )
        .unwrap();
        let consumed = if self.mode == Mode::SubstitutedBacking {
            permit.consume(prepared.launch_target(&substituted, &self.run.cwd))
        } else {
            permit.consume(prepared.launch_target(&plan, &self.run.cwd))
        };
        if let Err(reason) = consumed {
            return HelperSettlement::quarantined(Err(reason));
        }
        let name = backing
            .path_for(ArtifactRole::Golden)
            .unwrap()
            .as_str()
            .to_owned();
        let name = match self.mode {
            Mode::WrongBacking => format!("{name}.other"),
            _ => name,
        };
        let target = slot_path(&prepared.workspace().root.path, prepared.slot()).unwrap();
        std::fs::OpenOptions::new()
            .write(true)
            .open(target.as_str())
            .and_then(|mut file| {
                use std::io::Write;
                file.write_all(&backed_header(size, &name))
            })
            .unwrap();
        self.writes.set(self.writes.get() + 1);
        let mut inputs: Vec<_> = plan
            .inputs()
            .iter()
            .map(|(role, input)| (*role, input.identity().unwrap()))
            .collect();
        if self.mode == Mode::WrongInputs {
            inputs.retain(|(role, _)| *role != ArtifactRole::Golden);
        }
        let proof = held_release(
            prepared.workspace(),
            prepared.slot(),
            prepared.identity(),
            inputs,
        );
        let clean = |output| HelperSettlement {
            output,
            disposition: StopDisposition::CleanRelease,
            released: Some(proof),
        };
        match self.mode {
            Mode::ExitNonzero => clean(Err(NativeRefusal::JobUnverified)),
            Mode::NoRelease => HelperSettlement::quarantined(Ok(Vec::new())),
            Mode::CancelAfterRelease => {
                self.cancelled.set(true);
                clean(Ok(Vec::new()))
            }
            _ => clean(Ok(Vec::new())),
        }
    }
}

/// The observations a settlement makes while the worker still owns, or no
/// longer owns, its holds.
#[derive(Default)]
struct Observed {
    golden_lease_free: Option<bool>,
    clone_lease_free: Option<bool>,
    golden_writable: Option<bool>,
    overlay_writable: Option<bool>,
    vars_writable: Option<bool>,
}

struct Case<'f> {
    fx: &'f SweepFixture,
    golden: &'f Golden,
    mode: Mode,
    /// Wraps the settlement, for example to tamper with the pair.
    tamper: Option<&'f dyn Fn([ArtifactEvidence; 2]) -> [ArtifactEvidence; 2]>,
    before_consume: &'f dyn Fn(),
    before_vars: &'f dyn Fn(),
    ledger: Option<&'f dyn CustodyCheckpoints>,
    settle_fault: Option<SettleFault>,
}

/// A lost settlement: nothing committed, or committed with a lost reply.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SettleFault {
    BeforeCommit,
    AfterCommit,
}

impl<'f> Case<'f> {
    fn new(fx: &'f SweepFixture, golden: &'f Golden, mode: Mode) -> Self {
        Self {
            fx,
            golden,
            mode,
            tamper: None,
            before_consume: &|| {},
            before_vars: &|| {},
            ledger: None,
            settle_fault: None,
        }
    }

    /// Stages and settles one admitted clone; returns the settlement status,
    /// the overlay writes and the observations made during settlement.
    async fn stage(
        &self,
        run: &Run<'_>,
        admitted: &Admitted,
    ) -> (anyhow::Result<Status>, u32, Observed) {
        let fx = self.fx;
        let intent = run.intent(admitted);
        let writes = Cell::new(0);
        let cancelled = Cell::new(false);
        let observed = std::cell::RefCell::new(Observed::default());
        let clone_ledger = LedgerCheckpoints {
            store: fx.store(),
            origin: Origin::Clone,
        };
        let ledger: &dyn CustodyCheckpoints = self.ledger.unwrap_or(&clone_ledger);
        let expires: i64 = fx
            .store()
            .transact(|db| {
                Ok(db.query_row(
                    "SELECT expires_ms FROM vm_instances WHERE id=?1",
                    [admitted.vm],
                    |row| row.get(0),
                )?)
            })
            .unwrap();
        let reading = crate::vm::Clock::now(&fx.clock);
        let left = u64::try_from(expires - reading.utc_ms).unwrap_or(0);
        let is_cancelled = || cancelled.get();
        let request = CloneRequest {
            slots: admitted.slots,
            size: SIZE,
            worker_deadline: admitted.worker.deadline(),
            effect_cutoff: reading.monotonic + Duration::from_millis(left),
            owner: fx.owner_sid().0,
            cancelled: &is_cancelled,
        };
        let helper = FixtureOverlay {
            run,
            intent: &intent,
            mode: self.mode,
            writes: &writes,
            cancelled: &cancelled,
            before_consume: self.before_consume,
        };
        let golden = self.golden;
        let clone_generation = admitted.slots.disk.generation;
        let status = stage_linked_clone(
            || hold(fx, golden),
            || reserve_workspace_under(&fx.production, fx.owner_sid().0, clone_generation, ledger),
            ledger,
            &request,
            helper,
            |slot, content| {
                (self.before_vars)();
                let authority = intent.copy(slot, content)?.consume(slot, content)?;
                authority
                    .names(slot, content)
                    .then_some(())
                    .ok_or(NativeRefusal::QualificationMissing)
            },
            |outcome| {
                let mut seen = observed.borrow_mut();
                let owner = fx.owner_sid().0;
                let state = fx
                    .store()
                    .transact(|db| store::native::disposal::generation_state(db, clone_generation))
                    .unwrap();
                seen.golden_lease_free =
                    Some(hold_workspace_under(&fx.production, owner, golden.scope).is_ok());
                if let GenerationState::Bound { scope, .. } = state {
                    let binding = super::super::disposal::binding_from_scope(scope);
                    seen.clone_lease_free =
                        Some(hold_workspace_under(&fx.production, owner, binding).is_ok());
                }
                seen.golden_writable = Some(
                    std::fs::OpenOptions::new()
                        .write(true)
                        .open(fx.slot_file(golden.slots.disk))
                        .is_ok(),
                );
                seen.overlay_writable = Some(
                    std::fs::OpenOptions::new()
                        .write(true)
                        .open(fx.slot_file(admitted.slots.disk))
                        .is_ok(),
                );
                seen.vars_writable = Some(
                    exists(&fx.slot_file(admitted.slots.vars))
                        && std::fs::OpenOptions::new()
                            .write(true)
                            .open(fx.slot_file(admitted.slots.vars))
                            .is_ok(),
                );
                drop(seen);
                let outcome = match (outcome, self.tamper) {
                    (Ok(pair), Some(tamper)) => Ok(tamper(pair)),
                    (outcome, _) => outcome,
                };
                match self.settle_fault {
                    Some(SettleFault::BeforeCommit) => anyhow::bail!("settlement lost"),
                    Some(SettleFault::AfterCommit) => {
                        fx.ctl()
                            .finish_native_clone(&admitted.worker, admitted.slots, outcome)?;
                        anyhow::bail!("acknowledgement lost")
                    }
                    None => fx
                        .ctl()
                        .finish_native_clone(&admitted.worker, admitted.slots, outcome),
                }
            },
        )
        .await;
        (status, writes.get(), observed.into_inner())
    }
}

fn digest_file(path: &std::path::Path) -> [u8; 32] {
    crate::vm::qualification_record::digest(&std::fs::read(path).unwrap())
}

fn count(fx: &SweepFixture, sql: &str, vm: Id) -> i64 {
    fx.store()
        .transact(|db| store::count(db, sql, [vm]))
        .unwrap()
}

fn claims(fx: &SweepFixture, vm: Id) -> i64 {
    count(fx, "SELECT count(*) FROM vm_disk_claims WHERE vm=?1", vm)
}

/// Sweeps until the staging route settles or the bound is reached.
fn sweep_until_retired(fx: &SweepFixture, vm: Id) -> bool {
    for _ in 0..6 {
        fx.clock.advance(0, 900);
        assert!(fx.sweep(None));
        if count(fx, "SELECT count(*) FROM vm_instances WHERE id=?1", vm) == 0 {
            return true;
        }
    }
    false
}

/// The golden pair and its sealed rows are unchanged.
fn assert_golden_intact(fx: &SweepFixture, golden: &Golden) {
    assert_eq!(
        digest_file(&fx.slot_file(golden.slots.disk)),
        golden.evidence[0].pin.sha256
    );
    assert_eq!(
        digest_file(&fx.slot_file(golden.slots.vars)),
        golden.evidence[1].pin.sha256
    );
    let sealed = fx
        .store()
        .transact(|db| {
            Ok(
                store::evidence_matches(db, golden.slots.disk, golden.evidence[0], "sealed")?
                    && store::evidence_matches(
                        db,
                        golden.slots.vars,
                        golden.evidence[1],
                        "sealed",
                    )?,
            )
        })
        .unwrap();
    assert!(sealed, "the golden stays sealed");
    assert!(
        hold(fx, golden).is_ok(),
        "the golden is released after the worker"
    );
}

/// T25/T26/I05/I18: the verified overlay and VARS publish once, under the
/// held golden, with the initial clean epoch, the admitted lease and the
/// single peak claim. A replay returns the same receipt and mints nothing.
#[tokio::test]
async fn clone_publishes_only_the_verified_pair_under_the_held_golden() {
    let fx = SweepFixture::new();
    let golden = seal_golden(&fx, 1);
    let source = FakeSource::new(Instant::now());
    let run = Run::new(&fx, &source);
    let admitted = admit(&fx, &golden, 1, TTL);
    let expires = NOW + i64::try_from(TTL * 1_000).unwrap();
    // T09: a client retry while the daemon stages gets the same receipt and
    // adds no work or charge.
    let during = Cell::new(None);
    let retry = || {
        during.set(Some((
            replay(&fx, &golden, 1, TTL).id,
            claims(&fx, admitted.vm),
        )))
    };
    let mut case = Case::new(&fx, &golden, Mode::Clean);
    case.before_vars = &retry;
    let (status, writes, seen) = case.stage(&run, &admitted).await;
    assert_eq!(status.unwrap(), Status::Succeeded);
    assert_eq!(writes, 1);
    assert_eq!(during.get(), Some((admitted.operation.id, 1)));
    // Publication happened under every hold.
    assert_eq!(seen.golden_lease_free, Some(false));
    assert_eq!(seen.clone_lease_free, Some(false));
    assert_eq!(seen.golden_writable, Some(false));
    assert_eq!(seen.overlay_writable, Some(false));
    assert_eq!(seen.vars_writable, Some(false));
    let generation = admitted.slots.disk.generation;
    let (active, release, expiry, verified) = fx
        .store()
        .transact(|db| {
            Ok((
                store::count(
                    db,
                    "SELECT count(*) FROM vm_generations WHERE id=?1 AND active=1",
                    [generation],
                )?,
                store::count(
                    db,
                    "SELECT count(*) FROM vm_snapshot_release WHERE generation=?1 AND epoch=?1
                     AND disposition='clean'",
                    [generation],
                )?,
                db.query_row(
                    "SELECT expires_ms FROM vm_instances WHERE id=?1 AND state=1",
                    [admitted.vm],
                    |row| row.get::<_, i64>(0),
                )?,
                [
                    store::native::leaf(db, admitted.slots.disk)?,
                    store::native::leaf(db, admitted.slots.vars)?,
                ],
            ))
        })
        .unwrap();
    assert_eq!((active, release, expiry), (1, 1, expires));
    assert!(
        verified
            .iter()
            .all(|leaf| matches!(leaf, Leaf::Verified(..)))
    );
    assert_eq!(claims(&fx, admitted.vm), 1);
    assert_eq!(
        count(
            &fx,
            "SELECT persistent FROM vm_disk_claims WHERE vm=?1",
            admitted.vm
        ),
        i64::try_from(CLAIM).unwrap()
    );
    let overlay = std::fs::read(fx.slot_file(admitted.slots.disk)).unwrap();
    let backing = hold(&fx, &golden).unwrap().backing().unwrap();
    assert_eq!(
        crate::vm::qemu_img::inspect_header(
            &overlay,
            Some(backing.path_for(ArtifactRole::Golden).unwrap())
        ),
        Ok(SIZE)
    );
    assert_eq!(
        std::fs::read(fx.slot_file(admitted.slots.vars)).unwrap(),
        GOLDEN_VARS
    );
    assert_golden_intact(&fx, &golden);

    let first = admitted.operation.id;
    assert_eq!(replay(&fx, &golden, 1, TTL).id, first);
    assert_eq!(
        count(
            &fx,
            "SELECT count(*) FROM vm_instances WHERE id<>?1",
            admitted.vm
        ),
        0
    );
    assert_eq!(claims(&fx, admitted.vm), 1);
    assert!(
        run.issuer
            .intent(
                &admitted.worker,
                NativeTarget {
                    generation,
                    writer_epoch: None
                },
                &run.cancel
            )
            .is_err(),
        "a worker mints one intent"
    );
    assert_admission_closed();
}

/// A reaper whose independent quiesce cannot confirm release.
struct UnconfirmedReaper;

impl ReaperRuntime for UnconfirmedReaper {
    fn quiesce(&self, _: &CleanupWork, _: Instant) -> Quiesced {
        Quiesced::Unconfirmed
    }

    fn graceful_stop(&self, _: &CleanupWork, _: Instant) -> StopDisposition {
        StopDisposition::ChargedQuarantine
    }

    fn force_reap(&self, _: &CleanupWork, _: Instant) -> StopDisposition {
        StopDisposition::ChargedQuarantine
    }
}

/// T21/T22/T24/T27/I08: a refused or unproven helper never publishes.
/// Every hold is dropped before the failure settles, the claim stays until
/// the reaper's independent quiesce and the shared disposal retire the exact
/// staging objects, and a replay returns the first (failed) receipt.
#[tokio::test]
async fn refused_helper_settles_after_holds_drop_and_retires_through_disposal() {
    for (index, mode) in [
        Mode::ExitNonzero,
        Mode::NoRelease,
        Mode::WrongInputs,
        Mode::CancelAfterRelease,
        Mode::WrongBacking,
    ]
    .into_iter()
    .enumerate()
    {
        let fx = SweepFixture::new();
        let golden = seal_golden(&fx, 2);
        let source = FakeSource::new(Instant::now());
        let run = Run::new(&fx, &source);
        let key = 2 + index as u16;
        let admitted = admit(&fx, &golden, key, TTL);
        let (status, writes, seen) = Case::new(&fx, &golden, mode).stage(&run, &admitted).await;
        assert_eq!(status.unwrap(), Status::Interrupted, "{mode:?}");
        assert_eq!(writes, 1, "{mode:?}: the helper ran once");
        assert_eq!(
            seen.golden_lease_free,
            Some(true),
            "{mode:?}: golden released first"
        );
        assert_eq!(
            seen.clone_lease_free,
            Some(true),
            "{mode:?}: staging released first"
        );
        let generation = admitted.slots.disk.generation;
        assert!(exists(&fx.slot_file(admitted.slots.disk)), "{mode:?}");
        assert!(
            !exists(&fx.slot_file(admitted.slots.vars)),
            "{mode:?}: no VARS copy"
        );
        assert_eq!(claims(&fx, admitted.vm), 1, "{mode:?}: claim retained");
        let cause: bool = fx
            .store()
            .transact(|db| {
                Ok(db.query_row(
                    "SELECT failure_reason=?2 AND status<>0 FROM vm_operations WHERE activity_id=?1",
                    rusqlite::params![
                        admitted.operation.receipt.activity_id,
                        FailureReason::CloneFailed as i64
                    ],
                    |row| row.get(0),
                )?)
            })
            .unwrap();
        assert!(
            cause,
            "{mode:?}: the receipt settled with its failure cause"
        );
        if mode == Mode::NoRelease {
            // Unproven release: the reaper cannot quiesce, nothing retires.
            fx.clock.advance(0, 900);
            let ledger = LedgerDisposal { store: fx.store() };
            let guards = GenerationGuards::default();
            let custody = fx.disposal(&ledger, &guards);
            assert!(fx.ctl().sweep_with(&UnconfirmedReaper, &custody).unwrap());
            assert_eq!(claims(&fx, admitted.vm), 1);
            assert!(exists(&fx.slot_file(admitted.slots.disk)));
        }
        assert_eq!(
            replay(&fx, &golden, key, TTL).id,
            admitted.operation.id,
            "{mode:?}"
        );
        assert!(sweep_until_retired(&fx, admitted.vm), "{mode:?}");
        assert!(
            !exists(&fx.gen_dir(generation)),
            "{mode:?}: directory retired"
        );
        assert_eq!(claims(&fx, admitted.vm), 0, "{mode:?}: claim freed");
        assert_eq!(
            count(
                &fx,
                "SELECT count(*) FROM vm_native_generations WHERE generation=?1",
                generation
            ),
            0
        );
        assert_eq!(
            replay(&fx, &golden, key, TTL).id,
            admitted.operation.id,
            "{mode:?}"
        );
        assert_golden_intact(&fx, &golden);
        drop(run);
        assert_admission_closed();
    }
}

/// Fails the identity checkpoint of one slot role after its create-new, so
/// the leaf is present and unidentified (the irreducible window, I15).
struct LoseIdentity<'a> {
    inner: LedgerCheckpoints<'a>,
    role: &'static str,
}

impl CustodyCheckpoints for LoseIdentity<'_> {
    fn intend_workspace(&self, g: Id, p: FileIdentity) -> Result<Option<FileIdentity>, Q> {
        self.inner.intend_workspace(g, p)
    }
    fn workspace_directory(&self, g: Id, d: FileIdentity) -> Result<(), Q> {
        self.inner.workspace_directory(g, d)
    }
    fn workspace_bound(&self, b: WorkspaceBinding) -> Result<(), Q> {
        self.inner.workspace_bound(b)
    }
    fn confirm_bound(&self, b: WorkspaceBinding) -> Result<(), Q> {
        self.inner.confirm_bound(b)
    }
    fn workspace_intent_settled(
        &self,
        g: Id,
        l: store::native::WorkspaceLeaf,
        p: store::native::Presence,
    ) -> Result<(), Q> {
        self.inner.workspace_intent_settled(g, l, p)
    }
    fn confirm_start_pair(&self, b: WorkspaceBinding, p: StartPair) -> Result<(), Q> {
        self.inner.confirm_start_pair(b, p)
    }
    fn intend_slot(&self, b: WorkspaceBinding, s: ArtifactSlot) -> Result<(), Q> {
        self.inner.intend_slot(b, s)
    }
    fn slot_identified(
        &self,
        b: WorkspaceBinding,
        s: ArtifactSlot,
        i: FileIdentity,
    ) -> Result<(), Q> {
        if s.role == self.role {
            return Err(PATH);
        }
        self.inner.slot_identified(b, s, i)
    }
    fn confirm_identified(
        &self,
        b: WorkspaceBinding,
        s: ArtifactSlot,
        i: FileIdentity,
    ) -> Result<(), Q> {
        self.inner.confirm_identified(b, s, i)
    }
    fn slot_verified(
        &self,
        b: WorkspaceBinding,
        s: ArtifactSlot,
        e: ArtifactEvidence,
    ) -> Result<(), Q> {
        self.inner.slot_verified(b, s, e)
    }
    fn slot_intent_settled(&self, b: WorkspaceBinding, s: ArtifactSlot, p: bool) -> Result<(), Q> {
        self.inner.slot_intent_settled(b, s, p)
    }
}

/// I15/I18/T27/T33 restart: a failure after the first verified leaf, or a
/// lost settlement, rolls back and retires the exact claim and files across
/// a restart with no volatile state. An unidentified present leaf keeps the
/// claim charged and is never deleted.
#[tokio::test]
async fn failure_after_the_first_leaf_retires_across_restart_and_unknown_stays_charged() {
    // (a) The VARS write fails after its identity is durable.
    {
        let mut fx = SweepFixture::new();
        let golden = seal_golden(&fx, 3);
        let source = FakeSource::new(Instant::now());
        let admitted = {
            let run = Run::new(&fx, &source);
            let admitted = admit(&fx, &golden, 10, TTL);
            let mut case = Case::new(&fx, &golden, Mode::Clean);
            let fault = || WRITE_FAULT.set(Some(WriteFault::Partial));
            case.before_vars = &fault;
            let (status, ..) = case.stage(&run, &admitted).await;
            WRITE_FAULT.set(None);
            assert_eq!(status.unwrap(), Status::Interrupted);
            admitted.vm
        };
        let vm = admitted;
        let leaf = |fx: &SweepFixture, role| {
            fx.store()
                .transact(|db| {
                    let slot: ArtifactSlot = db.query_row(
                        "SELECT g.id,a.object_key FROM vm_generations g JOIN vm_artifacts a
                         ON a.generation_id=g.id WHERE g.vm_id=?1 AND a.role=?2",
                        rusqlite::params![vm, role],
                        |row| {
                            Ok(ArtifactSlot {
                                generation: row.get(0)?,
                                role: if role == "disk" { "disk" } else { "vars" },
                                key: row.get(1)?,
                            })
                        },
                    )?;
                    store::native::leaf(db, slot)
                })
                .unwrap()
        };
        assert!(matches!(leaf(&fx, "disk"), Leaf::Verified(..)));
        assert!(matches!(leaf(&fx, "vars"), Leaf::Identified(..)));
        fx.reopen();
        assert!(
            sweep_until_retired(&fx, vm),
            "known staging retires after restart"
        );
        assert_eq!(claims(&fx, vm), 0);
        assert_golden_intact(&fx, &golden);
        assert_admission_closed();
    }
    // (b) The settlement is lost before it commits: the pending receipt
    // stays visible and restart recovery retires the staged clone. With a
    // lost reply after the commit, the published clone survives restart.
    for fault in [SettleFault::BeforeCommit, SettleFault::AfterCommit] {
        let mut fx = SweepFixture::new();
        let golden = seal_golden(&fx, 4);
        let source = FakeSource::new(Instant::now());
        let (vm, slots) = {
            let run = Run::new(&fx, &source);
            let admitted = admit(&fx, &golden, 11, TTL);
            let mut case = Case::new(&fx, &golden, Mode::Clean);
            case.settle_fault = Some(fault);
            let (status, ..) = case.stage(&run, &admitted).await;
            assert!(status.is_err(), "{fault:?}: reported once, never resettled");
            let pending = count(
                &fx,
                "SELECT count(*) FROM activity WHERE vm=?1 AND status=0 AND action=1",
                admitted.vm,
            );
            assert_eq!(
                pending,
                i64::from(fault == SettleFault::BeforeCommit),
                "{fault:?}"
            );
            (admitted.vm, admitted.slots)
        };
        fx.reopen();
        if fault == SettleFault::BeforeCommit {
            assert!(
                sweep_until_retired(&fx, vm),
                "restart recovery retires the clone"
            );
            assert!(!exists(&fx.gen_dir(slots.disk.generation)));
            assert_eq!(claims(&fx, vm), 0);
        } else {
            assert!(fx.sweep(None));
            assert_eq!(
                count(
                    &fx,
                    "SELECT count(*) FROM vm_instances WHERE id=?1 AND state=1",
                    vm
                ),
                1,
                "the committed clone survives restart"
            );
            assert_eq!(claims(&fx, vm), 1);
            assert!(exists(&fx.slot_file(slots.disk)) && exists(&fx.slot_file(slots.vars)));
        }
        assert_golden_intact(&fx, &golden);
        assert_admission_closed();
    }
    // (c) The VARS identity is lost after its create-new: unknown present.
    {
        let mut fx = SweepFixture::new();
        let golden = seal_golden(&fx, 5);
        let source = FakeSource::new(Instant::now());
        let (vm, slots) = {
            let run = Run::new(&fx, &source);
            let admitted = admit(&fx, &golden, 12, TTL);
            let losing = LoseIdentity {
                inner: LedgerCheckpoints {
                    store: fx.store(),
                    origin: Origin::Clone,
                },
                role: "vars",
            };
            let mut case = Case::new(&fx, &golden, Mode::Clean);
            case.ledger = Some(&losing);
            let (status, ..) = case.stage(&run, &admitted).await;
            assert_eq!(status.unwrap(), Status::Interrupted);
            (admitted.vm, admitted.slots)
        };
        let vars = || {
            fx.store()
                .transact(|db| store::native::leaf(db, slots.vars))
                .unwrap()
        };
        assert_eq!(vars(), Leaf::Unidentified);
        assert!(exists(&fx.slot_file(slots.vars)));
        assert!(
            !sweep_until_retired(&fx, vm),
            "unknown present stays charged"
        );
        fx.reopen();
        assert!(!sweep_until_retired(&fx, vm), "and across restart");
        assert_eq!(claims(&fx, vm), 1);
        assert!(exists(&fx.slot_file(slots.vars)), "never deleted");
        assert!(exists(&fx.lease_file(slots.vars.generation)));
        assert_golden_intact(&fx, &golden);
        assert_admission_closed();
    }
}

/// T12/T25/I06: a stale permit, a substituted backing or a changed golden
/// writes nothing, and the failure still retires through disposal.
#[tokio::test]
async fn stale_permit_or_wrong_backing_writes_nothing() {
    // Changed golden content: refused before any clone workspace exists.
    {
        let fx = SweepFixture::new();
        let golden = seal_golden(&fx, 6);
        let source = FakeSource::new(Instant::now());
        let run = Run::new(&fx, &source);
        let admitted = admit(&fx, &golden, 20, TTL);
        let mut changed = unbacked_qcow2_header(SIZE);
        changed[200] = 1;
        std::fs::write(fx.slot_file(golden.slots.disk), &changed).unwrap();
        let (status, writes, _) = Case::new(&fx, &golden, Mode::Clean)
            .stage(&run, &admitted)
            .await;
        assert_eq!(status.unwrap(), Status::Interrupted);
        assert_eq!(writes, 0);
        let generation = admitted.slots.disk.generation;
        assert!(
            !exists(&fx.gen_dir(generation)),
            "no clone write under a wrong golden"
        );
        assert!(matches!(
            fx.store()
                .transact(|db| store::native::disposal::generation_state(db, generation))
                .unwrap(),
            GenerationState::Unreserved { .. }
        ));
        assert!(sweep_until_retired(&fx, admitted.vm));
        assert_eq!(claims(&fx, admitted.vm), 0);
        assert_admission_closed();
    }
    // Wrong sealed identity, content or roles: refused at hold.
    {
        let fx = SweepFixture::new();
        let golden = seal_golden(&fx, 7);
        assert!(hold(&fx, &golden).is_ok(), "positive control");
        let mut wrong = golden;
        wrong.evidence[0].identity.file[0] ^= 1;
        assert!(hold(&fx, &wrong).is_err());
        let mut wrong = golden;
        wrong.evidence[0].pin.sha256[0] ^= 1;
        assert!(hold(&fx, &wrong).is_err());
        let mut wrong = golden;
        wrong.evidence[1].pin.sha256[0] ^= 1;
        assert!(hold(&fx, &wrong).is_err());
        let mut wrong = golden;
        wrong.slots = PairSlots {
            disk: golden.slots.vars,
            vars: golden.slots.disk,
        };
        assert!(hold(&fx, &wrong).is_err());
        assert_admission_closed();
    }
    // Stale permits: cancellation, an expired worker, or a substituted plan
    // at consumption.
    for (index, stale) in ["cancel", "deadline", "substitute"].into_iter().enumerate() {
        let fx = SweepFixture::new();
        let golden = seal_golden(&fx, 8);
        let source = FakeSource::new(Instant::now());
        let run = Run::new(&fx, &source);
        let admitted = admit(&fx, &golden, 30 + index as u16, TTL);
        let cancel = || run.cancel.cancel();
        let expire = || fx.clock.advance(0, 600);
        let mut case = Case::new(
            &fx,
            &golden,
            if stale == "substitute" {
                Mode::SubstitutedBacking
            } else {
                Mode::Clean
            },
        );
        case.before_consume = match stale {
            "cancel" => &cancel,
            "deadline" => &expire,
            _ => &|| {},
        };
        let (status, writes, _) = case.stage(&run, &admitted).await;
        assert_eq!(writes, 0, "{stale}: no write without a consumed permit");
        assert_eq!(status.unwrap(), Status::Interrupted, "{stale}");
        assert!(sweep_until_retired(&fx, admitted.vm), "{stale}");
        assert_eq!(claims(&fx, admitted.vm), 0, "{stale}");
        assert_golden_intact(&fx, &golden);
        assert_admission_closed();
    }
}

/// T26/I18: the ledger publishes only the pair its custody verified.
#[tokio::test]
async fn publication_requires_the_ledger_verified_pair() {
    let fx = SweepFixture::new();
    let golden = seal_golden(&fx, 9);
    let source = FakeSource::new(Instant::now());
    let run = Run::new(&fx, &source);
    let admitted = admit(&fx, &golden, 40, TTL);
    let mut case = Case::new(&fx, &golden, Mode::Clean);
    let altered = |mut pair: [ArtifactEvidence; 2]| {
        pair[1].pin.sha256[0] ^= 1;
        pair
    };
    case.tamper = Some(&altered);
    let (status, ..) = case.stage(&run, &admitted).await;
    assert_eq!(status.unwrap(), Status::Interrupted);
    assert_eq!(
        count(
            &fx,
            "SELECT count(*) FROM vm_instances WHERE id=?1 AND state=1",
            admitted.vm
        ),
        0
    );
    assert!(sweep_until_retired(&fx, admitted.vm));
    assert_eq!(claims(&fx, admitted.vm), 0);
    assert_golden_intact(&fx, &golden);
    assert_admission_closed();
}

/// T35: an extension committed while the clone stages moves its cutoff and
/// replays its original end. Once expiry claims the staging clone, the
/// extension is refused, no later checkpoint lands, and the claim retires.
#[tokio::test]
async fn extension_races_the_claimed_staging_expiry() {
    // Extension before the claim: publication carries the extended lease.
    {
        let fx = SweepFixture::new();
        let golden = seal_golden(&fx, 10);
        let source = FakeSource::new(Instant::now());
        let run = Run::new(&fx, &source);
        let admitted = admit(&fx, &golden, 50, 60);
        let key = fresh_key(0x7777);
        let end = fx
            .ctl()
            .extend_lease("alice", admitted.vm, key, TTL)
            .unwrap()
            .unwrap();
        assert_eq!(end, NOW + i64::try_from(TTL * 1_000).unwrap());
        assert_eq!(
            fx.ctl()
                .extend_lease("alice", admitted.vm, key, 7_200)
                .unwrap(),
            Ok(end),
            "a replay returns the original end"
        );
        let (status, ..) = Case::new(&fx, &golden, Mode::Clean)
            .stage(&run, &admitted)
            .await;
        assert_eq!(status.unwrap(), Status::Succeeded);
        assert_eq!(
            count(
                &fx,
                "SELECT expires_ms FROM vm_instances WHERE id=?1",
                admitted.vm
            ),
            end
        );
        assert_eq!(claims(&fx, admitted.vm), 1, "no new charge from extension");
        assert_admission_closed();
    }
    // Expiry claims the staging clone mid-create: extension is refused and
    // the worker's next checkpoint cannot land.
    {
        let fx = SweepFixture::new();
        let golden = seal_golden(&fx, 11);
        let source = FakeSource::new(Instant::now());
        let run = Run::new(&fx, &source);
        let admitted = admit(&fx, &golden, 51, 60);
        let key = fresh_key(0x7778);
        let claim = || {
            fx.clock.advance(61_000, 0);
            assert!(fx.sweep(None));
            assert_eq!(
                fx.ctl()
                    .extend_lease("alice", admitted.vm, key, TTL)
                    .unwrap(),
                Err(Refusal::CleanupUnconfirmed),
                "a claimed staging clone cannot be extended"
            );
        };
        let mut case = Case::new(&fx, &golden, Mode::Clean);
        case.before_vars = &claim;
        let (status, ..) = case.stage(&run, &admitted).await;
        assert_eq!(status.unwrap(), Status::Interrupted);
        assert!(
            !exists(&fx.slot_file(admitted.slots.vars)),
            "no checkpoint after the claim"
        );
        assert!(sweep_until_retired(&fx, admitted.vm));
        assert_eq!(claims(&fx, admitted.vm), 0);
        assert!(
            fx.ctl()
                .extend_lease("alice", admitted.vm, key, TTL)
                .unwrap()
                .is_err()
        );
        assert_golden_intact(&fx, &golden);
        assert_admission_closed();
    }
}

/// I24: the helper cutoff is the earliest of worker, effect and budget, one
/// tick either side, and an elapsed cutoff precreates nothing.
#[test]
fn helper_cutoff_is_the_earliest_worker_effect_or_budget() {
    let now = Instant::now();
    let tick = Duration::from_millis(1);
    let budget = now + HELPER_BUDGET;
    assert_eq!(helper_deadline(budget + tick, budget + tick, now), budget);
    assert_eq!(
        helper_deadline(budget - tick, budget + tick, now),
        budget - tick
    );
    assert_eq!(
        helper_deadline(budget + tick, budget - tick, now),
        budget - tick
    );
    assert_eq!(helper_deadline(budget, budget, now), budget);

    let fx = SweepFixture::new();
    let golden = seal_golden(&fx, 12);
    let admitted = admit(&fx, &golden, 60, TTL);
    let ledger = LedgerCheckpoints {
        store: fx.store(),
        origin: Origin::Clone,
    };
    let ws = reserve_workspace_under(
        &fx.production,
        fx.owner_sid().0,
        admitted.slots.disk.generation,
        &ledger,
    )
    .unwrap();
    let past = Instant::now();
    assert!(
        ws.prepare_helper(
            admitted.slots.disk,
            &ledger,
            past,
            past + HELPER_BUDGET,
            &|| false
        )
        .is_err()
    );
    assert!(
        ws.prepare_helper(
            admitted.slots.disk,
            &ledger,
            Instant::now() + HELPER_BUDGET,
            Instant::now() + HELPER_BUDGET,
            &|| true
        )
        .is_err()
    );
    let untouched = fx
        .store()
        .transact(|db| store::native::leaf(db, admitted.slots.disk))
        .unwrap();
    assert_eq!(
        untouched,
        Leaf::NeverAttempted,
        "no create intent after the cutoff"
    );
    let prepared = ws
        .prepare_helper(
            admitted.slots.disk,
            &ledger,
            Instant::now() + HELPER_BUDGET * 2,
            Instant::now() + HELPER_BUDGET * 2,
            &|| false,
        )
        .unwrap();
    assert!(prepared.deadline() <= Instant::now() + HELPER_BUDGET);
    assert!(matches!(
        fx.store()
            .transact(|db| store::native::leaf(db, admitted.slots.disk))
            .unwrap(),
        Leaf::Identified(file) if file == prepared.identity().file
    ));
    assert_eq!(
        std::fs::metadata(fx.slot_file(admitted.slots.disk))
            .unwrap()
            .len(),
        0,
        "identified empty before any helper write"
    );
    assert_admission_closed();
}

/// T19: a Mutate plan's offline lease names exactly its held target, and
/// writable inspection stays closed.
#[test]
fn the_helper_lease_names_exactly_the_held_target() {
    let fx = SweepFixture::new();
    let golden = seal_golden(&fx, 13);
    let admitted = admit(&fx, &golden, 70, TTL);
    let ledger = LedgerCheckpoints {
        store: fx.store(),
        origin: Origin::Clone,
    };
    let ws = reserve_workspace_under(
        &fx.production,
        fx.owner_sid().0,
        admitted.slots.disk.generation,
        &ledger,
    )
    .unwrap();
    assert!(
        ws.precreate_helper_target(admitted.slots.vars, &ledger)
            .is_err(),
        "only disk-shaped helper targets"
    );
    let target = ws
        .precreate_helper_target(admitted.slots.disk, &ledger)
        .unwrap();
    let exact = OfflineLease::helper_target(ws.binding(), target.identity());
    let mut other_id = target.identity();
    other_id.file[0] ^= 1;
    let other = OfflineLease::helper_target(ws.binding(), other_id);
    let mut other_binding = ws.binding();
    other_binding.generation = golden.scope.generation;
    let elsewhere = OfflineLease::helper_target(other_binding, target.identity());
    let check = |kind, lease, target| check_helper_lease(kind, lease, &ws, target);
    assert_eq!(check(PlanKind::Mutate, Some(&exact), Some(&target)), Ok(()));
    for lease in [&other, &elsewhere] {
        assert_eq!(
            check(PlanKind::Mutate, Some(lease), Some(&target)),
            Err(PATH)
        );
    }
    assert_eq!(check(PlanKind::Mutate, None, Some(&target)), Err(PATH));
    assert_eq!(check(PlanKind::Mutate, Some(&exact), None), Err(PATH));
    assert_eq!(
        check(PlanKind::InspectWritable, Some(&exact), None),
        Err(PATH)
    );
    assert_eq!(
        check(PlanKind::InspectWritable, Some(&exact), Some(&target)),
        Err(PATH)
    );
    assert_eq!(check(PlanKind::Boot, None, None), Ok(()));
    assert_eq!(check(PlanKind::Inspect, None, None), Ok(()));
    assert_admission_closed();
}

/// I06: the clone copy permit binds the admitted clone, its own VARS slot
/// and the exact content, once, and refreshes at consumption.
#[test]
fn the_copy_permit_binds_the_clone_slot_content_and_admission() {
    for case in ["negatives", "cancelled", "positive"] {
        let fx = SweepFixture::new();
        let golden = seal_golden(&fx, 14);
        let source = FakeSource::new(Instant::now());
        let run = Run::new(&fx, &source);
        let admitted = admit(&fx, &golden, 80, TTL);
        let intent = run.intent(&admitted);
        let content = golden.evidence[1].pin;
        let vars = admitted.slots.vars;
        match case {
            "negatives" => {
                assert!(
                    intent.copy(admitted.slots.disk, content).is_err(),
                    "VARS only"
                );
                let mut foreign = vars;
                foreign.generation = golden.slots.vars.generation;
                assert!(
                    intent.copy(foreign, content).is_err(),
                    "own generation only"
                );
                let permit = intent.copy(vars, content).unwrap();
                assert!(intent.copy(vars, content).is_err(), "one permit per slot");
                let mut altered = content;
                altered.sha256[0] ^= 1;
                assert!(permit.consume(vars, altered).is_err(), "exact content");
            }
            "cancelled" => {
                let permit = intent.copy(vars, content).unwrap();
                run.cancel.cancel();
                assert!(
                    permit.consume(vars, content).is_err(),
                    "refreshed at consumption"
                );
            }
            _ => {
                let authority = intent
                    .copy(vars, content)
                    .unwrap()
                    .consume(vars, content)
                    .unwrap();
                assert!(authority.names(vars, content));
                assert!(!authority.names(admitted.slots.disk, content));
            }
        }
        assert_admission_closed();
    }
}

/// I08/T20–T24: helper output is separate from release. Output needs a
/// release of exactly the planned inputs with this target as staging, a
/// clean disposition and a final cancellation and cutoff check; a refusal
/// keeps the held proof, an unmatched or missing release is uncertain.
#[test]
fn helper_output_needs_a_matched_clean_release_and_a_final_check() {
    let fx = SweepFixture::new();
    let golden = seal_golden(&fx, 16);
    let admitted = admit(&fx, &golden, 90, TTL);
    let ledger = LedgerCheckpoints {
        store: fx.store(),
        origin: Origin::Clone,
    };
    let ws = reserve_workspace_under(
        &fx.production,
        fx.owner_sid().0,
        admitted.slots.disk.generation,
        &ledger,
    )
    .unwrap();
    let far = Instant::now() + HELPER_BUDGET * 2;
    let prepared = ws
        .prepare_helper(admitted.slots.disk, &ledger, far, far, &|| false)
        .unwrap();
    let image = FileIdentity {
        volume: prepared.identity().volume,
        file: [0x51; 16],
    };
    let inputs = [
        (ArtifactRole::QemuImg, image),
        (ArtifactRole::Staging, prepared.identity()),
        (ArtifactRole::Golden, golden.evidence[0].identity),
    ];
    let settle = |output: Result<Vec<u8>, NativeRefusal>, disposition, proven: Option<&[_]>| {
        HelperSettlement {
            output,
            disposition,
            released: proven.map(|inputs: &[(ArtifactRole, FileIdentity)]| {
                held_release(&ws, prepared.slot(), prepared.identity(), inputs.to_vec())
            }),
        }
    };
    let writable = || {
        std::fs::OpenOptions::new()
            .write(true)
            .open(fx.slot_file(admitted.slots.disk))
            .is_ok()
    };
    let clean = StopDisposition::CleanRelease;
    match prepared.accept(
        settle(Ok(b"ok".to_vec()), clean, Some(&inputs)),
        &inputs,
        &|| false,
    ) {
        HelperOutcome::Accepted { output, proof } => {
            assert_eq!(output, b"ok");
            assert!(!writable(), "the accepted target stays writer-excluded");
            drop(proof);
            assert!(writable());
        }
        _ => panic!("matched clean release with output is accepted"),
    }
    for (name, settlement, cancelled) in [
        (
            "post-sample cancellation",
            settle(Ok(Vec::new()), clean, Some(&inputs)),
            true,
        ),
        (
            "helper error",
            settle(Err(NativeRefusal::JobUnverified), clean, Some(&inputs)),
            false,
        ),
        (
            "dirty release",
            settle(Ok(Vec::new()), StopDisposition::DirtyRelease, Some(&inputs)),
            false,
        ),
    ] {
        match prepared.accept(settlement, &inputs, &|| cancelled) {
            HelperOutcome::Refused { proof, .. } => {
                assert!(!writable(), "{name}: the refused output keeps its proof");
                drop(proof);
            }
            _ => panic!("{name}: output is refused with the proof kept"),
        }
    }
    let other = [inputs[0], inputs[1]];
    let foreign = [inputs[0], (ArtifactRole::Staging, image), inputs[2]];
    for (name, settlement, expected) in [
        (
            "unproven release",
            settle(Ok(Vec::new()), StopDisposition::ChargedQuarantine, None),
            &inputs[..],
        ),
        (
            "other inputs",
            settle(Ok(Vec::new()), clean, Some(&other)),
            &inputs[..],
        ),
        (
            "expected inputs name another staging",
            settle(Ok(Vec::new()), clean, Some(&inputs)),
            &foreign[..],
        ),
    ] {
        assert!(
            matches!(
                prepared.accept(settlement, expected, &|| false),
                HelperOutcome::Uncertain(_)
            ),
            "{name}: no matched release"
        );
    }
    assert_admission_closed();
}
