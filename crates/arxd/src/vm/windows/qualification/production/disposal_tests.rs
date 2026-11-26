//! Resumable native TEST disposal through real NTFS handles and the VM ledger
//! (T33, T67-T71, I16-I18, I22-I23).
use super::super::tests::{create_dir_sddl, sid_ptr};
use super::*;
use crate::store::vm::{
    self as store, Binding, VmStore,
    native::{
        Origin,
        disposal::{DisposalScope, GenerationState, SlotDisposal},
    },
};
use crate::vm::{
    BuildRequest, Clock, ClockReading, DiskClaim, Guest, PairSlots, RequestId, Status, Subject,
    VmController, VmPolicy,
    paths::ArtifactPin,
    reaper::{CleanupArtifact, CleanupKind, CleanupWork},
    runtime::{Quiesced, ReaperRuntime, StopDisposition},
};
use rusqlite::params;
use std::cell::{Cell, RefCell};
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const NOW: i64 = 1_800_000_000_000;
const CLAIM: u64 = 4096;

#[derive(Clone)]
struct FakeClock(Arc<Mutex<ClockReading>>);

impl FakeClock {
    fn new(utc_ms: i64) -> Self {
        Self(Arc::new(Mutex::new(ClockReading {
            utc_ms,
            monotonic: Instant::now(),
        })))
    }

    fn advance(&self, utc_ms: i64, secs: u64) {
        let mut guard = self.0.lock().unwrap();
        guard.utc_ms += utc_ms;
        let stepped = guard.monotonic + Duration::from_secs(secs);
        guard.monotonic = stepped.max(Instant::now() + Duration::from_secs(secs));
    }
}

impl Clock for FakeClock {
    fn now(&self) -> ClockReading {
        *self.0.lock().unwrap()
    }
}

struct OfflineReaper;

impl ReaperRuntime for OfflineReaper {
    fn quiesce(&self, _: &CleanupWork, _: Instant) -> Quiesced {
        Quiesced::Offline
    }

    fn graceful_stop(&self, _: &CleanupWork, _: Instant) -> StopDisposition {
        StopDisposition::ChargedQuarantine
    }

    fn force_reap(&self, _: &CleanupWork, _: Instant) -> StopDisposition {
        StopDisposition::ChargedQuarantine
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Fault {
    /// Fail before the Nth (1-based) call to `step`.
    Before(&'static str, usize),
    /// Commit the Nth (1-based) call to `step`, then simulate lost ack / crash.
    After(&'static str, usize),
}

/// Real `LedgerDisposal` wrapper that logs every durable checkpoint and can
/// inject a crash before or after any Nth checkpoint call.
struct FaultDisposal<'a> {
    inner: LedgerDisposal<'a>,
    fault: Cell<Option<Fault>>,
    counts: RefCell<std::collections::HashMap<&'static str, usize>>,
    log: RefCell<Vec<&'static str>>,
}

impl<'a> FaultDisposal<'a> {
    fn new(store: &'a VmStore, fault: Option<Fault>) -> Self {
        Self {
            inner: LedgerDisposal { store },
            fault: Cell::new(fault),
            counts: RefCell::new(std::collections::HashMap::new()),
            log: RefCell::new(Vec::new()),
        }
    }

    fn step<T>(&self, name: &'static str, commit: impl FnOnce() -> Result<T, Q>) -> Result<T, Q> {
        let count = {
            let mut counts = self.counts.borrow_mut();
            let entry = counts.entry(name).or_insert(0);
            *entry += 1;
            *entry
        };
        self.log.borrow_mut().push(name);
        if self.fault.get() == Some(Fault::Before(name, count)) {
            return Err(PATH);
        }
        let result = commit()?;
        if self.fault.get() == Some(Fault::After(name, count)) {
            return Err(PATH);
        }
        Ok(result)
    }
}

impl DisposalCheckpoints for FaultDisposal<'_> {
    fn generation_state(&self, generation: Id) -> Result<GenerationState, Q> {
        self.inner.generation_state(generation)
    }

    fn slot_disposal(&self, slot: ArtifactSlot) -> Result<(Leaf, SlotDisposal), Q> {
        self.inner.slot_disposal(slot)
    }

    fn settle_workspace_intent(
        &self,
        generation: Id,
        leaf: WorkspaceLeaf,
        presence: Presence,
    ) -> Result<(), Q> {
        self.step("settle_workspace_intent", || {
            self.inner
                .settle_workspace_intent(generation, leaf, presence)
        })
    }

    fn settle_slot_intent(
        &self,
        scope: native::Scope,
        slot: ArtifactSlot,
        present: bool,
    ) -> Result<(), Q> {
        self.step("settle_slot_intent", || {
            self.inner.settle_slot_intent(scope, slot, present)
        })
    }

    fn intend_slot_disposal(
        &self,
        auth: DisposalScope,
        scope: native::Scope,
        slot: ArtifactSlot,
        identity: FileIdentity,
    ) -> Result<(), Q> {
        self.step("intend_slot_disposal", || {
            self.inner.intend_slot_disposal(auth, scope, slot, identity)
        })
    }

    fn slot_disposed(
        &self,
        auth: DisposalScope,
        scope: native::Scope,
        slot: ArtifactSlot,
        identity: FileIdentity,
    ) -> Result<(), Q> {
        self.step("slot_disposed", || {
            self.inner.slot_disposed(auth, scope, slot, identity)
        })
    }

    fn intend_lease_disposal(&self, auth: DisposalScope, scope: native::Scope) -> Result<(), Q> {
        self.step("intend_lease_disposal", || {
            self.inner.intend_lease_disposal(auth, scope)
        })
    }

    fn intend_directory_disposal(
        &self,
        auth: DisposalScope,
        scope: native::Scope,
    ) -> Result<(), Q> {
        self.step("intend_directory_disposal", || {
            self.inner.intend_directory_disposal(auth, scope)
        })
    }

    fn generation_disposed(&self, auth: DisposalScope, scope: native::Scope) -> Result<(), Q> {
        self.step("generation_disposed", || {
            self.inner.generation_disposed(auth, scope)
        })
    }
}

/// Real `VmController` + private NTFS production root for end-to-end sweep and
/// crash-reopen tests with zero volatile state across reopens.
struct SweepFixture {
    ctl: Option<VmController<FakeClock>>,
    clock: FakeClock,
    production: PathPin,
    owner: Vec<usize>,
    owner_text: String,
    temp: tempfile::TempDir,
    state: tempfile::TempDir,
    state_in_production: bool,
}

impl SweepFixture {
    fn new() -> Self {
        Self::new_with_store(false)
    }

    fn new_with_store(state_in_production: bool) -> Self {
        let temp = tempfile::tempdir_in(std::env::temp_dir()).unwrap();
        let (owner, owner_text) = default_owner_sid();
        let production = create_dir_sddl(
            temp.path(),
            "production",
            &format!("O:{owner_text}D:P(A;OICI;FA;;;{owner_text})"),
        );
        let production = PathPin::open(&production, true, false).unwrap();
        let state = crate::service::application::tests::private_temp();
        let clock = FakeClock::new(NOW);
        let root = if state_in_production {
            Path::new(production.path.as_str())
        } else {
            state.path()
        };
        let ctl = VmController::open(root, VmPolicy::default(), clock.clone()).unwrap();
        Self {
            ctl: Some(ctl),
            clock,
            production,
            owner,
            owner_text,
            temp,
            state,
            state_in_production,
        }
    }

    fn ctl(&self) -> &VmController<FakeClock> {
        self.ctl.as_ref().unwrap()
    }

    fn store(&self) -> &VmStore {
        self.ctl().reaper_test_store()
    }

    /// Reopens a brand-new `VmController` and `VmStore` from disk with zero
    /// volatile caches, guards or sweep state.
    fn reopen(&mut self) {
        drop(self.ctl.take());
        self.ctl = Some(
            VmController::open(self.state_root(), VmPolicy::default(), self.clock.clone()).unwrap(),
        );
    }

    fn state_root(&self) -> &Path {
        if self.state_in_production {
            Path::new(self.production.path.as_str())
        } else {
            self.state.path()
        }
    }

    fn owner_sid(&self) -> PSID {
        sid_ptr(&self.owner)
    }

    fn gen_dir(&self, generation: Id) -> PathBuf {
        Path::new(self.production.path.as_str()).join(hex::encode(generation))
    }

    fn slot_file(&self, slot: ArtifactSlot) -> PathBuf {
        self.gen_dir(slot.generation).join(leaf_name(slot))
    }

    fn lease_file(&self, generation: Id) -> PathBuf {
        self.gen_dir(generation).join(GENERATION_LEASE)
    }

    fn disposal<'a>(
        &'a self,
        ledger: &'a dyn DisposalCheckpoints,
        guards: &'a GenerationGuards,
    ) -> NativeDisposal<'a> {
        NativeDisposal::under(&self.production, self.owner_sid(), ledger, guards)
    }

