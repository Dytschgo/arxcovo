//! Offline TEST snapshot capture, restore and ordinary expiry through real
//! NTFS custody, the VM ledger, the production issuer bodies and the shared
//! resumable disposal route (7c 15: I11, I16–I19, I22–I24, T35, T51,
//! T61–T74). A fixture stands in for the read-only qemu-img child only;
//! every other edge is the production code.

use super::super::clone::tests::{
    Golden, SIZE, backed_header, fresh_key, golden_backing, host, resources, seal_golden,
};
use super::super::disposal::tests::{Fault, NOW, OfflineReaper, SweepFixture, exists};
use super::*;
use crate::service::Shutdown;
use crate::store::vm::{
    self as store, Binding,
    native::{Origin, release::Target},
};
use crate::vm::reaper::{CleanupArtifact, CleanupKind, CleanupWork};
use crate::vm::runtime::{Quiesced, ReaperRuntime};
use crate::vm::snapshot::Request;
use crate::vm::test_issuer::tests::{FakeSource, assert_admission_closed, config};
use crate::vm::test_issuer::{Installation, NativeTarget, TestIssuer};
use crate::vm::{OperationRef, RequestId, Status, Subject, VaultTestCleanup};
use rusqlite::params;
use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;

const MINUTE_MS: i64 = 60_000;
/// The fake-time lease: expiry is claimed on the first sweep after it.
const LEASE_MS: i64 = 30 * MINUTE_MS;
/// The fake-time point at which the ordinary sweep runs.
const SWEEP_AT_MS: i64 = 40 * MINUTE_MS;
const VM_CLAIM: u64 = 1 << 30;
const CLONE_VARS: &[u8] = b"clone vars, same length";
const FIRMWARE: ArtifactPin = ArtifactPin {
    size: 4096,
    sha256: [0x46; 32],
};

fn vm_id(nonce: u8) -> Id {
    let mut vm = [0xb0; 16];
    vm[15] = nonce;
    vm
}

/// A valid clone overlay over the golden, tagged so captures differ.
fn clone_disk(fx: &SweepFixture, golden: &Golden, tag: u8) -> Vec<u8> {
    let mut disk = backed_header(SIZE, golden_backing(fx, golden).as_str());
    disk[60_000] = tag;
    disk
}

/// A stopped native TEST VM over the sealed golden, staged through the real
/// custody checkpoints and the clone settlement, with its exact charge.
fn active_vm(
    fx: &SweepFixture,
    golden: &Golden,
    nonce: u8,
    principal: &'static str,
    lease_ms: i64,
) -> Id {
    let vm = vm_id(nonce);
    let (activity, slots) = fx
        .store()
        .transact(|db| {
            db.execute(
                "INSERT INTO vm_instances(id,template,principal,expires_ms) VALUES(?1,?2,?3,?4)",
                params![vm, golden.template, principal, NOW + lease_ms],
            )?;
            let activity = Binding {
                principal,
                key: RequestId::parse(&format!("{:032x}", 0x700 + u32::from(nonce))).unwrap(),
                subject: Subject::Template(golden.template, [nonce; 32]),
                action: 1,
            }
            .record(db, NOW, Some(vm), Some(NOW + lease_ms))?
            .receipt
            .activity_id;
            db.execute(
                "INSERT INTO vm_disk_claims(activity_id,vm,persistent,staging) VALUES(?1,?2,?3,0)",
                params![activity, vm, VM_CLAIM],
            )?;
            Ok((activity, store::plan_clone(db, vm)?))
        })
        .unwrap();
    let ledger = LedgerCheckpoints {
        store: fx.store(),
        origin: Origin::Clone,
    };
    let ws = reserve_workspace_under(
        &fx.production,
        fx.owner_sid().0,
        slots.disk.generation,
        &ledger,
    )
    .unwrap();
    let evidence = [
        fx.stage_slot(&ws, &ledger, slots.disk, &clone_disk(fx, golden, nonce)),
        fx.stage_slot(&ws, &ledger, slots.vars, CLONE_VARS),
    ];
    drop(ws);
    fx.store()
        .transact(|db| {
            assert_eq!(
                store::settle_clone(db, activity, slots, Some(evidence), true, NOW, None)?,
                Status::Succeeded
            );
            Ok(())
        })
        .unwrap();
    vm
}

fn active(fx: &SweepFixture, vm: Id) -> crate::store::vm::snapshot::Active {
    fx.store()
        .transact(|db| store::snapshot::active(db, vm, false))
        .unwrap()
        .expect("released active pair")
}

fn scope_of(fx: &SweepFixture, generation: Id) -> WorkspaceBinding {
    bound_scope(&LedgerDisposal { store: fx.store() }, generation).unwrap()
}

fn lease_free(fx: &SweepFixture, scope: WorkspaceBinding) -> bool {
    hold_workspace_under(&fx.production, fx.owner_sid().0, scope).is_ok()
}

fn disk_bytes(fx: &SweepFixture) -> u64 {
    fx.store()
        .transact(|db| store::disk_bytes(db))
        .unwrap()
        .unwrap()
}

fn query<T: rusqlite::types::FromSql>(fx: &SweepFixture, sql: &str, id: Id) -> T {
    fx.store()
        .transact(|db| Ok(db.query_row(sql, [id], |row| row.get(0))?))
        .unwrap()
}

/// A stale ledger view while the inputs are held: another epoch released
/// the same bytes.
fn move_release(fx: &SweepFixture, vm: Id, epoch: Id) {
    fx.store()
        .transact(|db| {
            store::snapshot::invalidate_release(db, vm, epoch)?;
            db.execute(
                "UPDATE vm_snapshot_release SET disposition='clean' WHERE epoch=?1",
                [epoch],
            )?;
            Ok(())
        })
        .unwrap();
}

/// A new writer epoch over the active pair, before any write.
fn begin_epoch(fx: &SweepFixture, vm: Id, epoch: Id) {
    fx.store()
        .transact(|db| store::snapshot::invalidate_release(db, vm, epoch))
        .unwrap();
}

/// The exact release commit (7c 06) for `epoch` from a held proof of the
/// current pair, without a stop receipt.
fn commit_release(fx: &SweepFixture, vm: Id, epoch: Id, clean: bool) {
    let (slots, evidence) = fx
        .store()
        .transact(|db| {
            let generation: Id = db.query_row(
                "SELECT id FROM vm_generations WHERE vm_id=?1 AND active=1",
                [vm],
                |row| row.get(0),
            )?;
            let mut stmt = db.prepare(
                "SELECT object_key,volume_id,file_id FROM vm_artifacts
                 WHERE generation_id=?1 ORDER BY role",
            )?;
            let rows = stmt
                .query_map([generation], |row| {
                    Ok((
                        row.get::<_, Id>(0)?,
                        u64::from_be_bytes(row.get(1)?),
                        row.get::<_, Id>(2)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let slot = |role, at: usize| ArtifactSlot {
                generation,
                role,
                key: rows[at].0,
            };
            let id = |at: usize| FileIdentity {
                volume: rows[at].1,
                file: rows[at].2,
            };
            Ok((
                PairSlots {
                    disk: slot("disk", 0),
                    vars: slot("vars", 1),
                },
                [id(0), id(1)],
            ))
        })
        .unwrap();
    let ws = hold_workspace_under(
        &fx.production,
        fx.owner_sid().0,
        scope_of(fx, slots.disk.generation),
    )
    .unwrap();
    let disposition = if clean {
        StopDisposition::CleanRelease
    } else {
        StopDisposition::DirtyRelease
    };
    let proof = super::super::release_tests::proof(&ws, slots, evidence, disposition);
    proof
        .observe_test_release(
            Target {
                vm,
                generation: slots.disk.generation,
                epoch,
            },
            Instant::now() + Duration::from_secs(10),
            &|| false,
        )
        .unwrap()
        .commit(fx.store(), None, || fx.clock.now())
        .unwrap();
}

/// One offline guest run: a new writer epoch, new disk content in the same
/// object, then the exact clean release of that epoch.
fn run_guest(fx: &SweepFixture, golden: &Golden, vm: Id, tag: u8) {
    let epoch = [0xe0 ^ tag; 16];
    let disk = active(fx, vm).pair.0.disk;
    begin_epoch(fx, vm, epoch);
    std::fs::write(fx.slot_file(disk), clone_disk(fx, golden, tag)).unwrap();
    commit_release(fx, vm, epoch, true);
}

struct Admitted {
    work: Work,
    worker: PendingOperation,
    operation: OperationRef,
    key: RequestId,
}

fn admit(fx: &SweepFixture, vm: Id, number: u32, request: &Request) -> Admitted {
    let action = match request {
        Request::Create(_) => 5,
        Request::Restore(_) => 6,
        Request::Delete(_) => 18,
    };
    let key = fresh_key(number);
    let (work, worker, operation) = fx
        .ctl()
        .admit_snapshot("alice", key, vm, action, request, None, host(), resources())
        .unwrap()
        .unwrap_or_else(|reason| panic!("snapshot admission refused: {reason:?}"));
    Admitted {
        work,
        worker,
        operation,
        key,
    }
}

fn same_work(work: &Work) -> Work {
    Work { ..*work }
}

/// Stands in for the sealed read-only `qemu-img info` child: it reads the
/// held disk's first cluster and reports what the pinned helper would.
type InspectionCheck<'a> = dyn Fn(&InspectTarget<'_>, &SnapshotInfo<'_>, &CopyBudget<'_>) + 'a;

struct FixtureInspector<'a> {
    calls: u32,
    fail: bool,
    check: &'a InspectionCheck<'a>,
}

impl SnapshotInspector for FixtureInspector<'_> {
    fn inspect(
        &mut self,
        target: InspectTarget<'_>,
        info: &SnapshotInfo<'_>,
        output: &mut [u8],
        budget: &CopyBudget<'_>,
    ) -> Result<usize, copy::Refusal> {
        budget.check()?;
        self.calls += 1;
        let path = info.path().as_str();
        if self.fail
            || info.args() != ["info", "--output=json", "-f", "qcow2", path]
            || slot_path(&target.workspace.root.path, target.slot).as_ref() != Ok(info.path())
        {
            return Err(copy::Refusal::Inspection);
        }
        (self.check)(&target, info, budget);
        let mut header = vec![0; crate::vm::qemu_img::CLUSTER];
        read_exact_at(target.file, 0, &mut header)?;
        let golden = info
            .golden()
            .map(|golden| golden.path_for(ArtifactRole::Golden).cloned())
            .transpose()
            .map_err(|_| copy::Refusal::Inspection)?;
        let virtual_size = crate::vm::qemu_img::inspect_header(&header, golden.as_ref())
            .map_err(|_| copy::Refusal::Inspection)?;
        let actual = target.file.metadata().map_err(|_| copy::Refusal::Io)?.len();
        let mut body = serde_json::json!({
            "filename": path,
            "format": "qcow2",
            "virtual-size": virtual_size,
            "actual-size": actual,
            "dirty-flag": false,
        });
        if let Some(golden) = golden {
            body["backing-filename"] = serde_json::json!(golden.as_str());
            body["full-backing-filename"] = serde_json::json!(golden.as_str());
            body["backing-filename-format"] = serde_json::json!("qcow2");
        }
        let encoded = serde_json::to_vec(&body).map_err(|_| copy::Refusal::Inspection)?;
        output
            .get_mut(..encoded.len())
            .ok_or(copy::Refusal::Inspection)?
            .copy_from_slice(&encoded);
        Ok(encoded.len())
    }
}

/// What the settlement saw while the worker still owned, or no longer
/// owned, its holds.
#[derive(Debug, Default)]
struct Observed {
    holds: u32,
    checkpoints: u32,
    inspections: u32,
    commit_holds_live: Option<bool>,
    fail_holds_free: Option<bool>,
    failed: Option<[StagedLeaf; 2]>,
}

/// Wraps the production controller ledger to observe hold lifetimes and to
/// lose a settlement, as a crash would.
struct Recording<'a> {
    inner: ControllerSnapshot<'a, super::super::disposal::tests::FakeClock>,
    fx: &'a SweepFixture,
    scopes: Vec<WorkspaceBinding>,
    candidate: Id,
    seen: RefCell<Observed>,
    before_commit: &'a dyn Fn(),
    lose_commit: bool,
    lose_failure: bool,
}

impl Recording<'_> {
    fn scopes(&self) -> Vec<WorkspaceBinding> {
        let mut scopes = self.scopes.clone();
        if let Ok(candidate) = bound_scope(
            &LedgerDisposal {
                store: self.fx.store(),
            },
            self.candidate,
        ) {
            scopes.push(candidate);
        }
        scopes
    }
}

impl SnapshotLedger for Recording<'_> {
    fn checkpoint(&self) -> anyhow::Result<()> {
        self.seen.borrow_mut().checkpoints += 1;
        self.inner.checkpoint()
    }

    fn leaf_verified(&self, slot: ArtifactSlot, evidence: ArtifactEvidence) -> anyhow::Result<()> {
        self.inner.leaf_verified(slot, evidence)
    }

    fn commit(&self, proof: [ArtifactEvidence; 2]) -> anyhow::Result<()> {
        let live = self
            .scopes()
            .into_iter()
            .all(|scope| !lease_free(self.fx, scope));
        self.seen.borrow_mut().commit_holds_live = Some(live);
        (self.before_commit)();
        if self.lose_commit {
            anyhow::bail!("the process ended before the commit");
        }
        self.inner.commit(proof)
    }

    fn fail(&self, leaves: [StagedLeaf; 2]) -> anyhow::Result<()> {
        let free = self
            .scopes()
            .into_iter()
            .all(|scope| lease_free(self.fx, scope));
        let mut seen = self.seen.borrow_mut();
        seen.fail_holds_free = Some(free);
        seen.failed = Some(leaves);
        drop(seen);
        if self.lose_failure {
            return Ok(());
        }
        self.inner.fail(leaves)
    }
}

struct Case<'c> {
    /// Replaces the admitted work the adapter receives.
    work: Option<Work>,
    /// Runs right after every input's lease and writer exclusion are held.
    after_hold: &'c dyn Fn(),
    /// Runs inside the settlement, just before the commit transaction.
    before_commit: &'c dyn Fn(),
    /// Mints and consumes the copy authority for this content instead.
    copy_content: Option<ArtifactPin>,
    custody: Option<&'c dyn CustodyCheckpoints>,
    inspect_fails: bool,
    checked_inspection: bool,
    cancelled: &'c dyn Fn() -> bool,
    lose_commit: bool,
    lose_failure: bool,
}

impl Default for Case<'_> {
    fn default() -> Self {
        Self {
            work: None,
            after_hold: &|| {},
            before_commit: &|| {},
            copy_content: None,
            custody: None,
            inspect_fails: false,
            checked_inspection: false,
            cancelled: &|| false,
            lose_commit: false,
            lose_failure: false,
        }
    }
}

/// Runs one admitted copy through the production adapter, the real issuer
/// and the controller ledger.
fn run(
    fx: &SweepFixture,
    source: &FakeSource,
    admitted: &Admitted,
    case: Case<'_>,
) -> (anyhow::Result<()>, Observed) {
    let issuer = TestIssuer::open(&Installation::fixture(), fx.ctl(), source, &config()).unwrap();
    let cancel = Shutdown::default();
    let candidate = admitted.work.candidate.unwrap();
    let intent = issuer
        .intent(
            &admitted.worker,
            NativeTarget {
                generation: candidate.disk.generation,
                writer_epoch: None,
            },
            &cancel,
        )
        .unwrap();
    let work = case.work.unwrap_or_else(|| same_work(&admitted.work));
    let restore_ledger = LedgerCheckpoints {
        store: fx.store(),
        origin: Origin::Restore,
    };
    let custody: &dyn CustodyCheckpoints = case.custody.unwrap_or(&restore_ledger);
    let disposal = LedgerDisposal { store: fx.store() };
    let owner = fx.owner_sid().0;
    let reading = fx.clock.now();
    let expires: i64 = query(
        fx,
        "SELECT expires_ms FROM vm_instances WHERE id=?1",
        work.vm,
    );
    let left = u64::try_from(expires - reading.utc_ms).unwrap_or(0);
    let stopped = || cancel.is_cancelled() || (case.cancelled)();
    let request = SnapshotRequest {
        work: &work,
        worker: &admitted.worker,
        production: &fx.production,
        owner,
        firmware: FIRMWARE,
        effect_cutoff: reading.monotonic + Duration::from_millis(left),
        cancelled: &stopped,
    };
    let mut scopes = vec![
        scope_of(fx, admitted.work.active.pair.0.disk.generation),
        scope_of(fx, admitted.work.active.golden.unwrap().0.disk.generation),
    ];
    if admitted.work.action == 6 {
        scopes.push(scope_of(fx, admitted.work.source.0.disk.generation));
    }
    let host_now = host;
    let ledger = Recording {
        inner: ControllerSnapshot {
            controller: fx.ctl(),
            worker: &admitted.worker,
            work: &admitted.work,
            host: &host_now,
            resources: resources(),
        },
        fx,
        scopes,
        candidate: candidate.disk.generation,
        seen: RefCell::default(),
        before_commit: case.before_commit,
        lose_commit: case.lose_commit,
        lose_failure: case.lose_failure,
    };
    let holds = Cell::new(0);
    let check_inspection =
        |target: &InspectTarget<'_>, info: &SnapshotInfo<'_>, budget: &CopyBudget<'_>| {
            if !case.checked_inspection {
                return;
            }
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("qemu-img.exe");
            std::fs::write(&path, b"fixture image").unwrap();
            let image = LocalPath::parse(path.to_str().unwrap()).unwrap();
            let pin = PathPin::open(&image, false, false).unwrap();
            let image = launch::artifact(ArtifactRole::QemuImg, &pin).unwrap();
            let cwd = LocalPath::parse(dir.path().to_str().unwrap()).unwrap();
            let ws = target.workspace;
            let lease = OfflineLease::for_inspection(ws, target.slot, target.identity);
            let plan = offline_snapshot_info(&lease, &image, info).unwrap();
            assert_eq!(
                helper::check_helper_lease(plan.kind(), Some(&lease), ws, None, plan.inputs()),
                Ok(())
            );
            assert!(
                helper::check_helper_lease(PlanKind::Mutate, Some(&lease), ws, None, plan.inputs())
                    .is_err()
            );
            let pins = hold_plan_inputs(&plan, budget.deadline(), &|| false).unwrap();
            assert!(pins.iter().all(|pin| !pin.mutable));
            assert_eq!(pins[1].identity, target.identity);
            let binding = ws.binding();
            let launch_target = inspection_launch_target(&plan, ws, &cwd);
            let permit = intent.launch(binding, launch_target).unwrap();
            assert!(
                permit
                    .consume(inspection_launch_target(&plan, ws, &cwd))
                    .is_ok()
            );
            assert!(
                intent
                    .launch(binding, inspection_launch_target(&plan, ws, &cwd))
                    .is_err()
            );
            let wrong = ArtifactSlot {
                role: "vars",
                ..target.slot
            };
            let wrong_lease = OfflineLease::for_inspection(ws, wrong, target.identity);
            let wrong_plan = offline_snapshot_info(&wrong_lease, &image, info).unwrap();
            assert!(
                helper::check_helper_lease(
                    wrong_plan.kind(),
                    Some(&wrong_lease),
                    ws,
                    None,
                    wrong_plan.inputs()
                )
                .is_err()
            );
            assert!(
                intent
                    .launch(binding, inspection_launch_target(&wrong_plan, ws, &cwd))
                    .is_err()
            );
        };
    let mut inspector = FixtureInspector {
        calls: 0,
        fail: case.inspect_fails,
        check: &check_inspection,
    };
    let result = copy_offline_snapshot(
        &request,
        || {
            holds.set(holds.get() + 1);
            let held = Holds::hold(
                &fx.production,
                owner,
                &restore_ledger,
                &disposal,
                &work,
                request.cutoff(),
                request.cancelled,
            )?;
            (case.after_hold)();
            Ok(held)
        },
        || reserve_workspace_under(&fx.production, owner, candidate.disk.generation, custody),
        custody,
        |slot, content| {
            let named = case.copy_content.unwrap_or(content);
            intent.copy(slot, named)?.consume(slot, named)
        },
        &mut inspector,
        &ledger,
    );
    let mut seen = ledger.seen.into_inner();
    seen.holds = holds.get();
    seen.inspections = inspector.calls;
    (result, seen)
}