    fn sweep(&self, fault: Option<Fault>) -> bool {
        let ledger = FaultDisposal::new(self.store(), fault);
        let guards = GenerationGuards::default();
        let custody = self.disposal(&ledger, &guards);
        self.ctl().sweep_with(&OfflineReaper, &custody).unwrap()
    }

    fn count(&self, sql: &str) -> i64 {
        self.store()
            .transact(|db| store::count(db, sql, []))
            .unwrap()
    }

    fn stage_slot(
        &self,
        ws: &ProductionWorkspace,
        ledger: &LedgerCheckpoints<'_>,
        slot: ArtifactSlot,
        bytes: &[u8],
    ) -> ArtifactEvidence {
        let mut writer = ws.create_slot(slot, ledger).unwrap();
        writer.write_all(bytes).unwrap();
        writer.sync().unwrap();
        let ev = ArtifactEvidence {
            identity: writer.identity(),
            directory: ws.binding().workspace.file,
            pin: ArtifactPin {
                size: bytes.len() as u64,
                sha256: crate::vm::qualification_record::digest(bytes),
            },
        };
        drop(writer);
        ledger.slot_verified(ws.binding(), slot, ev).unwrap();
        self.store()
            .transact(|db| store::identify(db, slot, ev))
            .unwrap();
        ev
    }

    /// Reserves and stages a full native build (primary disk/vars/seed +
    /// verification overlay/vars) with both bound workspaces on NTFS.
    fn stage_build(&self, nonce: u8) -> (i64, BuildSlots, [ArtifactEvidence; 2]) {
        let request = build_request(nonce);
        let (activity, slots) = self
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
                let slots = store::reserve_build(db, request, operation, CLAIM * 2)?;
                Ok((activity, slots))
            })
            .unwrap();
        let ledger = LedgerCheckpoints {
            store: self.store(),
            origin: Origin::Build,
        };
        let primary_ws = reserve_workspace_under(
            &self.production,
            self.owner_sid(),
            slots.pair.disk.generation,
            &ledger,
        )
        .unwrap();
        let disk_ev = self.stage_slot(&primary_ws, &ledger, slots.pair.disk, b"golden-disk");
        let vars_ev = self.stage_slot(&primary_ws, &ledger, slots.pair.vars, b"golden-vars");
        let _seed_ev = self.stage_slot(&primary_ws, &ledger, slots.seed, b"build-seed");
        drop(primary_ws);

        let verify_ws = reserve_workspace_under(
            &self.production,
            self.owner_sid(),
            slots.verification.disk.generation,
            &ledger,
        )
        .unwrap();
        let _vo = self.stage_slot(&verify_ws, &ledger, slots.verification.disk, b"verify-ovl");
        let _vv = self.stage_slot(&verify_ws, &ledger, slots.verification.vars, b"verify-vars");
        drop(verify_ws);

        (activity, slots, [disk_ev, vars_ev])
    }

    /// Reserves, stages and activates a native TEST clone VM whose lease
    /// expires at `NOW + 1000`.
    fn active_clone(&self, nonce: u8) -> (Id, PairSlots, [ArtifactEvidence; 2]) {
        let mut template = [0x80u8; 16];
        template[15] = nonce;
        let mut vm = [0xa0u8; 16];
        vm[15] = nonce;
        let (activity, slots) = self
            .store()
            .transact(|db| {
                db.execute(
                    "INSERT OR IGNORE INTO vm_templates(id,guest,recipe_digest,disk_digest,vars_digest,bytes)
                     VALUES(?1,1,?2,?2,?2,1)",
                    params![template, [nonce; 32]],
                )?;
                db.execute(
                    "INSERT INTO vm_instances(id,template,principal,expires_ms)
                     VALUES(?1,?2,'alice',?3)",
                    params![vm, template, NOW + 1_000],
                )?;
                let activity = Binding {
                    principal: "alice",
                    key: RequestId::parse(&format!("{:032x}", 0x200 + u32::from(nonce))).unwrap(),
                    subject: Subject::Template(template, [nonce; 32]),
                    action: 1,
                }
                .record(db, NOW, Some(vm), Some(NOW + 1_000))?
                .receipt
                .activity_id;
                db.execute(
                    "INSERT INTO vm_disk_claims(activity_id,vm,persistent,staging)
                     VALUES(?1,?2,?3,0)",
                    params![activity, vm, CLAIM],
                )?;
                Ok((activity, store::plan_clone(db, vm)?))
            })
            .unwrap();
        let ledger = LedgerCheckpoints {
            store: self.store(),
            origin: Origin::Clone,
        };
        let ws = reserve_workspace_under(
            &self.production,
            self.owner_sid(),
            slots.disk.generation,
            &ledger,
        )
        .unwrap();
        let ev = [
            self.stage_slot(&ws, &ledger, slots.disk, b"clone-disk"),
            self.stage_slot(&ws, &ledger, slots.vars, b"clone-vars"),
        ];
        drop(ws);
        self.store()
            .transact(|db| {
                assert_eq!(
                    store::settle_clone(db, activity, slots, Some(ev), true, NOW, None)?,
                    Status::Succeeded
                );
                Ok(())
            })
            .unwrap();
        (vm, slots, ev)
    }
}

fn default_owner_sid() -> (Vec<usize>, String) {
    use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
    let token = process_token(unsafe { GetCurrentProcess() }, TOKEN_QUERY).unwrap();
    let data = token_data(&token, TokenOwner).unwrap();
    let sid = unsafe { (*data.as_ptr().cast::<TOKEN_OWNER>()).Owner };
    let mut text = std::ptr::null_mut();
    assert_ne!(unsafe { ConvertSidToStringSidW(sid, &mut text) }, 0);
    let len = (0..).take_while(|&i| unsafe { *text.add(i) } != 0).count();
    let string = String::from_utf16(unsafe { std::slice::from_raw_parts(text, len) }).unwrap();
    unsafe { LocalFree(text.cast()) };
    (data, string)
}

fn build_request(nonce: u8) -> BuildRequest<'static> {
    BuildRequest {
        principal: "alice",
        key: RequestId::parse(&format!("{nonce:032x}")).unwrap(),
        recipe: "fece98c2030f405fb6402fc92a79935d",
        digest: [nonce; 32],
        retained: DiskClaim {
            images: CLAIM,
            metadata: 0,
            staging: 0,
        },
        transient: DiskClaim {
            images: CLAIM,
            metadata: 0,
            staging: 0,
        },
    }
}