/// One ordinary sweep turn after any retry backoff.
fn sweep(fx: &SweepFixture) {
    fx.clock.advance(0, 61);
    assert!(fx.sweep(None));
}

fn receipt_status(fx: &SweepFixture, activity: i64) -> i64 {
    fx.store()
        .transact(|db| {
            Ok(db.query_row(
                "SELECT status FROM activity WHERE id=?1",
                [activity],
                |row| row.get(0),
            )?)
        })
        .unwrap()
}

fn generation_rows(fx: &SweepFixture, generation: Id) -> i64 {
    fx.store()
        .transact(|db| {
            store::count(
                db,
                "SELECT (SELECT count(*) FROM vm_generations WHERE id=?1)
                    +(SELECT count(*) FROM vm_native_generations WHERE generation=?1)
                    +(SELECT count(*) FROM vm_native_slots WHERE generation=?1)
                    +(SELECT count(*) FROM vm_artifacts WHERE generation_id=?1)",
                [generation],
            )
        })
        .unwrap()
}

/// A4/A5 at a quiescent end state: the production root holds exactly these
/// generation directories, each with exactly its disk, VARS and lease, and
/// the ledger binds native custody for exactly these generations.
fn assert_exact_inventory(fx: &SweepFixture, expected: &[PairSlots]) {
    let names = |dir: &std::path::Path| -> BTreeSet<String> {
        std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect()
    };
    let root = std::path::Path::new(fx.production.path.as_str());
    let wanted: BTreeSet<String> = expected
        .iter()
        .map(|pair| hex::encode(pair.disk.generation))
        .collect();
    assert_eq!(names(root), wanted, "exact generation directory set");
    for pair in expected {
        let generation = pair.disk.generation;
        let _workspace =
            hold_workspace_under(&fx.production, fx.owner_sid().0, scope_of(fx, generation))
                .unwrap();
        let leaves: BTreeSet<String> = [
            super::super::disposal::leaf_name(pair.disk),
            super::super::disposal::leaf_name(pair.vars),
            GENERATION_LEASE.to_owned(),
        ]
        .into_iter()
        .collect();
        assert_eq!(names(&fx.gen_dir(pair.disk.generation)), leaves);
    }
    let count = expected.len() as i64;
    assert_eq!(
        fx.count("SELECT count(*) FROM vm_native_generations"),
        count
    );
    assert_eq!(fx.count("SELECT count(*) FROM vm_generations"), count);
    assert_eq!(fx.count("SELECT count(*) FROM vm_artifacts"), 2 * count);
    assert_eq!(fx.count("SELECT count(*) FROM vm_native_slots"), 2 * count);
}

/// T61/T62/I19: a capture holds the exact released pair and golden, writes
/// only after its permits and identity checkpoints, and publishes one point
/// with its exact charge while every hold is live. A replay mints nothing.
#[test]
fn native_capture_publishes_one_point_with_exact_lineage_charge_and_replay() {
    assert_admission_closed();
    let fx = SweepFixture::new();
    let source = FakeSource::new(Instant::now());
    let golden = seal_golden(&fx, 1);
    let vm = active_vm(&fx, &golden, 1, "alice", LEASE_MS);
    let before = active(&fx, vm);
    let charged = disk_bytes(&fx);
    let golden_bytes = std::fs::read(fx.slot_file(golden.slots.disk)).unwrap();

    let admitted = admit(&fx, vm, 1, &Request::Create(None));
    let (result, seen) = run(&fx, &source, &admitted, Case::default());
    result.unwrap();
    assert_eq!(seen.holds, 1);
    assert!(seen.checkpoints >= 2, "{seen:?}");
    assert_eq!(seen.inspections, 2, "source and staging inspected");
    assert_eq!(seen.commit_holds_live, Some(true));
    assert_eq!(seen.fail_holds_free, None);

    let candidate = admitted.work.candidate.unwrap();
    let point = admitted.work.target;
    assert_eq!(
        receipt_status(&fx, admitted.operation.receipt.activity_id),
        1
    );
    assert_eq!(
        query::<String>(
            &fx,
            "SELECT state FROM vm_restore_points WHERE id=?1",
            point
        ),
        "ready"
    );
    assert_eq!(
        query::<Id>(
            &fx,
            "SELECT generation FROM vm_restore_points WHERE id=?1",
            point
        ),
        candidate.disk.generation
    );
    let charge: u64 = query(
        &fx,
        "SELECT charge FROM vm_restore_points WHERE id=?1",
        point,
    );
    assert_eq!(charge, admitted.work.claim.persistent().unwrap());
    assert_eq!(disk_bytes(&fx), charged + charge, "staging released");
    assert_eq!(fx.count("SELECT count(*) FROM vm_snapshot_work"), 0);
    // Byte-identical point under its own bound scope, verified natively.
    for (slot, original) in [candidate.disk, candidate.vars]
        .into_iter()
        .zip([before.pair.0.disk, before.pair.0.vars])
    {
        assert_eq!(
            std::fs::read(fx.slot_file(slot)).unwrap(),
            std::fs::read(fx.slot_file(original)).unwrap()
        );
    }
    assert_eq!(
        fx.count(&format!(
            "SELECT count(*) FROM vm_native_slots WHERE state='verified' AND generation=x'{}'",
            hex::encode(candidate.disk.generation)
        )),
        2
    );
    // The active pair, epoch and golden are unchanged.
    let after = active(&fx, vm);
    assert!(after.pair == before.pair && after.epoch == before.epoch);
    assert_eq!(after.lineage, before.lineage);
    assert_eq!(
        std::fs::read(fx.slot_file(golden.slots.disk)).unwrap(),
        golden_bytes
    );
    assert!(lease_free(&fx, golden.scope));
    // Same-request replay returns the first receipt and admits nothing.
    assert!(matches!(
        fx.ctl().admit_snapshot(
            "alice",
            admitted.key,
            vm,
            5,
            &Request::Create(None),
            None,
            host(),
            resources()
        ),
        Ok(Err(crate::vm::Refusal::Busy))
    ));
    let replay = fx
        .ctl()
        .foreground_replay("alice", admitted.key, Request::Create(None).binding(vm), 5)
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(replay.operation.id, admitted.operation.id);
    assert_eq!(fx.count("SELECT count(*) FROM vm_restore_points"), 1);
    assert_exact_inventory(&fx, &[golden.slots, before.pair.0, candidate]);
}

/// T63/I19: a restore moves the active pointer and charge atomically to the
/// verified fresh pair with an initial clean epoch, persists the old
/// generation's retirement, and keeps the point and golden. The ordinary
/// sweep then retires the old pair through the shared disposal route.
#[test]
fn native_restore_moves_the_pointer_retires_the_old_generation_and_keeps_lineage() {
    assert_admission_closed();
    let fx = SweepFixture::new();
    let source = FakeSource::new(Instant::now());
    let golden = seal_golden(&fx, 2);
    let vm = active_vm(&fx, &golden, 2, "alice", LEASE_MS);
    let captured = admit(&fx, vm, 1, &Request::Create(None));
    run(&fx, &source, &captured, Case::default()).0.unwrap();
    let point = captured.work.target;
    let point_pair = captured.work.candidate.unwrap();
    run_guest(&fx, &golden, vm, 0x22);
    let old = active(&fx, vm);
    assert_ne!(
        std::fs::read(fx.slot_file(old.pair.0.disk)).unwrap(),
        std::fs::read(fx.slot_file(point_pair.disk)).unwrap()
    );
    let charged = disk_bytes(&fx);

    let restored = admit(&fx, vm, 2, &Request::Restore(point));
    let (result, seen) = run(&fx, &source, &restored, Case::default());
    result.unwrap();
    assert_eq!(seen.commit_holds_live, Some(true));
    let fresh = restored.work.candidate.unwrap();
    let now = active(&fx, vm);
    assert_eq!(now.pair.0, fresh);
    assert!(now.clean);
    assert_eq!(now.epoch, fresh.disk.generation, "initial clean epoch");
    assert_eq!(now.lineage, old.lineage, "golden lineage kept");
    assert_eq!(
        std::fs::read(fx.slot_file(fresh.disk)).unwrap(),
        std::fs::read(fx.slot_file(point_pair.disk)).unwrap()
    );
    // Retirement is durable: the old pair keeps its charge until disposal.
    let old_generation = old.pair.0.disk.generation;
    assert_eq!(
        query::<String>(
            &fx,
            "SELECT phase FROM vm_snapshot_work WHERE expected=?1",
            old_generation
        ),
        "retire"
    );
    let restore_charge = restored.work.claim.persistent().unwrap();
    assert_eq!(
        query::<u64>(&fx, "SELECT persistent FROM vm_disk_claims WHERE vm=?1", vm),
        restore_charge
    );
    assert_eq!(
        disk_bytes(&fx),
        charged - VM_CLAIM + restore_charge + VM_CLAIM,
        "the retired pair stays charged until its proof"
    );
    assert!(exists(&fx.gen_dir(old_generation)));
    // The ordinary sweep retires the old pair through checked disposal.
    sweep(&fx);
    assert!(!exists(&fx.gen_dir(old_generation)));
    assert_eq!(generation_rows(&fx, old_generation), 0);
    assert_eq!(fx.count("SELECT count(*) FROM vm_snapshot_work"), 0);
    assert_eq!(disk_bytes(&fx), charged - VM_CLAIM + restore_charge);
    assert_eq!(
        fx.count("SELECT count(*) FROM vm_snapshot_reconciliation WHERE cause='retired'"),
        1
    );
    // The point and golden are untouched and the point stays restorable.
    assert_eq!(
        query::<String>(
            &fx,
            "SELECT state FROM vm_restore_points WHERE id=?1",
            point
        ),
        "ready"
    );
    assert_exact_inventory(&fx, &[golden.slots, point_pair, fresh]);
}

/// I19/T61: a release that moved after admission is stale. Before the
/// candidate exists, the first claim revalidation refuses; after the copy,
/// the commit refuses it inside its own transaction.
#[test]
fn stale_release_refuses_before_any_write_and_again_at_commit() {
    assert_admission_closed();
    for at_commit in [false, true] {
        let fx = SweepFixture::new();
        let source = FakeSource::new(Instant::now());
        let golden = seal_golden(&fx, 3);
        let vm = active_vm(&fx, &golden, 3, "alice", LEASE_MS);
        let baseline = disk_bytes(&fx);
        let admitted = admit(&fx, vm, 1, &Request::Create(None));
        let candidate = admitted.work.candidate.unwrap();
        // A stale worker view: another epoch released the same bytes,
        // through the exact release commit before the copy, or as a ledger
        // race while the copy holds its inputs.
        let restale = || move_release(&fx, vm, [0x5a; 16]);
        let case = if at_commit {
            Case {
                before_commit: &restale,
                ..Case::default()
            }
        } else {
            Case {
                after_hold: &restale,
                ..Case::default()
            }
        };
        let (result, seen) = run(&fx, &source, &admitted, case);
        assert!(
            result.is_err(),
            "stale release accepted (at_commit={at_commit})"
        );
        assert_eq!(seen.fail_holds_free, Some(true));
        assert_eq!(fx.count("SELECT count(*) FROM vm_restore_points"), 0);
        if !at_commit {
            assert!(!exists(&fx.gen_dir(candidate.disk.generation)));
            assert_eq!(seen.failed, Some([StagedLeaf::Untouched; 2]));
        }
        assert_eq!(
            receipt_status(&fx, admitted.operation.receipt.activity_id),
            2
        );
        sweep(&fx);
        assert!(!exists(&fx.gen_dir(candidate.disk.generation)));
        assert_eq!(generation_rows(&fx, candidate.disk.generation), 0);
        assert_eq!(disk_bytes(&fx), baseline);
    }
}

/// I22/T61: a swapped directory, lease or leaf refuses at the hold; a
/// Static or swapped-role work item refuses before any custody effect. No
/// staging is created, and every refusal settles through disposal.
#[test]
fn swapped_dir_lease_or_pair_and_static_or_swapped_roles_refuse_without_staging() {
    assert_admission_closed();
    #[derive(Clone, Copy, Debug)]
    enum Swap {
        Directory,
        Lease,
        Leaf,
        Static,
        Roles,
        Source,
    }
    for swap in [
        Swap::Directory,
        Swap::Lease,
        Swap::Leaf,
        Swap::Static,
        Swap::Roles,
        Swap::Source,
    ] {
        let fx = SweepFixture::new();
        let source = FakeSource::new(Instant::now());
        let golden = seal_golden(&fx, 4);
        let vm = active_vm(&fx, &golden, 4, "alice", LEASE_MS);
        let baseline = disk_bytes(&fx);
        let admitted = admit(&fx, vm, 1, &Request::Create(None));
        let candidate = admitted.work.candidate.unwrap();
        let pair = admitted.work.active.pair.0;
        let dir = fx.gen_dir(pair.disk.generation);
        let mut work = same_work(&admitted.work);
        match swap {
            Swap::Directory => {
                let recorded = scope_of(&fx, pair.disk.generation);
                std::fs::remove_dir_all(&dir).unwrap();
                // A look-alike directory with the same leaf names.
                std::fs::create_dir(&dir).unwrap();
                std::fs::write(dir.join(GENERATION_LEASE), b"").unwrap();
                for slot in [pair.disk, pair.vars] {
                    std::fs::write(fx.slot_file(slot), b"look-alike").unwrap();
                }
                let replacement = open_relative(
                    &fx.production.leaf,
                    &hex::encode(pair.disk.generation),
                    Node::Directory,
                    false,
                    false,
                )
                .unwrap();
                assert_ne!(
                    file_identity(&replacement, true).unwrap().0,
                    recorded.workspace
                );
            }
            Swap::Lease => {
                let lease = fx.lease_file(pair.disk.generation);
                std::fs::remove_file(&lease).unwrap();
                std::fs::write(&lease, b"").unwrap();
            }
            Swap::Leaf => {
                let vars = fx.slot_file(pair.vars);
                let bytes = std::fs::read(&vars).unwrap();
                std::fs::remove_file(&vars).unwrap();
                std::fs::write(&vars, bytes).unwrap();
            }
            Swap::Static => work.is_static = true,
            Swap::Roles => {
                work.candidate = Some(PairSlots {
                    disk: ArtifactSlot {
                        role: "vars",
                        ..candidate.vars
                    },
                    vars: ArtifactSlot {
                        role: "disk",
                        ..candidate.disk
                    },
                });
            }
            Swap::Source => work.source = admitted.work.active.golden.unwrap(),
        }
        let refused_early = matches!(swap, Swap::Static | Swap::Roles | Swap::Source);
        let (result, seen) = run(
            &fx,
            &source,
            &admitted,
            Case {
                work: Some(work),
                ..Case::default()
            },
        );
        assert!(result.is_err(), "{swap:?} accepted");
        assert_eq!(seen.holds, u32::from(!refused_early), "{swap:?}");
        assert_eq!(seen.checkpoints, 0, "{swap:?}");
        let static_work = matches!(swap, Swap::Static);
        assert_eq!(
            seen.failed,
            (!static_work).then_some([StagedLeaf::Untouched; 2]),
            "{swap:?}"
        );
        assert!(!exists(&fx.gen_dir(candidate.disk.generation)), "{swap:?}");
        assert_eq!(fx.count("SELECT count(*) FROM vm_restore_points"), 0);
        if static_work {
            let mut work = same_work(&admitted.work);
            work.is_static = true;
            let ledger = ControllerSnapshot {
                controller: fx.ctl(),
                worker: &admitted.worker,
                work: &work,
                host: &host,
                resources: resources(),
            };
            let proof = [work.active.pair.1[0], work.active.pair.1[1]];
            for result in [
                ledger.checkpoint(),
                ledger.leaf_verified(candidate.disk, proof[0]),
                ledger.commit(proof),
                ledger.fail([StagedLeaf::Untouched; 2]),
            ] {
                assert_eq!(
                    result.unwrap_err().to_string(),
                    "native TEST snapshot adapter refuses Static work"
                );
            }
            assert_eq!(
                fx.count("SELECT count(*) FROM vm_snapshot_work WHERE phase='copy'"),
                1
            );
            assert_eq!(receipt_status(&fx, work.activity), 0);
            assert_eq!(disk_bytes(&fx), baseline + work.claim.total().unwrap());
            continue;
        }
        sweep(&fx);
        assert_eq!(
            generation_rows(&fx, candidate.disk.generation),
            0,
            "{swap:?}"
        );
        assert_eq!(fx.count("SELECT count(*) FROM vm_snapshot_work"), 0);
        assert_eq!(disk_bytes(&fx), baseline, "{swap:?}");
    }
}