fn exists(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

/// T33: transient-build disposal and ordinary sweep use the same resumable
/// interface. Interrupting after EVERY unlink/commit edge (seed, verification
/// overlay, verification vars, verification lease, verification dir, before
/// publish) converges cleanly both via retry-to-publish and via crash/reopen +
/// ordinary sweep.
#[test]
fn t33_transient_build_disposal_resumes_across_every_edge_to_publish_and_ordinary_sweep() {
    let faults = [
        Fault::After("intend_slot_disposal", 1),
        Fault::Before("slot_disposed", 1),
        Fault::After("intend_slot_disposal", 2),
        Fault::Before("slot_disposed", 2),
        Fault::After("intend_slot_disposal", 3),
        Fault::Before("slot_disposed", 3),
        Fault::After("intend_lease_disposal", 1),
        Fault::Before("intend_directory_disposal", 1),
        Fault::After("intend_directory_disposal", 1),
        Fault::Before("generation_disposed", 1),
        Fault::After("generation_disposed", 1),
    ];

    // Path A: interrupt, drop all handles/state, resume transient disposal, then publish_build.
    for (idx, fault) in faults.into_iter().enumerate() {
        let fx = SweepFixture::new();
        let (activity, slots, pair_ev) = fx.stage_build(idx as u8 + 1);
        let deadline = Instant::now() + Duration::from_secs(30);
        let guards = GenerationGuards::default();
        let faulty = FaultDisposal::new(fx.store(), Some(fault));
        assert_eq!(
            fx.disposal(&faulty, &guards)
                .dispose_build_transient(activity, slots, deadline),
            Err(PATH),
            "fault {fault:?} must interrupt"
        );
        // Primary pair and primary lease/dir are ALWAYS untouched.
        assert!(exists(&fx.slot_file(slots.pair.disk)));
        assert!(exists(&fx.slot_file(slots.pair.vars)));
        assert!(exists(&fx.lease_file(slots.pair.disk.generation)));

        // Retry with fresh guards and no volatile state -> converges cleanly.
        let clean = LedgerDisposal { store: fx.store() };
        let fresh_guards = GenerationGuards::default();
        fx.disposal(&clean, &fresh_guards)
            .dispose_build_transient(activity, slots, deadline)
            .unwrap();
        assert!(!exists(&fx.slot_file(slots.seed)));
        assert!(!exists(&fx.gen_dir(slots.verification.disk.generation)));
        assert!(exists(&fx.slot_file(slots.pair.disk)));
        assert!(exists(&fx.slot_file(slots.pair.vars)));
        assert!(exists(&fx.lease_file(slots.pair.disk.generation)));

        fx.store()
            .transact(|db| {
                store::publish_build(
                    db,
                    activity,
                    slots,
                    build_request(idx as u8 + 1),
                    Guest::UbuntuX86_64,
                    pair_ev,
                    NOW,
                )
            })
            .unwrap();
        assert_eq!(
            fx.count("SELECT count(*) FROM vm_templates"),
            1,
            "golden published after {fault:?}"
        );
        assert_eq!(
            fx.count("SELECT count(*) FROM vm_native_generations"),
            1,
            "only sealed primary generation retains native custody"
        );
    }

    // Path B: interrupt after seed/verification unlinks, then simulate daemon
    // crash + reopen (`recover()` abandons the pending build) and let the
    // ordinary `VmController::sweep_with` converge to zero residue and zero claim.
    for (idx, fault) in faults.into_iter().enumerate() {
        let mut fx = SweepFixture::new();
        let (activity, slots, _) = fx.stage_build(0x40 + idx as u8);
        let deadline = Instant::now() + Duration::from_secs(30);
        let guards = GenerationGuards::default();
        let faulty = FaultDisposal::new(fx.store(), Some(fault));
        assert!(
            fx.disposal(&faulty, &guards)
                .dispose_build_transient(activity, slots, deadline)
                .is_err()
        );
        // Crash & reopen: zero volatile state, `recover()` abandons pending build.
        fx.reopen();
        assert_eq!(fx.count("SELECT count(*) FROM vm_build_intents"), 1);
        fx.clock.advance(0, 0);
        assert!(fx.sweep(None));
        assert_eq!(
            fx.count("SELECT count(*) FROM vm_build_intents"),
            0,
            "ordinary sweep settled abandoned build after {fault:?}"
        );
        assert_eq!(fx.count("SELECT count(*) FROM vm_native_generations"), 0);
        assert_eq!(fx.count("SELECT count(*) FROM vm_native_slots"), 0);
        assert!(!exists(&fx.gen_dir(slots.pair.disk.generation)));
        assert!(!exists(&fx.gen_dir(slots.verification.disk.generation)));
    }
}

/// T67, T69, T70, T71: interrupt ordinary expiry sweep after EVERY unlink and
/// commit boundary (disk intent, disk unlink, vars intent, vars unlink, lease
/// intent, lease unlink, directory intent, directory unlink, before settle),
/// reopen fresh `VmStore`/`VmController` with ZERO volatile state, and verify
/// ordinary `sweep_with` converges to exact physical absence, tombstone, and
/// freed claims.
#[test]
fn t69_t70_t71_ordinary_sweep_resumes_after_every_unlink_and_reopen() {
    let faults = [
        Fault::After("intend_slot_disposal", 1),
        Fault::Before("slot_disposed", 1),
        Fault::After("slot_disposed", 1),
        Fault::After("intend_slot_disposal", 2),
        Fault::Before("slot_disposed", 2),
        Fault::After("intend_lease_disposal", 1),
        Fault::Before("intend_directory_disposal", 1),
        Fault::After("intend_directory_disposal", 1),
        Fault::Before("generation_disposed", 1),
        Fault::After("generation_disposed", 1),
    ];
    for (idx, fault) in faults.into_iter().enumerate() {
        let mut fx = SweepFixture::new();
        let (vm, slots, _) = fx.active_clone(idx as u8 + 1);
        fx.clock.advance(2_000, 10);

        // First sweep interrupts at `fault`. Claim MUST stay armed.
        assert!(fx.sweep(Some(fault)));
        assert_eq!(
            fx.count("SELECT count(*) FROM vm_disk_claims"),
            1,
            "claim retained across interruption {fault:?}"
        );
        assert_eq!(fx.count("SELECT count(*) FROM vm_instances"), 1);

        // Crash and reopen with zero volatile receipts, guards or backoff maps.
        fx.reopen();
        fx.clock.advance(0, 0);
        assert!(fx.sweep(None));

        // Exact physical absence (leaves, lease AND directory) and ledger settlement.
        assert!(!exists(&fx.slot_file(slots.disk)));
        assert!(!exists(&fx.slot_file(slots.vars)));
        assert!(!exists(&fx.lease_file(slots.disk.generation)));
        assert!(!exists(&fx.gen_dir(slots.disk.generation)));
        assert_eq!(fx.count("SELECT count(*) FROM vm_disk_claims"), 0);
        assert_eq!(fx.count("SELECT count(*) FROM vm_instances"), 0);
        assert_eq!(fx.count("SELECT count(*) FROM vm_generations"), 0);
        assert_eq!(fx.count("SELECT count(*) FROM vm_native_generations"), 0);
        assert_eq!(fx.count("SELECT count(*) FROM vm_native_slots"), 0);
        assert_eq!(
            fx.store()
                .transact(|db| store::tombstone_proof(db, "alice", vm))
                .unwrap()
                .and_then(|(_, confirmed)| confirmed),
            Some(true),
            "expiry tombstone proved after {fault:?}"
        );
    }
}

/// T70 (>128 disposals): prove progress is durable in SQLite with no volatile
/// receipt cap by interrupting and resuming >128 distinct slots across reopens.
#[test]
fn t70_more_than_128_disposals_converge_without_volatile_cache() {
    let mut fx = SweepFixture::new();
    // 65 expired clones * 2 slots each = 130 distinct leaf disposals + 65 leases + 65 dirs.
    let mut gens = Vec::new();
    for i in 0u8..65 {
        let (_, slots, _) = fx.active_clone(i);
        gens.push(slots.disk.generation);
    }
    fx.clock.advance(2_000, 10);
    // Interrupt after the first unlink, reopen, then drain all 65 through ordinary sweeps
    // (cleanup_kinds caps at 8 per sweep, exercising multiple turns and reopens).
    assert!(fx.sweep(Some(Fault::Before("slot_disposed", 1))));
    fx.reopen();
    for _ in 0..12 {
        fx.clock.advance(0, 1);
        assert!(fx.sweep(None));
        if fx.count("SELECT count(*) FROM vm_instances") == 0 {
            break;
        }
    }
    assert_eq!(fx.count("SELECT count(*) FROM vm_instances"), 0);
    assert_eq!(fx.count("SELECT count(*) FROM vm_disk_claims"), 0);
    assert_eq!(fx.count("SELECT count(*) FROM vm_native_generations"), 0);
    for generation in gens {
        assert!(!exists(&fx.gen_dir(generation)));
    }
}

/// T68 negative controls: busy writer, replaced leaf (before and after intent),
/// unexpected extra sibling (before lease and before dir), arbitrary absence,
/// wrong parent, failed settlement, unchanged golden, foreign/hardlink, Static
/// VM/role and backup object.
#[test]
fn t68_negative_controls_retain_claims_and_never_delete_protected_objects() {
    // 1. Busy writer (even sharing READ | WRITE | DELETE) refuses disposal;
    //    releasing the writer lets the next sweep succeed.
    {
        let fx = SweepFixture::new();
        let (_, slots, _) = fx.active_clone(1);
        fx.clock.advance(2_000, 10);
        let disk_path = fx.slot_file(slots.disk);
        let writer = std::fs::OpenOptions::new()
            .write(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .open(&disk_path)
            .unwrap();
        assert!(fx.sweep(None));
        assert!(exists(&disk_path));
        assert_eq!(fx.count("SELECT count(*) FROM vm_disk_claims"), 1);
        drop(writer);
        fx.clock.advance(0, 61);
        assert!(fx.sweep(None));
        assert!(!exists(&fx.gen_dir(slots.disk.generation)));
        assert_eq!(fx.count("SELECT count(*) FROM vm_disk_claims"), 0);
    }

    // 2. Replaced leaf BEFORE intent AND AFTER intent+unlink -> quarantined,
    //    replacement bytes untouched, claim retained.
    for after_intent in [false, true] {
        let mut fx = SweepFixture::new();
        let (_, slots, _) = fx.active_clone(2);
        fx.clock.advance(2_000, 10);
        let disk_path = fx.slot_file(slots.disk);
        if after_intent {
            // Interrupt right after disk unlink, before slot_disposed commits.
            assert!(fx.sweep(Some(Fault::Before("slot_disposed", 1))));
            assert!(!exists(&disk_path));
            fx.reopen();
            fx.clock.advance(0, 0);
        } else {
            std::fs::remove_file(&disk_path).unwrap();
        }
        std::fs::write(&disk_path, b"replacement-payload").unwrap();
        assert!(fx.sweep(None));
        assert_eq!(std::fs::read(&disk_path).unwrap(), b"replacement-payload");
        assert_eq!(fx.count("SELECT count(*) FROM vm_disk_claims"), 1);
    }

    // 3. Unexpected extra sibling BEFORE lease disposal AND AFTER lease unlink
    //    before directory disposal -> refused, sibling untouched, claim retained.
    for after_lease in [false, true] {
        let mut fx = SweepFixture::new();
        let (_, slots, _) = fx.active_clone(3);
        fx.clock.advance(2_000, 10);
        let extra = fx
            .gen_dir(slots.disk.generation)
            .join("unexpected-sibling.txt");
        if after_lease {
            assert!(fx.sweep(Some(Fault::Before("intend_directory_disposal", 1))));
            assert!(!exists(&fx.lease_file(slots.disk.generation)));
            fx.reopen();
            fx.clock.advance(0, 0);
        }
        std::fs::write(&extra, b"keep-me").unwrap();
        assert!(fx.sweep(None));
        assert_eq!(std::fs::read(&extra).unwrap(), b"keep-me");
        assert_eq!(fx.count("SELECT count(*) FROM vm_disk_claims"), 1);
        if !after_lease {
            // Lease itself was not deleted and generation never entered lease disposal.
            assert!(exists(&fx.lease_file(slots.disk.generation)));
            assert_eq!(
                fx.count("SELECT count(*) FROM vm_native_generations WHERE disposal IS NULL"),
                1
            );
        }
    }

    // 4. Arbitrary absence WITHOUT durable intent (leaf deleted externally while
    //    disposal IS NULL) -> refused, claim retained.
    {
        let fx = SweepFixture::new();
        let (_, slots, _) = fx.active_clone(4);
        fx.clock.advance(2_000, 10);
        std::fs::remove_file(fx.slot_file(slots.disk)).unwrap();
        assert!(fx.sweep(None));
        assert_eq!(fx.count("SELECT count(*) FROM vm_disk_claims"), 1);
        assert!(exists(&fx.lease_file(slots.disk.generation)));
    }

    // 5. Wrong parent / production root -> refused, original files untouched.
    {
        let fx = SweepFixture::new();
        let (_, slots, _) = fx.active_clone(5);
        fx.clock.advance(2_000, 10);
        let other_dir = create_dir_sddl(
            fx.temp.path(),
            "wrong-parent",
            &format!("O:{}D:P(A;OICI;FA;;;{})", fx.owner_text, fx.owner_text),
        );
        let wrong_parent = PathPin::open(&other_dir, true, false).unwrap();
        let ledger = LedgerDisposal { store: fx.store() };
        let guards = GenerationGuards::default();
        let wrong_custody = NativeDisposal::under(&wrong_parent, fx.owner_sid(), &ledger, &guards);
        fx.clock.advance(0, 0);
        assert!(fx.ctl().sweep_with(&OfflineReaper, &wrong_custody).unwrap());
        assert!(exists(&fx.slot_file(slots.disk)));
        assert_eq!(fx.count("SELECT count(*) FROM vm_disk_claims"), 1);
    }

    // 6. Failed settlement (SQL error during settle_cleanup) -> claim stays
    //    armed; removing the trigger and retrying after backoff settles cleanly.
    {
        let fx = SweepFixture::new();
        let (_, slots, _) = fx.active_clone(6);
        fx.clock.advance(2_000, 10);
        fx.store()
            .transact(|db| {
                db.execute_batch(
                    "CREATE TEMP TRIGGER fail_settle BEFORE INSERT ON vm_tombstones
                     BEGIN SELECT RAISE(ABORT,'injected settlement failure'); END;",
                )?;
                Ok(())
            })
            .unwrap();
        assert!(fx.sweep(None));
        assert_eq!(fx.count("SELECT count(*) FROM vm_disk_claims"), 1);
        assert_eq!(fx.count("SELECT count(*) FROM vm_instances"), 1);
        fx.store()
            .transact(|db| {
                db.execute_batch("DROP TRIGGER temp.fail_settle;")?;
                Ok(())
            })
            .unwrap();
        fx.clock.advance(0, 61);
        assert!(fx.sweep(None));
        assert_eq!(fx.count("SELECT count(*) FROM vm_disk_claims"), 0);
        assert_eq!(fx.count("SELECT count(*) FROM vm_instances"), 0);
        assert!(!exists(&fx.gen_dir(slots.disk.generation)));
    }

    // 7. Unchanged sealed golden, foreign object, Static VM/roles, and backup
    //    object stay untouched and outside deletion authority.
    {
        let fx = SweepFixture::new();
        let (activity, build_slots, pair_ev) = fx.stage_build(7);
        let ledger = LedgerDisposal { store: fx.store() };
        let guards = GenerationGuards::default();
        let disposal = fx.disposal(&ledger, &guards);
        disposal
            .dispose_build_transient(
                activity,
                build_slots,
                Instant::now() + Duration::from_secs(30),
            )
            .unwrap();
        fx.store()
            .transact(|db| {
                store::publish_build(
                    db,
                    activity,
                    build_slots,
                    build_request(7),
                    Guest::UbuntuX86_64,
                    pair_ev,
                    NOW,
                )
            })
            .unwrap();
        let golden_disk = fx.slot_file(build_slots.pair.disk);
        let golden_vars = fx.slot_file(build_slots.pair.vars);
        let golden_lease = fx.lease_file(build_slots.pair.disk.generation);
        let foreign = Path::new(fx.production.path.as_str()).join("foreign-object.bin");
        std::fs::write(&foreign, b"foreign").unwrap();

        // Direct attempt to dispose sealed golden via crafted CleanupWork fails.
        let crafted_golden = CleanupWork {
            kind: CleanupKind::Build(build_slots.id),
            pending_worker: None,
            snapshot_phase: None,
            artifacts: vec![CleanupArtifact {
                generation: build_slots.pair.disk.generation,
                role: "disk".into(),
                key: build_slots.pair.disk.key,
                directory: Some(pair_ev[0].directory),
                identity: Some(pair_ev[0].identity),
                retain: false,
            }],
            accounted: true,
            release_only: false,
        };
        assert_eq!(
            disposal.dispose_cleanup(&crafted_golden, Instant::now() + Duration::from_secs(10)),
            Err(PATH)
        );

        // Crafted Static VM / static_disk work is strictly refused.
        let static_work = CleanupWork {
            kind: CleanupKind::Snapshot {
                vm: [0x51; 16],
                generation: [0x52; 16],
            },
            pending_worker: None,
            snapshot_phase: Some("abandoned".into()),
            artifacts: vec![CleanupArtifact {
                generation: [0x52; 16],
                role: "static_disk".into(),
                key: [0x53; 16],
                directory: None,
                identity: None,
                retain: false,
            }],
            accounted: true,
            release_only: false,
        };
        assert_eq!(
            disposal.dispose_cleanup(&static_work, Instant::now() + Duration::from_secs(10)),
            Err(PATH)
        );

        // Backup owner fixture stays outside TEST deletion authority.
        let (_bdir, bstore, btarget, bgen) = store::backup_owner_fixture().unwrap();
        let backup_ledger = LedgerDisposal { store: &bstore };
        let backup_disposal = fx.disposal(&backup_ledger, &guards);
        let backup_work = CleanupWork {
            kind: CleanupKind::Snapshot {
                vm: btarget.vm,
                generation: bgen,
            },
            pending_worker: None,
            snapshot_phase: Some("abandoned".into()),
            artifacts: vec![CleanupArtifact {
                generation: bgen,
                role: "static_disk".into(),
                key: [0x54; 16],
                directory: None,
                identity: None,
                retain: false,
            }],
            accounted: true,
            release_only: false,
        };
        assert_eq!(
            backup_disposal.dispose_cleanup(&backup_work, Instant::now() + Duration::from_secs(10)),
            Err(PATH)
        );

        // Golden and foreign objects remain byte-for-byte intact.
        assert_eq!(std::fs::read(&golden_disk).unwrap(), b"golden-disk");
        assert_eq!(std::fs::read(&golden_vars).unwrap(), b"golden-vars");
        assert!(exists(&golden_lease));
        assert_eq!(std::fs::read(&foreign).unwrap(), b"foreign");
    }
}

/// Recovery-only work verifies retained TEST pairs without deleting anything,
/// release-only expiry carries no deletion permit, and an uncreated reservation
/// settles without touching disk.
#[test]
fn recovery_retains_files_release_only_refuses_and_uncreated_reservation_proves_absent() {
    let fx = SweepFixture::new();
    let (vm, slots, _) = fx.active_clone(9);
    // Enter recovery via daemon restart recovery of a running VM (`state=2`):
    // verify active pair is kept intact and recovery settles.
    fx.store()
        .transact(|db| {
            db.execute("UPDATE vm_instances SET state=2 WHERE id=?1", [vm])?;
            store::recover(db, NOW)
        })
        .unwrap();
    fx.clock.advance(0, 0);
    assert!(fx.sweep(None));
    assert!(exists(&fx.slot_file(slots.disk)));
    assert!(exists(&fx.slot_file(slots.vars)));
    assert!(exists(&fx.lease_file(slots.disk.generation)));
    assert_eq!(fx.count("SELECT count(*) FROM vm_disk_claims"), 1);
    assert_eq!(
        fx.store()
            .transact(|db| Ok(db.query_row(
                "SELECT state FROM vm_instances WHERE id=?1",
                [vm],
                |r| r.get::<_, i64>(0)
            )?))
            .unwrap(),
        1
    );

    // Uncreated build reservation (primary `intended` + absent, verification
    // unreserved + absent): ordinary sweep settles without any delete call.
    let request = build_request(10);
    let (act, uncreated) = fx
        .store()
        .transact(|db| {
            let op = Binding {
                principal: request.principal,
                key: request.key,
                subject: Subject::Recipe(request.recipe, Some(request.digest)),
                action: 11,
            }
            .record(db, NOW, None, None)?;
            let act = op.receipt.activity_id;
            let slots = store::reserve_build(db, request, op, CLAIM)?;
            native::intend_workspace(
                db,
                slots.pair.disk.generation,
                Origin::Build,
                fx.production.identity,
            )?;
            store::abandon_build(db, slots.id, act, NOW)?;
            Ok((act, slots))
        })
        .unwrap();
    let _ = act;
    assert!(!exists(&fx.gen_dir(uncreated.pair.disk.generation)));
    fx.clock.advance(0, 0);
    assert!(fx.sweep(None));
    assert_eq!(fx.count("SELECT count(*) FROM vm_build_intents"), 0);

    // Release-only work carries no deletion permit even if passed directly.
    let ledger = LedgerDisposal { store: fx.store() };
    let guards = GenerationGuards::default();
    let disposal = fx.disposal(&ledger, &guards);
    let release_only = CleanupWork {
        kind: CleanupKind::Expiry { vm, activity_id: 1 },
        pending_worker: None,
        snapshot_phase: None,
        artifacts: Vec::new(),
        accounted: false,
        release_only: true,
    };
    assert_eq!(
        disposal.dispose_cleanup(&release_only, Instant::now() + Duration::from_secs(10)),
        Err(PATH)
    );
}

/// Generation-level guard serializes concurrent disposal on the same generation.
#[test]
fn generation_guard_blocks_concurrent_disposal_of_same_generation() {
    let guards = GenerationGuards::default();
    let g = [0x77; 16];
    let held = guards.acquire(g).unwrap();
    assert!(guards.acquire(g).is_err());
    drop(held);
    assert!(guards.acquire(g).is_ok());
}

/// Production TEST admission and default reaper custody stay closed after chunk 07.
#[test]
fn resumable_disposal_leaves_production_test_admission_and_default_sweep_closed() {
    use crate::vm::runtime::{CandidateRuntime, NativeMutation};
    let fx = SweepFixture::new();
    let (_, slots, _) = fx.active_clone(11);
    fx.clock.advance(2_000, 10);
    let profile = crate::config::VmRuntimeProfile::parse(
        br#"{
        "enabled":true,"owner_sid":"S-1-5-21-1-2-3-1001","supervisor_sid":"S-1-5-21-1-2-3-1002",
        "bundle":"E:/pinned bundle","volume":"D:/","state":"D:/test_vms/state",
        "disk_ceiling_bytes":10737418240,"manifest":"qemu-11.1.3-windows-x64"
    }"#,
    )
    .unwrap();
    let candidate = CandidateRuntime::new(Some(&profile));
    // Default `VmController::sweep_once` uses `ClosedCustody`, never native disposal.
    assert!(fx.ctl().sweep_once(&candidate).unwrap());
    assert!(exists(&fx.slot_file(slots.disk)));
    assert_eq!(fx.count("SELECT count(*) FROM vm_disk_claims"), 1);

    for mutation in [
        NativeMutation::CopyMedia,
        NativeMutation::Boot,
        NativeMutation::CreateImage,
    ] {
        assert_eq!(
            candidate.dispatch::<()>(mutation, |_| {
                panic!("resumable disposal opened a production TEST effect")
            }),
            Err(NativeRefusal::QualificationMissing)
        );
    }
}

/// Reviewer lead (PR #372): a resumed lease or directory deletion must
/// re-check the current `DisposalScope` before the destructive operation.
#[test]
fn review_resumed_lease_and_directory_deletion_rechecks_authority() {
    for (nonce, fault, lease_phase) in [
        (0x71u8, Fault::After("intend_lease_disposal", 1), true),
        (0x72u8, Fault::After("intend_directory_disposal", 1), false),
    ] {
        let fx = SweepFixture::new();
        let (activity, slots, _) = fx.stage_build(nonce);
        let deadline = Instant::now() + Duration::from_secs(30);
        let verification = slots.verification.disk.generation;
        {
            let guards = GenerationGuards::default();
            let faulty = FaultDisposal::new(fx.store(), Some(fault));
            assert!(
                fx.disposal(&faulty, &guards)
                    .dispose_build_transient(activity, slots, deadline)
                    .is_err()
            );
        }
        assert!(exists(&fx.gen_dir(verification)));
        assert_eq!(exists(&fx.lease_file(verification)), lease_phase);
        let clean = LedgerDisposal { store: fx.store() };
        let guards = GenerationGuards::default();
        assert!(
            fx.disposal(&clean, &guards)
                .dispose_build_transient(activity + 1000, slots, deadline)
                .is_err()
        );
        if lease_phase {
            assert!(
                exists(&fx.lease_file(verification)),
                "lease deleted before authority re-check ({fault:?})"
            );
        }
        assert!(
            exists(&fx.gen_dir(verification)),
            "directory deleted before authority re-check ({fault:?})"
        );
        // Positive control: the authorized scope still converges.
        fx.disposal(&clean, &guards)
            .dispose_build_transient(activity, slots, deadline)
            .unwrap();
        assert!(!exists(&fx.gen_dir(verification)));
    }
}

/// Resumed ordinary-cleanup (`CleanupKind::Expiry`) lease and directory
/// deletions also refuse a stale/wrong `activity_id` before unlinking.
#[test]
fn review_resumed_cleanup_lease_and_directory_deletion_rechecks_authority() {
    for (nonce, fault, lease_phase) in [
        (0x73u8, Fault::After("intend_lease_disposal", 1), true),
        (0x74u8, Fault::After("intend_directory_disposal", 1), false),
    ] {
        let fx = SweepFixture::new();
        let (vm, slots, _) = fx.active_clone(nonce);
        let generation = slots.disk.generation;
        fx.clock.advance(2_000, 10);
        assert!(fx.sweep(Some(fault)));
        assert!(exists(&fx.gen_dir(generation)));
        assert_eq!(exists(&fx.lease_file(generation)), lease_phase);

        let activity_id: i64 = fx
            .store()
            .transact(|db| {
                Ok(db.query_row(
                    "SELECT cleanup_id FROM vm_instances WHERE id=?1",
                    [vm],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        let mut stale_work = fx
            .store()
            .transact(|db| store::cleanup_work(db, CleanupKind::Expiry { vm, activity_id }))
            .unwrap()
            .expect("expiry cleanup work");
        stale_work.kind = CleanupKind::Expiry {
            vm,
            activity_id: activity_id + 1000,
        };
        let clean = LedgerDisposal { store: fx.store() };
        let guards = GenerationGuards::default();
        assert_eq!(
            fx.disposal(&clean, &guards)
                .dispose_cleanup(&stale_work, Instant::now() + Duration::from_secs(30)),
            Err(PATH)
        );
        if lease_phase {
            assert!(
                exists(&fx.lease_file(generation)),
                "cleanup lease deleted before authority re-check ({fault:?})"
            );
        }
        assert!(
            exists(&fx.gen_dir(generation)),
            "cleanup directory deleted before authority re-check ({fault:?})"
        );
        // Positive control: the authorized ordinary sweep converges cleanly.
        fx.clock.advance(0, 61);
        assert!(fx.sweep(None));
        assert!(!exists(&fx.gen_dir(generation)));
        assert_eq!(fx.count("SELECT count(*) FROM vm_disk_claims"), 0);
    }
}

/// The runtime has no current worker after reopening. The separate
/// application-gate fixture covers reservation and late registration.
struct RestartReaper {
    owned: bool,
    can_fence: bool,
    retries: Cell<usize>,
}

impl ReaperRuntime for RestartReaper {
    fn quiesce(&self, work: &CleanupWork, _: Instant) -> Quiesced {
        if self.owned {
            Quiesced::OwnedTestJob
        } else if work.release_only {
            Quiesced::Orphaned
        } else {
            Quiesced::Offline
        }
    }

    fn with_orphan_fence(&self, _: &CleanupWork, recover: &mut dyn FnMut() -> bool) -> bool {
        self.can_fence && recover()
    }

    fn graceful_stop(&self, _: &CleanupWork, _: Instant) -> StopDisposition {
        self.retries.set(self.retries.get() + 1);
        // An optimistic or stale worker response cannot substitute for the
        // release transaction's current epoch.
        StopDisposition::CleanRelease
    }

    fn force_reap(&self, _: &CleanupWork, _: Instant) -> StopDisposition {
        panic!("restart recovery may not signal an unowned process")
    }
}

fn arm_restart(fixture: &SweepFixture, vm: Id, pair: PairSlots, boot: Option<Hash>) -> Id {
    use crate::store::vm::native::recovery;
    let generation = pair.disk.generation;
    let origin = boot.map(|boot| {
        super::super::recovery::StorePins::open(&fixture.production)
            .unwrap()
            .origin(fixture.store(), boot)
            .unwrap()
    });
    fixture
        .store()
        .transact(|db| {
            let receipt = Binding {
                principal: "alice",
                key: RequestId::parse("0000000000000064000000000000aaaa").unwrap(),
                subject: Subject::Vm(vm),
                action: 3,
            }
            .record(db, NOW, Some(vm), None)?
            .receipt;
            db.execute("UPDATE vm_instances SET state=2 WHERE id=?1", [vm])?;
            let epoch = db.query_row(
                "SELECT operation_id FROM vm_operations WHERE activity_id=?1",
                [receipt.activity_id],
                |row| row.get(0),
            )?;
            store::snapshot::invalidate_release(db, vm, epoch)?;
            if let Some(origin) = origin {
                let GenerationState::Bound { scope, .. } =
                    disposal::generation_state(db, generation)?
                else {
                    unreachable!();
                };
                recovery::record_job(db, scope, epoch, origin)?;
            }
            Ok(epoch)
        })
        .unwrap()
}

#[test]
fn orphan_restart_releases_without_selection_and_retains_the_exact_pair() {
    let mut fx = SweepFixture::new_with_store(true);
    let (vm, slots, evidence) = fx.active_clone(61);
    let epoch = arm_restart(&fx, vm, slots, Some(boot::marker().unwrap()));
    std::fs::write(fx.slot_file(slots.disk), b"changed after the writer start").unwrap();
    fx.reopen();
    let guards = GenerationGuards::default();
    let custody = NativeRecovery::open(fx.store(), fx.state_root(), &guards).unwrap();
    let runtime = RestartReaper {
        owned: false,
        can_fence: true,
        retries: Cell::new(0),
    };
    // A selected effect can fail a temporary live probe while the
    // independent offline path still settles the interrupted epoch.
    use crate::vm::selection_epoch::{Invalidation, Sample, SelectionEpoch};
    let reading = fx.clock.now();
    let sample = Sample {
        captured: reading.monotonic,
        now: crate::vm::qualification_record::Instant2 {
            utc: u64::try_from(reading.utc_ms / 1_000).unwrap(),
            mono_ms: boot::uptime(),
        },
        boot_marker: boot::marker().unwrap(),
        reading,
    };
    let selection = SelectionEpoch::open([0x55; 16], sample, reading.monotonic).unwrap();
    let temporarily_unavailable = Invalidation::Transient(NativeRefusal::ReservesUnverified);
    assert_eq!(
        selection.refresh(|| Err(temporarily_unavailable), Instant::now),
        Err(temporarily_unavailable)
    );
    assert!(!selection.effects_revoked());
    fx.clock.advance(0, 0);
    assert!(fx.ctl().sweep_with(&runtime, &custody).unwrap());
    assert_eq!(
        fx.count("SELECT count(*) FROM vm_snapshot_release WHERE disposition='dirty'"),
        1
    );
    assert_eq!(
        fx.count("SELECT count(*) FROM vm_instances WHERE state=3"),
        1
    );
    fx.clock.advance(0, 0);
    assert!(fx.ctl().sweep_with(&runtime, &custody).unwrap());
    assert_eq!(
        fx.count("SELECT count(*) FROM vm_instances WHERE state=1"),
        1
    );
    assert_eq!(
        fx.count("SELECT sum(persistent) FROM vm_disk_claims"),
        CLAIM as i64
    );
    fx.store()
        .transact(|db| {
            let saved: Id = db.query_row(
                "SELECT epoch FROM vm_snapshot_release WHERE generation=?1",
                [slots.disk.generation],
                |row| row.get(0),
            )?;
            assert_eq!(saved, epoch);
            let dirty: Hash = db.query_row(
                "SELECT content_digest FROM vm_artifacts WHERE object_key=?1",
                [slots.disk.key],
                |row| row.get(0),
            )?;
            assert_ne!(dirty, evidence[0].pin.sha256);
            Ok(())
        })
        .unwrap();
    assert!(exists(&fx.slot_file(slots.disk)) && exists(&fx.slot_file(slots.vars)));
    assert_eq!(runtime.retries.get(), 0);
    assert!(
        crate::vm::runtime::CandidateRuntime::new(None)
            .dispatch::<()>(crate::vm::runtime::NativeMutation::Boot, |_| {
                panic!("restart custody minted an effect")
            })
            .is_err()
    );
}

#[test]
fn orphan_restart_refuses_live_writer_wrong_slot_lease_boot_epoch_store_and_protocol() {
    for fault in [
        "disk writer",
        "vars writer",
        "live lease",
        "slot",
        "lease",
        "boot",
        "epoch",
        "store",
        "protocol",
        "reacquire",
    ] {
        let mut fx = SweepFixture::new_with_store(true);
        let (vm, pair, _) = fx.active_clone(62);
        let boot = match fault {
            "boot" => Some([0xf7; 32]),
            "protocol" => None,
            _ => Some(boot::marker().unwrap()),
        };
        arm_restart(&fx, vm, pair, boot);
        fx.reopen();
        let mut pinned = None;
        let mut moved = None;
        match fault {
            "disk writer" | "vars writer" => {
                let slot = if fault == "disk writer" {
                    pair.disk
                } else {
                    pair.vars
                };
                pinned = Some(
                    OpenOptions::new()
                        .write(true)
                        .open(fx.slot_file(slot))
                        .unwrap(),
                );
            }
            "live lease" => {
                let lease = std::fs::File::open(fx.lease_file(pair.disk.generation)).unwrap();
                lease.try_lock().unwrap();
                pinned = Some(lease);
            }
            "slot" | "lease" => {
                let original = if fault == "slot" {
                    fx.slot_file(pair.vars)
                } else {
                    fx.lease_file(pair.disk.generation)
                };
                let shifted = original.with_extension("fixture-original");
                std::fs::rename(&original, &shifted).unwrap();
                let bytes: &[u8] = if fault == "slot" {
                    b"foreign vars"
                } else {
                    b""
                };
                std::fs::write(&original, bytes).unwrap();
                moved = Some((original, shifted));
            }
            "epoch" | "store" => {
                fx.store()
                    .transact(|db| {
                        if fault == "epoch" {
                            db.execute("UPDATE vm_snapshot_release SET epoch=?1", [[0x48u8; 16]])?;
                        } else {
                            db.execute(
                                "UPDATE vm_native_writer SET database_file=?1",
                                [[0x48u8; 16]],
                            )?;
                        }
                        Ok(())
                    })
                    .unwrap();
            }
            _ => {}
        }
        let guards = GenerationGuards::default();
        let custody = NativeRecovery::open(fx.store(), fx.state_root(), &guards).unwrap();
        let runtime = RestartReaper {
            owned: false,
            can_fence: fault != "reacquire",
            retries: Cell::new(0),
        };
        fx.clock.advance(0, 0);
        assert!(fx.ctl().sweep_with(&runtime, &custody).unwrap());
        assert_eq!(
            fx.count("SELECT count(*) FROM vm_snapshot_release WHERE disposition IS NULL"),
            1,
            "{fault}"
        );
        assert_eq!(
            fx.count("SELECT count(*) FROM vm_instances WHERE state=3"),
            1,
            "{fault}"
        );
        assert_eq!(
            fx.count("SELECT sum(persistent) FROM vm_disk_claims"),
            CLAIM as i64,
            "{fault}"
        );
        drop(pinned);
        if let Some((original, shifted)) = moved {
            std::fs::remove_file(&original).unwrap();
            std::fs::rename(shifted, original).unwrap();
        }
    }
}

#[test]
fn orphan_restart_stale_owner_ack_cannot_skip_durable_release_or_backoff() {
    let mut fx = SweepFixture::new_with_store(true);
    let (vm, pair, _) = fx.active_clone(63);
    arm_restart(&fx, vm, pair, Some(boot::marker().unwrap()));
    fx.reopen();
    let guards = GenerationGuards::default();
    let custody = NativeRecovery::open(fx.store(), fx.state_root(), &guards).unwrap();
    let mut runtime = RestartReaper {
        owned: true,
        can_fence: true,
        retries: Cell::new(0),
    };
    fx.clock.advance(0, 0);
    assert!(fx.ctl().sweep_with(&runtime, &custody).unwrap());
    assert!(fx.ctl().sweep_with(&runtime, &custody).unwrap());
    assert_eq!(runtime.retries.get(), 1);
    assert_eq!(
        fx.count("SELECT count(*) FROM vm_snapshot_release WHERE disposition IS NULL"),
        1
    );
    runtime.owned = false;
    fx.clock.advance(0, 6);
    assert!(fx.ctl().sweep_with(&runtime, &custody).unwrap());
    assert_eq!(
        fx.count("SELECT count(*) FROM vm_snapshot_release WHERE disposition='dirty'"),
        1
    );
}

#[test]
fn orphan_checkpointed_build_requires_both_worker_quiescence_and_writer_origin() {
    for fault in ["missing origin", "owned", "wrong boot", "released"] {
        let mut fx = SweepFixture::new_with_store(true);
        let (_, slots, _) = fx.stage_build(64);
        let generation = slots.verification.disk.generation;
        match fault {
            "missing origin" => fx
                .store()
                .transact(|db| {
                    db.execute(
                        "DELETE FROM vm_native_writer WHERE generation=?1",
                        [generation],
                    )?;
                    Ok(())
                })
                .unwrap(),
            "wrong boot" => {
                let mut wrong_boot = boot::marker().unwrap();
                wrong_boot[0] ^= 0x80;
                let origin = super::super::recovery::StorePins::open(&fx.production)
                    .unwrap()
                    .origin(fx.store(), wrong_boot)
                    .unwrap();
                fx.store()
                    .transact(|db| {
                        let GenerationState::Bound { scope, .. } =
                            disposal::generation_state(db, generation)?
                        else {
                            unreachable!();
                        };
                        native::recovery::record_job(db, scope, generation, origin)
                    })
                    .unwrap();
            }
            _ => {}
        }
        fx.reopen();
        let guards = GenerationGuards::default();
        let custody = NativeRecovery::open(fx.store(), fx.state_root(), &guards).unwrap();
        let runtime = RestartReaper {
            owned: fault == "owned",
            can_fence: true,
            retries: Cell::new(0),
        };
        // The still-owned fixture reports a successful stop, but hasn't
        // joined any owned build writer. It must not return Offline.
        let claimed = fault == "released";
        fx.clock.advance(0, 0);
        assert!(fx.ctl().sweep_with(&runtime, &custody).unwrap());
        assert_eq!(
            fx.count("SELECT count(*) FROM vm_build_intents"),
            i64::from(!claimed),
            "{fault}"
        );
        assert_eq!(exists(&fx.slot_file(slots.pair.disk)), !claimed);
        assert_eq!(exists(&fx.slot_file(slots.verification.disk)), !claimed);
    }
}

#[test]
fn orphan_expiry_commits_release_on_a_distinct_turn_before_disposal() {
    let mut fx = SweepFixture::new_with_store(true);
    let (vm, pair, _) = fx.active_clone(65);
    arm_restart(&fx, vm, pair, Some(boot::marker().unwrap()));
    fx.reopen();
    fx.clock.advance(2_000, 0);
    let guards = GenerationGuards::default();
    let custody = NativeRecovery::open(fx.store(), fx.state_root(), &guards).unwrap();
    let runtime = RestartReaper {
        owned: false,
        can_fence: true,
        retries: Cell::new(0),
    };
    fx.clock.advance(0, 0);
    assert!(fx.ctl().sweep_with(&runtime, &custody).unwrap());
    assert_eq!(
        fx.count("SELECT count(*) FROM vm_snapshot_release WHERE disposition='dirty'"),
        1
    );
    assert!(exists(&fx.slot_file(pair.disk)));
    assert!(exists(&fx.slot_file(pair.vars)));
    assert_eq!(
        fx.count("SELECT sum(persistent) FROM vm_disk_claims"),
        CLAIM as i64
    );

    fx.clock.advance(0, 0);
    assert!(fx.ctl().sweep_with(&runtime, &custody).unwrap());
    assert_eq!(fx.count("SELECT count(*) FROM vm_disk_claims"), 0);
    assert!(!exists(&fx.gen_dir(pair.disk.generation)));
}

struct ReplacedExpiry<'a> {
    store: &'a VmStore,
    vm: Id,
    attempted: Cell<bool>,
}

impl ReaperRuntime for ReplacedExpiry<'_> {
    fn quiesce(&self, work: &CleanupWork, _: Instant) -> Quiesced {
        let CleanupKind::Expiry { vm, activity_id } = work.kind else {
            panic!("expected resumed expiry work");
        };
        assert_eq!(vm, self.vm);
        self.store
            .transact(|db| {
                let replacement =
                    store::activity(db, "arxd", vm, crate::vm::Action::Expire, NOW + 2_000)?;
                assert_ne!(replacement, activity_id);
                assert_eq!(
                    db.execute(
                        "UPDATE vm_instances SET cleanup_id=?3 WHERE id=?1 AND cleanup_id=?2",
                        params![vm, activity_id, replacement],
                    )?,
                    1
                );
                Ok(())
            })
            .unwrap();
        self.attempted.set(true);
        Quiesced::Offline
    }

    fn graceful_stop(&self, _: &CleanupWork, _: Instant) -> StopDisposition {
        panic!("no owner to stop")
    }

    fn force_reap(&self, _: &CleanupWork, _: Instant) -> StopDisposition {
        panic!("no Job to signal")
    }
}

#[test]
fn orphan_expiry_resume_refuses_superseded_scope_before_deleting() {
    for checkpoint in ["intend_lease_disposal", "intend_directory_disposal"] {
        let mut fx = SweepFixture::new_with_store(true);
        let (vm, pair, _) = fx.active_clone(66);
        arm_restart(&fx, vm, pair, Some(boot::marker().unwrap()));
        fx.reopen();
        fx.clock.advance(2_000, 0);
        {
            let native_guards = GenerationGuards::default();
            let custody =
                NativeRecovery::open(fx.store(), fx.state_root(), &native_guards).unwrap();
            let runtime = RestartReaper {
                owned: false,
                can_fence: true,
                retries: Cell::new(0),
            };
            fx.clock.advance(0, 0);
            assert!(fx.ctl().sweep_with(&runtime, &custody).unwrap());
            assert_eq!(
                fx.count("SELECT count(*) FROM vm_snapshot_release WHERE disposition='dirty'"),
                1
            );
            // Interrupt after the durable sidecar intent. Use the same guard
            // set for both adapters before reopening the process-local state.
            let ledger = FaultDisposal::new(fx.store(), Some(Fault::After(checkpoint, 1)));
            let fault = fx.disposal(&ledger, &native_guards);
            fx.clock.advance(0, 0);
            assert!(fx.ctl().sweep_with(&OfflineReaper, &fault).unwrap());
            assert_eq!(ledger.counts.borrow().get(checkpoint), Some(&1));
        }
        fx.reopen();
        let generation = pair.disk.generation;
        let dir = fx.gen_dir(generation);
        let lease = fx.lease_file(generation);
        assert!(exists(&dir));
        let lease_remains = checkpoint == "intend_lease_disposal";
        assert_eq!(exists(&lease), lease_remains);

        let guards = GenerationGuards::default();
        let custody = NativeRecovery::open(fx.store(), fx.state_root(), &guards).unwrap();
        let stale = ReplacedExpiry {
            store: fx.store(),
            vm,
            attempted: Cell::new(false),
        };
        fx.clock.advance(0, 0);
        assert!(fx.ctl().sweep_with(&stale, &custody).unwrap());
        assert!(stale.attempted.get());
        assert!(exists(&dir));
        assert_eq!(exists(&lease), lease_remains);
        assert_eq!(
            fx.count("SELECT sum(persistent) FROM vm_disk_claims"),
            CLAIM as i64
        );

        fx.clock.advance(0, 0);
        assert!(fx.ctl().sweep_with(&OfflineReaper, &custody).unwrap());
        assert!(!exists(&dir), "fresh scope must converge");
        assert_eq!(fx.count("SELECT count(*) FROM vm_disk_claims"), 0);
    }
}

struct HungExpiry<'a> {
    fixture: &'a SweepFixture,
    target: native::release::Target,
    pair: PairSlots,
    identities: [FileIdentity; 2],
    owned: RefCell<Option<(ProductionWorkspace, File)>>,
    graceful_attempts: Cell<usize>,
    forced_attempts: Cell<usize>,
}

impl ReaperRuntime for HungExpiry<'_> {
    fn quiesce(&self, _: &CleanupWork, _: Instant) -> Quiesced {
        if self.owned.borrow().is_some() {
            Quiesced::OwnedTestJob
        } else {
            Quiesced::Offline
        }
    }

    fn graceful_stop(&self, work: &CleanupWork, _: Instant) -> StopDisposition {
        assert!(work.release_only);
        assert!(self.owned.borrow().is_some());
        self.graceful_attempts.set(self.graceful_attempts.get() + 1);
        StopDisposition::ChargedQuarantine
    }

    fn force_reap(&self, work: &CleanupWork, deadline: Instant) -> StopDisposition {
        assert!(matches!(work.kind, CleanupKind::Expiry { vm, .. } if vm == self.target.vm));
        assert!(work.release_only);
        self.forced_attempts.set(self.forced_attempts.get() + 1);
        let (workspace, writer) = self.owned.borrow_mut().take().expect("current owner");
        // Fixture stand-in for confirmed Job exit and worker join. The same
        // workspace lease stays held through the actual release transaction.
        drop(writer);
        let proof = super::super::release_tests::proof(
            &workspace,
            self.pair,
            self.identities,
            StopDisposition::DirtyRelease,
        );
        proof
            .observe_test_release(self.target, deadline, &|| false)
            .unwrap()
            .commit(self.fixture.store(), None, || self.fixture.clock.now())
            .unwrap();
        StopDisposition::DirtyRelease
    }
}

#[test]
fn orphan_expiry_force_reaps_owned_hung_guest_before_a_later_disposal_turn() {
    let fx = SweepFixture::new_with_store(true);
    let (vm, pair, evidence) = fx.active_clone(67);
    let epoch = arm_restart(&fx, vm, pair, Some(boot::marker().unwrap()));
    let generation = pair.disk.generation;
    let scope = fx
        .store()
        .transact(|db| {
            let GenerationState::Bound { scope, .. } = disposal::generation_state(db, generation)?
            else {
                unreachable!();
            };
            Ok(scope)
        })
        .unwrap();
    let workspace =
        hold_workspace_under(&fx.production, fx.owner_sid(), binding_from_scope(scope)).unwrap();
    let writer = OpenOptions::new()
        .write(true)
        .open(fx.slot_file(pair.disk))
        .unwrap();
    let runtime = HungExpiry {
        fixture: &fx,
        target: native::release::Target {
            vm,
            generation,
            epoch,
        },
        pair,
        identities: evidence.map(|file| file.identity),
        owned: RefCell::new(Some((workspace, writer))),
        graceful_attempts: Cell::new(0),
        forced_attempts: Cell::new(0),
    };
    let guards = GenerationGuards::default();
    let custody = NativeRecovery::open(fx.store(), fx.state_root(), &guards).unwrap();

    fx.clock.advance(2_000, 0);
    assert!(fx.ctl().sweep_with(&runtime, &custody).unwrap());
    assert_eq!(runtime.graceful_attempts.get(), 1);
    assert_eq!(runtime.forced_attempts.get(), 1);
    assert!(runtime.owned.borrow().is_none());
    assert_eq!(
        fx.count("SELECT count(*) FROM vm_snapshot_release WHERE disposition='dirty'"),
        1
    );
    assert_eq!(
        fx.count("SELECT sum(persistent) FROM vm_disk_claims"),
        CLAIM as i64
    );
    assert!(exists(&fx.slot_file(pair.disk)));
    assert!(exists(&fx.slot_file(pair.vars)));
    assert!(exists(&fx.lease_file(generation)));

    fx.clock.advance(0, 0);
    assert!(fx.ctl().sweep_with(&runtime, &custody).unwrap());
    assert_eq!(runtime.forced_attempts.get(), 1);
    assert_eq!(fx.count("SELECT count(*) FROM vm_disk_claims"), 0);
    assert!(!exists(&fx.gen_dir(generation)));
}