/// T64/I18: a copy write failure and a commit whose SQL transaction rolls
/// back both drop every hold before the failed settlement, publish nothing,
/// keep the original active pointer and retire the identified staging
/// through the shared disposal route.
#[test]
fn copy_failure_and_sql_rollback_drop_holds_first_and_retire_through_disposal() {
    assert_admission_closed();
    for rollback in [false, true] {
        let fx = SweepFixture::new();
        let source = FakeSource::new(Instant::now());
        let golden = seal_golden(&fx, 5);
        let vm = active_vm(&fx, &golden, 5, "alice", LEASE_MS);
        let captured = admit(&fx, vm, 1, &Request::Create(None));
        run(&fx, &source, &captured, Case::default()).0.unwrap();
        run_guest(&fx, &golden, vm, 0x55);
        let before = active(&fx, vm);
        let baseline = disk_bytes(&fx);
        let admitted = admit(&fx, vm, 2, &Request::Restore(captured.work.target));
        let candidate = admitted.work.candidate.unwrap();
        // The commit transaction applies the pointer CAS and the fresh pair's
        // activation, then fails at the initial release row and rolls all of
        // it back.
        let abort = || {
            fx.store()
                .transact(|db| {
                    db.execute_batch(
                        "CREATE TEMP TRIGGER rollback_fault BEFORE INSERT ON main.vm_snapshot_release
                         BEGIN SELECT RAISE(ABORT,'injected commit fault'); END;",
                    )?;
                    Ok(())
                })
                .unwrap();
        };
        let case = if rollback {
            Case {
                before_commit: &abort,
                ..Case::default()
            }
        } else {
            WRITE_FAULT.set(Some(WriteFault::Partial));
            Case::default()
        };
        let (result, seen) = run(&fx, &source, &admitted, case);
        WRITE_FAULT.set(None);
        if rollback {
            fx.store()
                .transact(|db| Ok(db.execute_batch("DROP TRIGGER temp.rollback_fault")?))
                .unwrap();
        }
        assert!(result.is_err());
        assert_eq!(seen.fail_holds_free, Some(true), "rollback={rollback}");
        let Some([StagedLeaf::Identified(..), vars]) = seen.failed else {
            panic!("identified staging expected: {seen:?}");
        };
        assert!(
            matches!(vars, StagedLeaf::Identified(..)),
            "rollback={rollback}"
        );
        let after = active(&fx, vm);
        assert!(after.pair == before.pair && after.epoch == before.epoch);
        assert_eq!(
            query::<u64>(&fx, "SELECT persistent FROM vm_disk_claims WHERE vm=?1", vm),
            VM_CLAIM
        );
        assert_eq!(
            receipt_status(&fx, admitted.operation.receipt.activity_id),
            2
        );
        assert!(exists(&fx.slot_file(candidate.disk)));
        assert_eq!(
            disk_bytes(&fx),
            baseline + admitted.work.claim.total().unwrap()
        );
        sweep(&fx);
        assert!(!exists(&fx.gen_dir(candidate.disk.generation)));
        assert_eq!(generation_rows(&fx, candidate.disk.generation), 0);
        assert_eq!(disk_bytes(&fx), baseline);
        assert_exact_inventory(
            &fx,
            &[
                golden.slots,
                captured.work.candidate.unwrap(),
                before.pair.0,
            ],
        );
    }
}

/// Fails the VARS identity checkpoint after its create-new: the object
/// exists without a recorded identity.
struct LoseVarsIdentity<'a>(LedgerCheckpoints<'a>);

impl CustodyCheckpoints for LoseVarsIdentity<'_> {
    fn intend_workspace(
        &self,
        generation: Id,
        production: FileIdentity,
    ) -> Result<Option<FileIdentity>, Q> {
        self.0.intend_workspace(generation, production)
    }
    fn workspace_directory(&self, generation: Id, directory: FileIdentity) -> Result<(), Q> {
        self.0.workspace_directory(generation, directory)
    }
    fn workspace_bound(&self, binding: WorkspaceBinding) -> Result<(), Q> {
        self.0.workspace_bound(binding)
    }
    fn confirm_bound(&self, binding: WorkspaceBinding) -> Result<(), Q> {
        self.0.confirm_bound(binding)
    }
    fn workspace_intent_settled(
        &self,
        generation: Id,
        leaf: crate::store::vm::native::WorkspaceLeaf,
        presence: crate::store::vm::native::Presence,
    ) -> Result<(), Q> {
        self.0.workspace_intent_settled(generation, leaf, presence)
    }
    fn confirm_start_pair(&self, binding: WorkspaceBinding, pair: StartPair) -> Result<(), Q> {
        self.0.confirm_start_pair(binding, pair)
    }
    fn intend_slot(&self, binding: WorkspaceBinding, slot: ArtifactSlot) -> Result<(), Q> {
        self.0.intend_slot(binding, slot)
    }
    fn slot_identified(
        &self,
        binding: WorkspaceBinding,
        slot: ArtifactSlot,
        identity: FileIdentity,
    ) -> Result<(), Q> {
        if slot.role == "vars" {
            return Err(PATH);
        }
        self.0.slot_identified(binding, slot, identity)
    }
    fn confirm_identified(
        &self,
        binding: WorkspaceBinding,
        slot: ArtifactSlot,
        identity: FileIdentity,
    ) -> Result<(), Q> {
        self.0.confirm_identified(binding, slot, identity)
    }
    fn slot_verified(
        &self,
        binding: WorkspaceBinding,
        slot: ArtifactSlot,
        evidence: ArtifactEvidence,
    ) -> Result<(), Q> {
        self.0.slot_verified(binding, slot, evidence)
    }
    fn slot_intent_settled(
        &self,
        binding: WorkspaceBinding,
        slot: ArtifactSlot,
        present: bool,
    ) -> Result<(), Q> {
        self.0.slot_intent_settled(binding, slot, present)
    }
}

/// I18/T64/T68: an unidentified present staging leaf is never adopted or
/// deleted. Its candidate stays charged across sweeps; the identified disk
/// is not freed on its own either, since the claim covers the pair.
#[test]
fn unidentified_present_staging_leaf_stays_charged() {
    assert_admission_closed();
    let fx = SweepFixture::new();
    let source = FakeSource::new(Instant::now());
    let golden = seal_golden(&fx, 6);
    let vm = active_vm(&fx, &golden, 6, "alice", LEASE_MS);
    let baseline = disk_bytes(&fx);
    let admitted = admit(&fx, vm, 1, &Request::Create(None));
    let candidate = admitted.work.candidate.unwrap();
    let lossy = LoseVarsIdentity(LedgerCheckpoints {
        store: fx.store(),
        origin: Origin::Restore,
    });
    let (result, seen) = run(
        &fx,
        &source,
        &admitted,
        Case {
            custody: Some(&lossy),
            ..Case::default()
        },
    );
    assert!(result.is_err());
    assert_eq!(seen.fail_holds_free, Some(true));
    assert!(matches!(
        seen.failed,
        Some([StagedLeaf::Identified(..), StagedLeaf::Untouched])
    ));
    assert!(exists(&fx.slot_file(candidate.vars)));
    for _ in 0..3 {
        sweep(&fx);
    }
    assert!(exists(&fx.slot_file(candidate.vars)), "unknown leaf kept");
    assert_eq!(
        query::<String>(
            &fx,
            "SELECT state FROM vm_native_slots WHERE object_key=?1",
            candidate.vars.key
        ),
        "quarantined"
    );
    assert_eq!(
        disk_bytes(&fx),
        baseline + admitted.work.claim.total().unwrap(),
        "the candidate's claim stays charged"
    );
    assert!(active(&fx, vm).pair == admitted.work.active.pair);
}

/// I19/T61 (review: claim revalidation after lease acquisition): a claim
/// that moved while the input leases were acquired refuses before the
/// candidate workspace exists.
#[test]
fn claim_lost_during_lease_acquisition_refuses_before_the_candidate_exists() {
    assert_admission_closed();
    let fx = SweepFixture::new();
    let source = FakeSource::new(Instant::now());
    let golden = seal_golden(&fx, 7);
    let vm = active_vm(&fx, &golden, 7, "alice", LEASE_MS);
    let baseline = disk_bytes(&fx);
    let admitted = admit(&fx, vm, 1, &Request::Create(None));
    let candidate = admitted.work.candidate.unwrap();
    // While the leases are held, a stale ledger view moves the release.
    let moved = || move_release(&fx, vm, [0x77; 16]);
    let (result, seen) = run(
        &fx,
        &source,
        &admitted,
        Case {
            after_hold: &moved,
            ..Case::default()
        },
    );
    assert!(result.is_err());
    assert_eq!(seen.holds, 1);
    assert_eq!(seen.checkpoints, 1);
    assert_eq!(seen.inspections, 0);
    assert_eq!(seen.fail_holds_free, Some(true));
    assert!(!exists(&fx.gen_dir(candidate.disk.generation)));
    assert_eq!(
        fx.count(&format!(
            "SELECT count(*) FROM vm_native_generations WHERE generation=x'{}'",
            hex::encode(candidate.disk.generation)
        )),
        0
    );
    sweep(&fx);
    assert_eq!(generation_rows(&fx, candidate.disk.generation), 0);
    assert_eq!(disk_bytes(&fx), baseline);
}

/// T70/T72/T73: a crash mid-copy, mid-restore (before its commit) and
/// mid-disposal leaves only durable state. A fresh controller with no
/// volatile state recovers the interrupted receipt, keeps the original
/// active pair and point, and the ordinary sweep converges exactly.
#[test]
fn restart_mid_copy_mid_restore_and_mid_disposal_converges_without_volatile_state() {
    assert_admission_closed();
    for crash in ["copy", "restore", "disposal"] {
        let mut fx = SweepFixture::new();
        let source = FakeSource::new(Instant::now());
        let golden = seal_golden(&fx, 8);
        let vm = active_vm(&fx, &golden, 8, "alice", LEASE_MS);
        let captured = admit(&fx, vm, 1, &Request::Create(None));
        run(&fx, &source, &captured, Case::default()).0.unwrap();
        let point_pair = captured.work.candidate.unwrap();
        run_guest(&fx, &golden, vm, 0x88);
        let before = active(&fx, vm);
        let baseline = disk_bytes(&fx);
        let admitted = admit(&fx, vm, 2, &Request::Restore(captured.work.target));
        let candidate = admitted.work.candidate.unwrap();
        let case = match crash {
            "copy" => Case {
                inspect_fails: true,
                lose_failure: true,
                ..Case::default()
            },
            "restore" => Case {
                lose_commit: true,
                lose_failure: true,
                ..Case::default()
            },
            _ => Case::default(),
        };
        let (result, _) = run(&fx, &source, &admitted, case);
        assert_eq!(result.is_ok(), crash == "disposal", "{crash}");
        if crash == "disposal" {
            // Crash after the first slot's absence checkpoint of the retired
            // pair.
            assert!(fx.sweep(Some(Fault::After("slot_disposed", 1))));
        } else {
            assert_eq!(
                receipt_status(&fx, admitted.operation.receipt.activity_id),
                0,
                "{crash}: the settlement was lost"
            );
        }
        fx.reopen();
        if crash != "disposal" {
            assert_eq!(
                receipt_status(&fx, admitted.operation.receipt.activity_id),
                3,
                "{crash}: recovery interrupts the receipt"
            );
        }
        for _ in 0..3 {
            sweep(&fx);
        }
        let now = active(&fx, vm);
        let (kept, retired) = if crash == "disposal" {
            assert_eq!(now.pair.0, candidate);
            (candidate, before.pair.0.disk.generation)
        } else {
            assert!(
                now.pair == before.pair && now.epoch == before.epoch,
                "{crash}"
            );
            (before.pair.0, candidate.disk.generation)
        };
        assert!(!exists(&fx.gen_dir(retired)), "{crash}");
        assert_eq!(generation_rows(&fx, retired), 0, "{crash}");
        assert_eq!(fx.count("SELECT count(*) FROM vm_snapshot_work"), 0);
        let expected = if crash == "disposal" {
            baseline - VM_CLAIM + admitted.work.claim.persistent().unwrap()
        } else {
            baseline
        };
        assert_eq!(disk_bytes(&fx), expected, "{crash}");
        assert_exact_inventory(&fx, &[golden.slots, point_pair, kept]);
    }
}

/// Builds the expiry scenario: two points and a pending retired pair.
fn points_and_retired(
    fx: &SweepFixture,
    source: &FakeSource,
    golden: &Golden,
    vm: Id,
) -> (Vec<PairSlots>, Id, PairSlots) {
    let first = admit(fx, vm, 1, &Request::Create(None));
    run(fx, source, &first, Case::default()).0.unwrap();
    run_guest(fx, golden, vm, 0x91);
    let second = admit(fx, vm, 2, &Request::Create(None));
    run(fx, source, &second, Case::default()).0.unwrap();
    run_guest(fx, golden, vm, 0x92);
    let retired = active(fx, vm).pair.0.disk.generation;
    let restored = admit(fx, vm, 3, &Request::Restore(first.work.target));
    run(fx, source, &restored, Case::default()).0.unwrap();
    (
        vec![
            first.work.candidate.unwrap(),
            second.work.candidate.unwrap(),
        ],
        retired,
        restored.work.candidate.unwrap(),
    )
}

/// T66/T71/I17/I18: ordinary expiry on a fake 30-minute lease, swept at 40
/// minutes, reaps points and the retired pair before the final active pair,
/// keeps each charge visible until that object's absence is proved, and
/// leaves a tombstone and exactly the golden inventory.
#[test]
fn ordinary_expiry_on_fake_time_sweeps_points_and_retired_pairs_before_the_active_pair() {
    assert_admission_closed();
    let fx = SweepFixture::new();
    let source = FakeSource::new(Instant::now());
    let golden = seal_golden(&fx, 9);
    let vm = active_vm(&fx, &golden, 9, "alice", LEASE_MS);
    let golden_only = disk_bytes(&fx) - VM_CLAIM;
    let (points, retired, live) = points_and_retired(&fx, &source, &golden, vm);
    fx.clock.advance(SWEEP_AT_MS, 0);
    let generations: Vec<Id> = points
        .iter()
        .map(|pair| pair.disk.generation)
        .chain([retired, live.disk.generation])
        .collect();
    let mut gone_at = vec![None; generations.len()];
    for turn in 0..16 {
        sweep(&fx);
        for (at, generation) in generations.iter().enumerate() {
            let present = exists(&fx.gen_dir(*generation));
            if !present && gone_at[at].is_none() {
                gone_at[at] = Some(turn);
            }
            if present {
                // Charges stay visible until this object's absence proof.
                let charged = fx.count(&format!(
                    "SELECT (SELECT count(*) FROM vm_restore_points WHERE generation=x'{0}')
                        +(SELECT count(*) FROM vm_snapshot_work WHERE expected=x'{0}')
                        +(SELECT count(*) FROM vm_disk_claims c JOIN vm_generations g
                            ON g.vm_id=c.vm AND g.active=1 AND g.id=x'{0}')",
                    hex::encode(generation)
                ));
                assert_eq!(charged, 1, "turn {turn}: generation {at} lost its charge");
            }
        }
        if gone_at.iter().all(Option::is_some) {
            break;
        }
    }
    let gone: Vec<usize> = gone_at
        .iter()
        .map(|turn| turn.expect("every generation disposed"))
        .collect();
    let active_turn = gone[generations.len() - 1];
    assert!(
        gone[..generations.len() - 1]
            .iter()
            .all(|turn| *turn < active_turn),
        "the active pair goes last: {gone:?}"
    );
    assert!(
        fx.ctl().expired_status("alice", vm).unwrap().is_some(),
        "tombstone"
    );
    assert_eq!(
        fx.store()
            .transact(|db| store::tombstone_proof(db, "alice", vm))
            .unwrap()
            .map(|(_, confirmed)| confirmed),
        Some(Some(true))
    );
    assert_eq!(fx.count("SELECT count(*) FROM vm_disk_claims"), 0);
    assert_eq!(fx.count("SELECT count(*) FROM vm_restore_points"), 0);
    assert_eq!(disk_bytes(&fx), golden_only);
    assert_exact_inventory(&fx, &[golden.slots]);
}

/// The live owner of one TEST generation: its witness, its Job's writer
/// handle and the order of the reaper's calls.
struct LiveExpiry<'a> {
    fx: &'a SweepFixture,
    vm: Id,
    epoch: Id,
    witness: Cell<bool>,
    owned: RefCell<Option<File>>,
    quarantine: Cell<bool>,
    calls: RefCell<Vec<&'static str>>,
}

impl ReaperRuntime for LiveExpiry<'_> {
    fn quiesce(&self, work: &CleanupWork, _: Instant) -> Quiesced {
        let ours = matches!(work.kind, CleanupKind::Expiry { vm, .. } if vm == self.vm);
        if ours && self.owned.borrow().is_some() {
            // Withdraw the public witness before any teardown wait.
            if self.witness.replace(false) {
                self.calls.borrow_mut().push("withdraw");
            }
            return Quiesced::OwnedTestJob;
        }
        Quiesced::Offline
    }

    fn graceful_stop(&self, work: &CleanupWork, _: Instant) -> StopDisposition {
        assert!(work.release_only && !self.witness.get());
        self.calls.borrow_mut().push("graceful");
        StopDisposition::ChargedQuarantine
    }

    fn force_reap(&self, work: &CleanupWork, _: Instant) -> StopDisposition {
        assert!(work.release_only && !self.witness.get());
        self.calls.borrow_mut().push("force");
        if self.quarantine.replace(false) {
            return StopDisposition::ChargedQuarantine;
        }
        // Fixture stand-in for confirmed Job exit: the writer closes, then
        // the owned stop path commits the exact epoch's release.
        drop(self.owned.borrow_mut().take().expect("owned writer"));
        commit_release(self.fx, self.vm, self.epoch, false);
        StopDisposition::DirtyRelease
    }
}

/// T51/T66/I11: expiry of a live generation withdraws its witness before
/// the owned stop's teardown waits, commits the exact release on its own
/// turn while every point and the active pair stay charged, and only then
/// sweeps the points before the active pair.
#[test]
fn ordinary_expiry_quiesces_a_live_generation_before_any_point_disposal() {
    assert_admission_closed();
    let fx = SweepFixture::new();
    let source = FakeSource::new(Instant::now());
    let golden = seal_golden(&fx, 10);
    let vm = active_vm(&fx, &golden, 10, "alice", LEASE_MS);
    let (points, retired, live) = points_and_retired(&fx, &source, &golden, vm);
    // The generation goes live: a running writer epoch and its open writer.
    let epoch = [0xa1; 16];
    begin_epoch(&fx, vm, epoch);
    fx.store()
        .transact(|db| {
            db.execute("UPDATE vm_instances SET state=2 WHERE id=?1", [vm])?;
            Ok(())
        })
        .unwrap();
    let writer = std::fs::OpenOptions::new()
        .write(true)
        .open(fx.slot_file(live.disk))
        .unwrap();
    let runtime = LiveExpiry {
        fx: &fx,
        vm,
        epoch,
        witness: Cell::new(true),
        owned: RefCell::new(Some(writer)),
        quarantine: Cell::new(true),
        calls: RefCell::default(),
    };
    let charged = disk_bytes(&fx);
    let guards = GenerationGuards::default();
    let ledger = LedgerDisposal { store: fx.store() };
    let custody = fx.disposal(&ledger, &guards);
    fx.clock.advance(SWEEP_AT_MS, 0);
    // A quarantined stop and the release-only retry both preserve every pair.
    for released in [false, true] {
        fx.clock.advance(0, 61);
        assert!(fx.ctl().sweep_with(&runtime, &custody).unwrap());
        assert_eq!(runtime.owned.borrow().is_none(), released);
        assert_eq!(
            fx.count("SELECT count(*) FROM vm_snapshot_release WHERE disposition='dirty'"),
            i64::from(released)
        );
        for pair in points.iter().chain([&live]) {
            assert!(exists(&fx.slot_file(pair.disk)) && exists(&fx.slot_file(pair.vars)));
        }
        assert!(
            exists(&fx.gen_dir(retired)),
            "retired pair disposed on release turn"
        );
        assert_eq!(disk_bytes(&fx), charged, "charges visible before proof");
    }
    assert_eq!(
        *runtime.calls.borrow(),
        ["withdraw", "graceful", "force", "graceful", "force"]
    );
    // Later turns sweep the points, then the active pair.
    let mut active_gone = None;
    for turn in 0..12 {
        fx.clock.advance(0, 61);
        assert!(fx.ctl().sweep_with(&runtime, &custody).unwrap());
        let points_left = points
            .iter()
            .filter(|pair| exists(&fx.gen_dir(pair.disk.generation)))
            .count();
        if !exists(&fx.gen_dir(live.disk.generation)) {
            assert_eq!(points_left, 0, "the active pair goes last");
            assert!(
                !exists(&fx.gen_dir(retired)),
                "retired pair goes before active"
            );
            active_gone = Some(turn);
            break;
        }
    }
    assert!(active_gone.is_some());
    assert_eq!(runtime.calls.borrow().len(), 5, "no stop after release");
    assert_eq!(fx.count("SELECT count(*) FROM vm_disk_claims"), 0);
    assert!(fx.ctl().expired_status("alice", vm).unwrap().is_some());
    assert_exact_inventory(&fx, &[golden.slots]);
}

/// I23/T51: a targeted vault release and the ordinary sweep run at the same
/// time over one controller and one process-wide guard set. Both converge
/// with no dropped claim, the points before the active pair, and exactly
/// the golden left.
#[test]
fn targeted_vault_release_and_simultaneous_ordinary_sweep_converge() {
    assert_admission_closed();
    let fx = SweepFixture::new();
    let source = FakeSource::new(Instant::now());
    let golden = seal_golden(&fx, 11);
    let vm = active_vm(&fx, &golden, 11, "alice", LEASE_MS);
    let vault = active_vm(&fx, &golden, 12, "arxd", LEASE_MS);
    let (points, retired, live) = points_and_retired(&fx, &source, &golden, vm);
    fx.clock.advance(SWEEP_AT_MS, 0);
    fx.ctl().expire_vault_test(vault).unwrap();
    let guards = GenerationGuards::default();
    let done = std::sync::atomic::AtomicBool::new(false);
    let outcome = std::thread::scope(|scope| {
        let ordinary = scope.spawn(|| {
            let ledger = LedgerDisposal { store: fx.store() };
            let custody = fx.disposal(&ledger, &guards);
            for _ in 0..40 {
                fx.ctl().sweep_with(&OfflineReaper, &custody).unwrap();
                if !exists(&fx.gen_dir(live.disk.generation)) {
                    break;
                }
                fx.clock.advance(0, 61);
                std::thread::sleep(Duration::from_millis(5));
            }
            done.store(true, std::sync::atomic::Ordering::SeqCst);
        });
        let targeted = scope.spawn(|| {
            let ledger = LedgerDisposal { store: fx.store() };
            let custody = fx.disposal(&ledger, &guards);
            let fenced = std::sync::atomic::AtomicBool::new(false);
            let mut turn = fx.ctl().prioritize_vault_sweep(
                vault,
                Duration::from_secs(3_600),
                Duration::from_secs(3_600),
            );
            for _ in 0..4_000 {
                if let Some(result) = fx
                    .ctl()
                    .sweep_vault_test_with(vault, &OfflineReaper, &custody, &mut turn, &fenced)
                    .unwrap()
                {
                    return Some(result);
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            None
        });
        let result = targeted.join().unwrap();
        ordinary.join().unwrap();
        result
    });
    assert!(done.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(outcome, Some(VaultTestCleanup::Confirmed));
    assert!(fx.ctl().vault_test_released(vault).unwrap());
    for pair in &points {
        assert!(!exists(&fx.gen_dir(pair.disk.generation)));
    }
    assert!(!exists(&fx.gen_dir(retired)));
    assert!(!exists(&fx.gen_dir(live.disk.generation)));
    assert!(fx.ctl().expired_status("alice", vm).unwrap().is_some());
    assert_eq!(fx.count("SELECT count(*) FROM vm_disk_claims"), 0);
    assert_eq!(fx.count("SELECT count(*) FROM vm_snapshot_work"), 0);
    assert_exact_inventory(&fx, &[golden.slots]);
    // No guard leaked from either sweep.
    for pair in points.iter().chain([&live]) {
        drop(guards.acquire(pair.disk.generation).unwrap());
    }
}

/// I22: a snapshot copy permit names only the admitted candidate's own disk
/// or VARS slot and its exact content; any other slot, role or generation
/// refuses, and the consumed authority names only what it was minted for.
#[test]
fn snapshot_copy_permits_are_scoped_to_the_candidate_disk_and_vars() {
    assert_admission_closed();
    let fx = SweepFixture::new();
    let source = FakeSource::new(Instant::now());
    let golden = seal_golden(&fx, 13);
    let vm = active_vm(&fx, &golden, 13, "alice", LEASE_MS);
    let admitted = admit(&fx, vm, 1, &Request::Create(None));
    let candidate = admitted.work.candidate.unwrap();
    let issuer = TestIssuer::open(&Installation::fixture(), fx.ctl(), &source, &config()).unwrap();
    let cancel = Shutdown::default();
    // A writer epoch never names a snapshot copy.
    assert!(
        issuer
            .intent(
                &admitted.worker,
                NativeTarget {
                    generation: candidate.disk.generation,
                    writer_epoch: Some(admitted.work.active.epoch),
                },
                &cancel,
            )
            .is_err()
    );
    let issuer = TestIssuer::open(&Installation::fixture(), fx.ctl(), &source, &config()).unwrap();
    let intent = issuer
        .intent(
            &admitted.worker,
            NativeTarget {
                generation: candidate.disk.generation,
                writer_epoch: None,
            },
            &cancel,
        )
        .unwrap();
    let pin = admitted.work.source.1[0].pin;
    let active = admitted.work.active.pair.0;
    for slot in [
        active.disk,
        golden.slots.disk,
        ArtifactSlot {
            role: "seed",
            ..candidate.disk
        },
        ArtifactSlot {
            role: "static_disk",
            ..candidate.disk
        },
    ] {
        assert!(intent.copy(slot, pin).is_err(), "{slot:?}");
    }
    let other = ArtifactPin {
        size: pin.size,
        sha256: [0x0f; 32],
    };
    assert!(
        intent
            .copy(candidate.disk, pin)
            .unwrap()
            .consume(candidate.disk, other)
            .is_err()
    );
    let authority = intent
        .copy(candidate.vars, admitted.work.source.1[1].pin)
        .unwrap()
        .consume(candidate.vars, admitted.work.source.1[1].pin)
        .unwrap();
    assert!(authority.names(candidate.vars, admitted.work.source.1[1].pin));
    assert!(!authority.names(candidate.disk, admitted.work.source.1[1].pin));
    // One permit per durable effect: the same slot cannot mint twice.
    assert!(
        intent
            .copy(candidate.vars, admitted.work.source.1[1].pin)
            .is_err()
    );
}

/// The snapshot issuer admits read-only inspection of exactly the source
/// and candidate, including a sealed restore point as the restore source.
#[test]
fn checked_inspection_admits_source_and_candidate_but_refuses_writers() {
    assert_admission_closed();
    let fx = SweepFixture::new();
    let source = FakeSource::new(Instant::now());
    let golden = seal_golden(&fx, 14);
    let vm = active_vm(&fx, &golden, 14, "alice", LEASE_MS);
    let admitted = admit(&fx, vm, 1, &Request::Create(None));
    let check = || Case {
        checked_inspection: true,
        ..Case::default()
    };
    let (result, seen) = run(&fx, &source, &admitted, check());
    result.unwrap();
    assert_eq!(seen.inspections, 2);

    let restored = admit(&fx, vm, 2, &Request::Restore(admitted.work.target));
    let (result, seen) = run(&fx, &source, &restored, check());
    result.unwrap();
    assert_eq!(seen.inspections, 2);
}

#[test]
fn dirty_release_refuses_capture_and_allows_restore() {
    assert_admission_closed();
    let fx = SweepFixture::new();
    let source = FakeSource::new(Instant::now());
    let golden = seal_golden(&fx, 15);
    let vm = active_vm(&fx, &golden, 15, "alice", LEASE_MS);
    let point = admit(&fx, vm, 1, &Request::Create(None));
    run(&fx, &source, &point, Case::default()).0.unwrap();
    let epoch = [0x57; 16];
    begin_epoch(&fx, vm, epoch);
    commit_release(&fx, vm, epoch, false);
    assert!(!active(&fx, vm).clean);
    assert_eq!(
        fx.ctl()
            .admit_snapshot(
                "alice",
                fresh_key(2),
                vm,
                5,
                &Request::Create(None),
                None,
                host(),
                resources()
            )
            .unwrap()
            .err(),
        Some(crate::vm::Refusal::SnapshotOfflineRequired)
    );
    assert_eq!(fx.count("SELECT count(*) FROM vm_snapshot_work"), 0);
    let restore = admit(&fx, vm, 3, &Request::Restore(point.work.target));
    run(&fx, &source, &restore, Case::default()).0.unwrap();
    assert_eq!(active(&fx, vm).pair.0, restore.work.candidate.unwrap());
    assert!(active(&fx, vm).clean);
}

#[test]
fn mid_copy_cancel_and_mismatched_authority_abandon_exact_staging() {
    assert_admission_closed();
    for cancel in [true, false] {
        let fx = SweepFixture::new();
        let source = FakeSource::new(Instant::now());
        let golden = seal_golden(&fx, 16);
        let vm = active_vm(&fx, &golden, 16, "alice", LEASE_MS);
        let baseline = disk_bytes(&fx);
        let admitted = admit(&fx, vm, 1, &Request::Create(None));
        let candidate = admitted.work.candidate.unwrap();
        // The writer was admitted before cancellation. The first payload
        // write grows the candidate, making cancellation deterministic.
        let disk = fx.slot_file(candidate.disk);
        let wrote = Cell::new(false);
        let cancelled = || {
            if cancel && disk.metadata().is_ok_and(|info| info.len() > 0) {
                wrote.set(true);
            }
            wrote.get()
        };
        let mut wrong = admitted.work.source.1[0].pin;
        wrong.sha256[0] ^= 1;
        let (result, seen) = run(
            &fx,
            &source,
            &admitted,
            Case {
                cancelled: &cancelled,
                copy_content: (!cancel).then_some(wrong),
                ..Case::default()
            },
        );
        let error = result.unwrap_err().to_string();
        assert_eq!(wrote.get(), cancel);
        let failed = seen.failed.unwrap();
        if cancel {
            assert_eq!(error, "snapshot copy refused: Cancelled");
            assert!(
                failed
                    .iter()
                    .all(|leaf| matches!(leaf, StagedLeaf::Identified(..)))
            );
        } else {
            assert_eq!(failed, [StagedLeaf::Untouched; 2]);
            assert!(!exists(&disk), "mismatched authority reached slot creation");
        }
        assert_eq!(seen.fail_holds_free, Some(true));
        assert_eq!(fx.count("SELECT count(*) FROM vm_restore_points"), 0);
        sweep(&fx);
        assert_eq!(disk_bytes(&fx), baseline);
        assert_exact_inventory(&fx, &[golden.slots, admitted.work.active.pair.0]);
    }
}

#[test]
fn test_delete_disposes_one_point_and_static_backup_delete_is_refused() {
    assert_admission_closed();
    let fx = SweepFixture::new();
    let source = FakeSource::new(Instant::now());
    let golden = seal_golden(&fx, 17);
    let vm = active_vm(&fx, &golden, 17, "alice", LEASE_MS);
    let baseline = disk_bytes(&fx);
    let point = admit(&fx, vm, 1, &Request::Create(None));
    run(&fx, &source, &point, Case::default()).0.unwrap();
    let pair = point.work.candidate.unwrap();
    let charged = disk_bytes(&fx);
    let delete = admit(&fx, vm, 2, &Request::Delete(point.work.target));
    assert!(delete.work.candidate.is_none());
    assert_eq!(disk_bytes(&fx), charged);
    assert_eq!(receipt_status(&fx, delete.work.activity), 0);
    assert!(exists(&fx.gen_dir(pair.disk.generation)));
    sweep(&fx);
    assert!(!exists(&fx.gen_dir(pair.disk.generation)));
    assert_eq!(
        receipt_status(&fx, delete.work.activity),
        Status::Succeeded.code()
    );
    assert_eq!(disk_bytes(&fx), baseline);
    assert_exact_inventory(&fx, &[golden.slots, active(&fx, vm).pair.0]);

    use crate::vm::static_web::{StaticName, StaticProvenance};
    let static_vm = [0x67; 16];
    let backup = [0x68; 16];
    let slots = fx
        .store()
        .transact(|db| {
            let slots = store::static_vm::reserve(
                db,
                static_vm,
                &StaticName::parse("fixture-static").unwrap(),
                "alice",
                golden.template,
                StaticProvenance {
                    source_generation: pair.disk.generation,
                    recipe_digest: [0x52; 32],
                    disk_digest: [0x44; 32],
                    vars_digest: [0x56; 32],
                },
                crate::vm::DiskClaim {
                    images: 64,
                    ..Default::default()
                },
            )?;
            db.execute(
                "INSERT INTO vm_restore_points
                 (id,static_vm,host,captured_ms,state,provenance,generation,lineage,charge)
             VALUES(?1,?2,(SELECT id FROM vm_state_host WHERE singleton=1),?3,'ready',?4,?4,?5,64)",
                params![backup, static_vm, NOW, slots.disk.generation, [0x4cu8; 32]],
            )?;
            Ok(slots)
        })
        .unwrap();
    let charged = disk_bytes(&fx);
    assert_eq!(
        fx.ctl()
            .foreground_snapshot_precheck("alice", static_vm, &Request::Delete(backup))
            .unwrap(),
        Err(crate::vm::Refusal::StaticBackupDeleteForbidden)
    );
    let guards = GenerationGuards::default();
    let ledger = LedgerDisposal { store: fx.store() };
    let work = CleanupWork {
        kind: CleanupKind::Snapshot {
            vm: static_vm,
            generation: slots.disk.generation,
        },
        pending_worker: None,
        snapshot_phase: Some("delete".into()),
        artifacts: [slots.disk, slots.vars]
            .into_iter()
            .map(|slot| CleanupArtifact {
                generation: slot.generation,
                role: slot.role.into(),
                key: slot.key,
                directory: None,
                identity: None,
                retain: false,
            })
            .collect(),
        accounted: true,
        release_only: false,
    };
    assert_eq!(
        fx.disposal(&ledger, &guards)
            .dispose_cleanup(&work, Instant::now() + Duration::from_secs(1)),
        Err(PATH)
    );
    assert_eq!(
        fx.count("SELECT count(*) FROM vm_restore_points WHERE static_vm IS NOT NULL"),
        1
    );
    assert_eq!(disk_bytes(&fx), charged);
}

#[test]
fn lease_extension_preserves_points_and_refuses_after_expiry_is_claimed() {
    assert_admission_closed();
    let fx = SweepFixture::new();
    let source = FakeSource::new(Instant::now());
    let golden = seal_golden(&fx, 18);
    let vm = active_vm(&fx, &golden, 18, "alice", LEASE_MS);
    let point = admit(&fx, vm, 1, &Request::Create(None));
    run(&fx, &source, &point, Case::default()).0.unwrap();
    let charged = disk_bytes(&fx);
    let seconds = 60 * 60;
    let end = fx
        .ctl()
        .extend_lease("alice", vm, fresh_key(2), seconds)
        .unwrap()
        .unwrap();
    assert_eq!(end, NOW + i64::try_from(seconds).unwrap() * 1000);
    fx.clock.advance(SWEEP_AT_MS, 0);
    sweep(&fx);
    assert!(exists(
        &fx.gen_dir(point.work.candidate.unwrap().disk.generation)
    ));
    assert_eq!(disk_bytes(&fx), charged);
    fx.clock.advance(SWEEP_AT_MS, 0);
    sweep(&fx);
    assert_eq!(
        fx.ctl()
            .extend_lease("alice", vm, fresh_key(3), seconds)
            .unwrap(),
        Err(crate::vm::Refusal::Expired)
    );
}
