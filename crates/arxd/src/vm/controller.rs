use super::{
    model::*,
    policy::*,
    reaper::{self, ArtifactCustody, ClosedCustody, ReaperTick, Sweeps},
    runtime::{CandidateRuntime, ReaperRuntime},
    selection_epoch::Invalidation,
    static_web::StaticState,
    template,
};
use crate::{
    service::Shutdown,
    store::{
        validate_principal,
        vm::{self as store, VmStore},
    },
};
use anyhow::{Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    sync::{
        Arc, Mutex, TryLockError,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

/// Only a fresh admission yields a worker handle. A status lookup has no
/// dispatch authority. Deadlines are process-local and never persisted.
/// Every field is private to the controller: no other code can mint a
/// worker, change its admitted identity or reset its one-intent latch.
#[derive(Debug)]
pub struct PendingOperation {
    activity_id: i64,
    deadline: Instant,
    binding: Arc<()>,
    /// The exact request this admission authorized, for native TEST work.
    #[cfg_attr(
        not(all(windows, target_arch = "x86_64")),
        allow(dead_code, reason = "the native TEST issuer is Windows x64 only")
    )]
    admitted: Option<Box<Admitted>>,
    /// Set once, when the native TEST issuer mints this worker's intent.
    #[cfg_attr(
        not(all(windows, target_arch = "x86_64")),
        allow(dead_code, reason = "the native TEST issuer is Windows x64 only")
    )]
    intent_minted: AtomicBool,
}

impl PendingOperation {
    pub fn activity_id(&self) -> i64 {
        self.activity_id
    }

    pub fn deadline(&self) -> Instant {
        self.deadline
    }

    /// The request fixed at admission, when it may carry native TEST work.
    #[cfg(all(windows, target_arch = "x86_64"))]
    pub(crate) fn admitted(&self) -> Option<&Admitted> {
        self.admitted.as_deref()
    }

    /// Claims this worker's single native TEST intent. The latch never
    /// resets.
    #[cfg(all(windows, target_arch = "x86_64"))]
    pub(super) fn claim_intent(&self) -> bool {
        !self.intent_minted.swap(true, Ordering::AcqRel)
    }

    /// A detached worker for fixtures that never reach the controller.
    #[cfg(test)]
    pub(crate) fn for_test(activity_id: i64, deadline: Instant) -> Self {
        Self {
            activity_id,
            deadline,
            binding: Arc::new(()),
            admitted: None,
            intent_minted: AtomicBool::new(false),
        }
    }

    #[cfg(test)]
    pub(crate) fn binding_for_test(&self) -> &Arc<()> {
        &self.binding
    }

    #[cfg(test)]
    pub(crate) fn extend_deadline_for_test(&mut self, by: Duration) {
        self.deadline += by;
    }
}

/// The request a fresh admission authorized, fixed when its worker is
/// minted.
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(
    not(all(windows, target_arch = "x86_64")),
    allow(dead_code, reason = "the native TEST issuer is Windows x64 only")
)]
pub(crate) struct Admitted {
    pub(crate) principal: String,
    pub(crate) request: RequestId,
    pub(crate) action: i64,
    pub(crate) operation: Id,
    pub(crate) effect: AdmittedEffect,
}

impl Admitted {
    fn new(binding: &store::Binding<'_>, operation: Id, effect: AdmittedEffect) -> Box<Self> {
        Box::new(Self {
            principal: binding.principal.to_owned(),
            request: binding.key,
            action: binding.action,
            operation,
            effect,
        })
    }
}

/// What kind of native TEST effect the admitted request asked for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(
    not(all(windows, target_arch = "x86_64")),
    allow(dead_code, reason = "the native TEST issuer is Windows x64 only")
)]
pub(crate) enum AdmittedEffect {
    Clone,
    Start,
    Script { digest: [u8; 32], timeout_s: u64 },
    Build { recipe_id: Id, digest: [u8; 32] },
}

/// Foreground VM-profile metadata. A successful admission commits intent;
/// there is no runtime, transport, or process launcher in this controller.
pub struct VmController<C> {
    store: VmStore,
    policy: VmPolicy,
    clock: C,
    gate: Mutex<AdmissionGate>,
    sweeps: Mutex<Sweeps>,
    schedule: Mutex<SweepSchedule>,
    worker_binding: Arc<()>,
}

// Held only while choosing a turn. The sweep lock owns runtime and store work.
#[derive(Default)]
struct SweepSchedule {
    vault_ready: bool,
    target: Option<Id>,
    ordinary_owed: bool,
    handoff: bool,
}

impl SweepSchedule {
    fn yield_between_items(&mut self) -> bool {
        self.handoff || self.yield_to_vault()
    }

    fn yield_to_vault(&mut self) -> bool {
        if self.vault_ready && !self.ordinary_owed {
            self.ordinary_owed = true;
            self.handoff = true;
            return true;
        }
        false
    }
}

pub(crate) struct VaultSweepPriority<'a> {
    schedule: &'a Mutex<SweepSchedule>,
    acquisition_deadline: Instant,
    custody_budget: Duration,
    custody_deadline: Option<Instant>,
    retry_at: Option<Instant>,
}

impl VaultSweepPriority<'_> {
    fn defer_until(
        &mut self,
        retry: Option<Instant>,
        deadline: Instant,
    ) -> Option<VaultTestCleanup> {
        let mut schedule = self.schedule.lock().unwrap();
        schedule.vault_ready = false;
        schedule.handoff = false;
        self.retry_at = retry;
        retry
            .filter(|next| *next >= deadline)
            .map(|_| VaultTestCleanup::Inconclusive)
    }
}

impl Drop for VaultSweepPriority<'_> {
    fn drop(&mut self) {
        let mut schedule = self.schedule.lock().unwrap();
        schedule.vault_ready = false;
        schedule.target = None;
        schedule.handoff = false;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum VaultTestCleanup {
    Confirmed,
    Unconfirmed,
    Inconclusive,
}

#[derive(Default)]
struct AdmissionGate {
    closed: bool,
    custody_uncertain: bool,
}

impl AdmissionGate {
    fn rejects_fresh(&self) -> bool {
        self.closed || self.custody_uncertain
    }
}

impl<C: Clock> VmController<C> {
    pub(crate) fn compose_now(&self) -> Result<i64> {
        utc(self.clock.now())
    }

    /// Advance `vm_retry_clock` with the wall clock read inside the store
    /// transaction, so no other writer moves the high water between the read
    /// and the comparison. A reading below the committed high water is a
    /// rollback. Contention is transient; the connection is never awaited.
    #[cfg_attr(
        not(all(windows, target_arch = "x86_64")),
        allow(dead_code, reason = "the native TEST issuer is Windows x64 only")
    )]
    pub(crate) fn advance_native_clock(&self) -> Result<ClockReading, Invalidation> {
        let reason = super::runtime::NativeRefusal::QualificationMissing;
        let classify = |error: anyhow::Error| Invalidation::from_store(&error, reason);
        let mut db = self
            .store
            .try_live_connection()
            .map_err(|error| classify(error.into()))?;
        let tx = db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|error| classify(error.into()))?;
        let clock = self.clock.now();
        let now = utc(clock).map_err(|_| Invalidation::Indeterminate(reason))?;
        let high_water = store::retry_time(&tx, now).map_err(classify)?;
        tx.commit().map_err(|error| classify(error.into()))?;
        if high_water > now {
            return Err(Invalidation::Confirmed(reason));
        }
        Ok(clock)
    }

    pub(crate) fn compose_store(&self) -> &VmStore {
        &self.store
    }

    pub(crate) fn validate_compose_backup_disk(
        &self,
        db: &Connection,
        capacity: Option<Capacity>,
        staging_bytes: u64,
    ) -> Result<Result<(), Refusal>> {
        let Some(capacity) = capacity else {
            return Ok(Err(Refusal::UnknownHost));
        };
        let disk = DiskClaim {
            staging: staging_bytes,
            images: 0,
            metadata: 0,
        };
        self.validate_disk(db, capacity, disk)
    }

    #[allow(
        dead_code,
        reason = "used by owner enrollment and E4 protected backup fixtures"
    )]
    pub(crate) fn enroll_compose_backup_target(
        &self,
        vm: Id,
        project: &str,
        volume: &str,
        database: &str,
        max_bytes: u64,
    ) -> Result<()> {
        self.store.transact(|db| {
            store::compose::enroll_backup_target(db, vm, project, volume, database, max_bytes)
        })
    }

    pub(crate) fn windows_recipe_id() -> Id {
        template::windows_recipe_id()
    }

    fn build_guest(recipe: &str) -> Guest {
        if recipe == hex::encode(Self::windows_recipe_id()) {
            Guest::WindowsX86_64
        } else {
            Guest::UbuntuX86_64
        }
    }

    pub fn open(vm_state_root: &Path, policy: VmPolicy, clock: C) -> Result<Self> {
        let store = VmStore::open(vm_state_root, utc(clock.now())?)?;
        Ok(Self {
            store,
            policy,
            clock,
            gate: Mutex::default(),
            sweeps: Mutex::default(),
            schedule: Mutex::default(),
            worker_binding: Arc::new(()),
        })
    }

    pub fn admit(
        &self,
        request: Request<'_>,
        host: HostObservation,
        resources: Resources,
    ) -> Result<Admission> {
        if validate_principal(request.principal).is_err() {
            return Ok(Admission::Refused(Refusal::Invalid));
        }
        let closed = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("VM gate unavailable"))?;
        let result = self.store.transact(|db| {
            let clock = self.clock.now();
            let now = utc(clock)?;
            let action = request.operation.action();

            let decision = self.admit_inner(db, &request, host, resources, clock, &closed)?;
            if let Admission::Refused(reason) = decision {
                store::refusal(db, request.principal, request.vm, action, now, reason)?;
            }
            Ok(decision)
        });
        match result {
            Err(error) if store::full(&error) => Ok(Admission::Refused(Refusal::StateHeadroom)),
            result => result,
        }
    }

    fn admit_inner(
        &self,
        db: &Connection,
        request: &Request<'_>,
        host: HostObservation,
        resources: Resources,
        clock: ClockReading,
        gate: &AdmissionGate,
    ) -> Result<Admission> {
        let now = clock.utc_ms;
        let action = request.operation.action();
        if matches!(action, Action::Snapshot | Action::Restore) {
            return Ok(Admission::Refused(Refusal::Invalid));
        }
        let binding = store::Binding {
            principal: request.principal,
            key: request.request_id,
            subject: Subject::Vm(request.vm),
            action: action as i64,
        };
        match binding.replay(db, now, gate.closed)? {
            Ok(Some(prior)) => return Ok(Admission::Existing(prior.operation.receipt)),
            Err(reason) => return Ok(Admission::Refused(reason)),
            Ok(None) => {}
        }
        if action != Action::Stop
            && (gate.custody_uncertain || store::has_failed_clone_cleanup(db)?)
        {
            if action != Action::Clone
                && let Err(reason) = target(db, request, now)?
            {
                return Ok(Admission::Refused(reason));
            }
            return Ok(Admission::Refused(Refusal::Shutdown));
        }
        if store::count(db, "SELECT count(*) FROM vm_operations WHERE status=0", [])?
            >= store::MAX_CALLER_PENDING
        {
            return Ok(Admission::Refused(Refusal::PendingFull));
        }
        let seconds = match timeout(&request.operation) {
            Ok(seconds) => seconds,
            Err(reason) => return Ok(Admission::Refused(reason)),
        };
        let Some(deadline) = clock.monotonic.checked_add(Duration::from_secs(seconds)) else {
            return Ok(Admission::Refused(Refusal::Invalid));
        };
        let check_target = || target(db, request, now);
        let target = if action == Action::Stop {
            if request.disk.total() != Some(0) {
                return Ok(Admission::Refused(Refusal::Invalid));
            }
            match check_target()? {
                Ok(target) => target,
                Err(reason) => return Ok(Admission::Refused(reason)),
            }
        } else {
            let capacity = match self.policy.host(host, resources) {
                Ok(capacity) => capacity,
                Err(reason) => return Ok(Admission::Refused(reason)),
            };
            if let Err(reason) = self.validate_disk(db, capacity, request.disk)? {
                return Ok(Admission::Refused(reason));
            }
            match check_target()? {
                Ok(target) => target,
                Err(reason) => return Ok(Admission::Refused(reason)),
            }
        };
        let lease_end = match target {
            Target::Clone { expires, .. } => Some(expires),
            _ => None,
        };
        let operation = binding.record(db, now, Some(request.vm), lease_end)?;
        let id = operation.receipt.activity_id;
        target.insert(db, request, id, resources)?;
        if action == Action::Start {
            let epoch = db.query_row(
                "SELECT operation_id FROM vm_operations WHERE activity_id=?1",
                [id],
                |row| row.get::<_, Id>(0),
            )?;
            store::snapshot::invalidate_release(db, request.vm, epoch)?;
        }
        store::require_headroom(db)?;
        let effect = match request.operation {
            Operation::Start => Some(AdmittedEffect::Start),
            Operation::Script {
                script,
                timeout_seconds,
            } => Some(AdmittedEffect::Script {
                digest: Sha256::digest(script.as_bytes()).into(),
                timeout_s: timeout_seconds,
            }),
            _ => None,
        };
        Ok(Admission::Admitted(PendingOperation {
            activity_id: id,
            deadline,
            binding: Arc::clone(&self.worker_binding),
            admitted: effect.map(|effect| Admitted::new(&binding, operation.id, effect)),
            intent_minted: AtomicBool::new(false),
        }))
    }

    pub fn finish(&self, operation: &PendingOperation, completion: Completion) -> Result<Status> {
        self.finish_with_cause(operation, completion, None)
    }

    /// As [`VmController::finish`], recording the worker's proven, closed
    /// failure cause independently of release proof.
    pub(crate) fn finish_with_cause(
        &self,
        operation: &PendingOperation,
        mut completion: Completion,
        cause: Option<FailureReason>,
    ) -> Result<Status> {
        ensure!(
            Arc::ptr_eq(&self.worker_binding, &operation.binding),
            "invalid VM worker handle"
        );
        self.store.transact(|db| {
            ensure!(store::count(db,
                "SELECT count(*) FROM vm_operations WHERE activity_id=?1 AND binding_kind IN ('recipe','template','snapshot')",
                [operation.activity_id],
            )? == 0, "durable VM worker requires artifact settlement");
            let now = self.clock.now();
            if now.monotonic >= operation.deadline {
                completion = Completion::CleanupUnconfirmed;
            }
            store::settle_with_script(
                db,
                operation.activity_id,
                utc(now)?,
                completion,
                None,
                cause,
            )
        })
    }

    #[allow(dead_code, reason = "foreground script execution is wired in 4d")]
    pub(crate) fn finish_script(
        &self,
        worker: &PendingOperation,
        result: ScriptResult,
    ) -> Result<Status> {
        self.check_worker(worker)?;
        self.store.transact(|db| {
            let clock = self.clock.now();
            let (completion, script) = if clock.monotonic >= worker.deadline {
                (Completion::CleanupUnconfirmed, None)
            } else if result == ScriptResult::Exited(0) {
                (Completion::Succeeded, Some(result))
            } else {
                (Completion::Failed, Some(result))
            };
            ensure!(store::count(db,
                "SELECT count(*) FROM vm_operations WHERE activity_id=?1 AND action=2 AND status=0",
                [worker.activity_id],
            )? == 1, "invalid script worker");
            store::settle_with_script(
                db,
                worker.activity_id,
                utc(clock)?,
                completion,
                script,
                None,
            )
        })
    }

    #[allow(dead_code, reason = "foreground MCP projection is wired in 4d")]
    pub(crate) fn operation_status(
        &self,
        principal: &str,
        id: Id,
    ) -> Result<Result<OperationStatus, Refusal>> {
        if validate_principal(principal).is_err() {
            return Ok(Err(Refusal::NotFound));
        }
        self.store.transact(|db| {
            Ok(
                store::operation_status(db, principal, id, utc(self.clock.now())?)?
                    .ok_or(Refusal::NotFound),
            )
        })
    }

    #[allow(dead_code, reason = "foreground MCP projection is wired in 4d")]
    pub(crate) fn list_status(
        &self,
        principal: &str,
        after: Option<Id>,
        limit: u32,
    ) -> Result<Result<Vec<VmStatus>, Refusal>> {
        if validate_principal(principal).is_err() || !(1..=20).contains(&limit) {
            return Ok(Err(Refusal::Invalid));
        }
        self.store.transact(|db| {
            Ok(Ok(store::vm_status_rows(
                db,
                principal,
                None,
                after,
                limit,
                utc(self.clock.now())?,
            )?))
        })
    }

    #[allow(dead_code, reason = "foreground MCP projection is wired in 4d")]
    pub(crate) fn get_status(&self, principal: &str, id: Id) -> Result<Result<VmStatus, Refusal>> {
        if validate_principal(principal).is_err() {
            return Ok(Err(Refusal::NotFound));
        }
        self.store.transact(|db| {
            let now = utc(self.clock.now())?;
            if let Some(item) = store::vm_status_rows(db, principal, Some(id), None, 1, now)?.pop()
            {
                return Ok(Ok(item));
            }
            store::purge_tombstones(db, now)?;
            // Correlation comes only from the attributed expiry receipt.
            let proof = store::tombstone_proof(db, principal, id)?;
            Ok(store::expired_status(db, principal, id)?
                .map(|row| VmStatus {
                    id,
                    template: None,
                    guest: None,
                    state: "expired",
                    ready: false,
                    expires_at_ms: None,
                    expired_at_ms: Some(row.expired_at_ms),
                    cleanup: proof.and_then(|(_, confirmed)| confirmed).map(|confirmed| {
                        if confirmed {
                            "confirmed"
                        } else {
                            "unconfirmed"
                        }
                    }),
                    snapshots: [],
                    cleanup_activity: proof.map(|(activity, _)| activity),
                })
                .ok_or(Refusal::NotFound))
        })
    }

    pub(crate) fn admit_build(
        &self,
        request: BuildRequest<'_>,
        host: HostObservation,
        resources: Resources,
        verify_sealed: impl FnOnce(Id, PairSlots) -> Option<[ArtifactEvidence; 2]>,
    ) -> Result<Result<BuildAdmission, Refusal>> {
        if validate_principal(request.principal).is_err()
            || request.recipe.len() != 32
            || !request
                .recipe
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        {
            return Ok(Err(Refusal::Invalid));
        }
        let closed = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("VM gate unavailable"))?;
        map_full(self.store.transact(|db| {
            let clock = self.clock.now();
            let now = utc(clock)?;
            let binding = store::Binding {
                principal: request.principal,
                key: request.key,
                subject: Subject::Recipe(request.recipe, Some(request.digest)),
                action: BUILD_ACTION,
            };
            let decision = self.reserve_build(
                db,
                request,
                &binding,
                clock,
                closed.rejects_fresh() || store::has_failed_clone_cleanup(db)?,
                host,
                resources,
                verify_sealed,
            )?;
            if let Err(reason) = decision {
                store::refusal_for(db, request.principal, None, BUILD_ACTION, now, reason)?;
            }
            Ok(decision)
        }))
    }

    #[allow(clippy::too_many_arguments)]
    fn reserve_build(
        &self,
        db: &Connection,
        request: BuildRequest<'_>,
        binding: &store::Binding<'_>,
        clock: ClockReading,
        closed: bool,
        host: HostObservation,
        resources: Resources,
        verify_sealed: impl FnOnce(Id, PairSlots) -> Option<[ArtifactEvidence; 2]>,
    ) -> Result<Result<BuildAdmission, Refusal>> {
        let now = utc(clock)?;
        match binding.replay(db, now, closed)? {
            Ok(Some(prior)) => return Ok(Ok(BuildAdmission::Existing(prior.operation))),
            Err(reason) => return Ok(Err(reason)),
            Ok(None) => {}
        }
        if let Some((template, slots)) = store::sealed_pair(db, request.recipe, request.digest)? {
            // Only a bounded read through the qualified runtime is permitted here.
            let Some([disk, vars]) = verify_sealed(template, slots) else {
                return Ok(Err(Refusal::Busy));
            };
            if !store::evidence_matches(db, slots.disk, disk, "sealed")?
                || !store::evidence_matches(db, slots.vars, vars, "sealed")?
            {
                return Ok(Err(Refusal::Busy));
            }
            let mut operation = binding.record(db, now, None, None)?;
            store::finish(
                db,
                operation.receipt.activity_id,
                Status::Succeeded,
                now,
                Some(true),
            )?;
            store::require_headroom(db)?;
            operation.receipt.status = Status::Succeeded;
            return Ok(Ok(BuildAdmission::Existing(operation)));
        }
        if store::count(db, "SELECT count(*) FROM vm_operations WHERE status=0", [])?
            >= store::MAX_CALLER_PENDING
        {
            return Ok(Err(Refusal::PendingFull));
        }
        if store::count(
            db,
            "SELECT EXISTS(SELECT 1 FROM vm_build_intents WHERE state<>'sealed')
                 OR EXISTS(SELECT 1 FROM vm_instances WHERE state IN (2,3,4))
                 OR EXISTS(SELECT 1 FROM vm_static_instances WHERE state IN (0,2,3))
                 OR EXISTS(SELECT 1 FROM vm_operations WHERE status=0
                     AND (action=3 OR binding_kind='template'))",
            [],
        )? != 0
        {
            return Ok(Err(Refusal::Busy));
        }
        if store::count(db, "SELECT count(*) FROM vm_templates", [])? >= 4 {
            return Ok(Err(Refusal::TemplateLimit));
        }
        if request.retained.images == 0
            || request.retained.staging != 0
            || request.transient.images == 0
        {
            return Ok(Err(Refusal::Invalid));
        }
        let Some(peak) = request
            .retained
            .total()
            .and_then(|bytes| bytes.checked_add(request.transient.total()?))
            .filter(|bytes| i64::try_from(*bytes).is_ok())
        else {
            return Ok(Err(Refusal::Disk));
        };
        let capacity = match self.policy.host(host, resources) {
            Ok(capacity) => capacity,
            Err(reason) => return Ok(Err(reason)),
        };
        if let Err(reason) = self.validate_disk(
            db,
            capacity,
            DiskClaim {
                staging: peak,
                ..DiskClaim::default()
            },
        )? {
            return Ok(Err(reason));
        }
        // Windows measures its install deadline from first launch. Allow
        // bounded preparation and settlement around that separate cutoff.
        let seconds = match Self::build_guest(request.recipe) {
            Guest::UbuntuX86_64 => 3_900,
            Guest::WindowsX86_64 => template::WINDOWS_BUILD_LIMIT.as_secs() + 300,
        };
        let pending = self.worker(clock, seconds)?;
        let operation = binding.record(db, now, None, None)?;
        let slots = store::reserve_build(db, request, operation, peak)?;
        store::require_headroom(db)?;
        let mut recipe_id = [0; 16];
        hex::decode_to_slice(request.recipe, &mut recipe_id)?;
        let effect = AdmittedEffect::Build {
            recipe_id,
            digest: request.digest,
        };
        Ok(Ok(BuildAdmission::Fresh(
            Box::new(slots),
            PendingOperation {
                activity_id: operation.receipt.activity_id,
                admitted: Some(Admitted::new(binding, operation.id, effect)),
                ..pending
            },
            operation,
        )))
    }

    pub(crate) fn admit_linked_clone(
        &self,
        mut request: Request<'_>,
        host: HostObservation,
        resources: Resources,
        golden: PairSlots,
        proof: [ArtifactEvidence; 2],
    ) -> Result<Result<CloneAdmission, Refusal>> {
        let Operation::Clone {
            template,
            lease_seconds,
        } = request.operation
        else {
            return Ok(Err(Refusal::Invalid));
        };
        let seconds = lease_seconds.unwrap_or(DEFAULT_LEASE_SECONDS);
        if validate_principal(request.principal).is_err() || lease(0, Some(seconds)).is_err() {
            return Ok(Err(Refusal::Invalid));
        }
        let digest = Sha256::new()
            .chain_update(b"arxcovo.clone.ttl.v1\0")
            .chain_update(seconds.to_be_bytes())
            .finalize()
            .into();
        let closed = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("VM gate unavailable"))?;
        map_full(self.store.transact(|db| {
            let clock = self.clock.now();
            let now = utc(clock)?;
            let binding = store::Binding {
                principal: request.principal,
                key: request.request_id,
                subject: Subject::Template(template, digest),
                action: Action::Clone as i64,
            };
            match binding.replay(
                db, now, closed.rejects_fresh() || store::has_failed_clone_cleanup(db)?,
            )? {
                    Ok(Some(prior)) => return Ok(Ok(CloneAdmission::Existing(prior.operation))),
                    Err(reason) => return Ok(Err(reason)),
                    Ok(None) => {}
                }
                if store::count(db, "SELECT count(*) FROM vm_operations WHERE status=0", [])?
                    >= store::MAX_CALLER_PENDING
                {
                    return Ok(Err(Refusal::PendingFull));
                }
                if store::count(db,
                    "SELECT EXISTS(SELECT 1 FROM vm_build_intents WHERE state<>'sealed')
                     OR EXISTS(SELECT 1 FROM vm_static_instances WHERE state IN (0,2,3))
                     OR EXISTS(SELECT 1 FROM vm_operations WHERE status=0 AND binding_kind='template')",
                    [],
                )? != 0 {
                    return Ok(Err(Refusal::Busy));
                }
                if golden.disk.generation != golden.vars.generation || store::count(db,
                    "SELECT count(*) FROM vm_build_intents b JOIN vm_generations g ON g.build_id=b.id
                     WHERE b.template_id=?1 AND b.state='sealed' AND g.active=1 AND g.id=?2",
                    params![template, golden.disk.generation],
                )? != 1
                    || !store::evidence_matches(db, golden.disk, proof[0], "sealed")?
                    || !store::evidence_matches(db, golden.vars, proof[1], "sealed")?
                {
                    return Ok(Err(Refusal::NotFound));
                }
                let capacity = match self.policy.host(host, resources) {
                    Ok(capacity) => capacity,
                    Err(reason) => return Ok(Err(reason)),
                };
                if let Err(reason) = self.validate_disk(db, capacity, request.disk)? {
                    return Ok(Err(reason));
                }
                let pending = self.worker(clock, 120)?;
                request.vm = store::new_id()?;
                let target = match target(db, &request, now)? {
                    Ok(target) => target,
                    Err(reason) => return Ok(Err(reason)),
                };
                let Target::Clone { expires, .. } = target else {
                    anyhow::bail!("invalid linked clone target");
                };
                let operation = binding.record(db, now, Some(request.vm), Some(expires))?;
                target.insert(db, &request, operation.receipt.activity_id, resources)?;
                let slots = store::plan_clone(db, request.vm)?;
                store::require_headroom(db)?;
                Ok(Ok(CloneAdmission::Fresh(request.vm, slots, PendingOperation {
                    activity_id: operation.receipt.activity_id,
                    admitted: Some(Admitted::new(&binding, operation.id, AdmittedEffect::Clone)),
                    ..pending
                }, operation)))
        }))
    }

    fn worker(&self, clock: ClockReading, seconds: u64) -> Result<PendingOperation> {
        Ok(PendingOperation {
            activity_id: 0,
            deadline: clock
                .monotonic
                .checked_add(Duration::from_secs(seconds))
                .ok_or_else(|| anyhow::anyhow!("invalid VM deadline"))?,
            binding: Arc::clone(&self.worker_binding),
            admitted: None,
            intent_minted: AtomicBool::new(false),
        })
    }

    fn check_worker(&self, worker: &PendingOperation) -> Result<()> {
        ensure!(
            Arc::ptr_eq(&self.worker_binding, &worker.binding),
            "invalid VM worker handle"
        );
        Ok(())
    }

    pub(crate) fn build_evidence(
        &self,
        worker: &PendingOperation,
        build: Id,
        entries: &[(ArtifactSlot, Option<ArtifactEvidence>)],
    ) -> Result<()> {
        self.check_worker(worker)?;
        self.store.transact(|db| {
            for &(slot, evidence) in entries {
                ensure!(store::count(db,
                    "SELECT count(*) FROM vm_build_intents b JOIN vm_operations o USING(operation_id)
                     JOIN vm_generations g ON g.build_id=b.id WHERE b.id=?1 AND o.activity_id=?2
                       AND b.state='pending' AND o.status=0 AND g.id=?3 AND g.active=0",
                    params![build, worker.activity_id, slot.generation],
                )? == 1, "invalid build artifact binding");
                if let Some(evidence) = evidence {
                    store::identify(db, slot, evidence)?;
                } else {
                    store::created(db, slot)?;
                }
            }
            store::require_headroom(db)
        })
    }

    pub(crate) fn finish_build(
        &self,
        worker: &PendingOperation,
        slots: BuildSlots,
        request: BuildRequest<'_>,
        proof: [ArtifactEvidence; 2],
    ) -> Result<Status> {
        self.check_worker(worker)?;
        self.store.transact(|db| {
            let clock = self.clock.now();
            let now = utc(clock)?;
            if clock.monotonic >= worker.deadline {
                store::abandon_build(db, slots.id, worker.activity_id, now)?;
                return Ok(Status::Interrupted);
            }
            store::publish_build(
                db,
                worker.activity_id,
                slots,
                request,
                Self::build_guest(request.recipe),
                proof,
                now,
            )?;
            Ok(Status::Succeeded)
        })
    }

    pub(crate) fn fail_build(
        &self,
        worker: &PendingOperation,
        slots: BuildSlots,
        untouched: bool,
    ) -> Result<()> {
        self.fail_build_with(worker, slots, untouched, None)
    }

    /// As [`VmController::fail_build`]. A staged build failure records the
    /// worker's proven cause; an untouched build stays a cancellation.
    pub(crate) fn fail_build_with(
        &self,
        worker: &PendingOperation,
        slots: BuildSlots,
        untouched: bool,
        cause: Option<FailureReason>,
    ) -> Result<()> {
        self.check_worker(worker)?;
        self.store.transact(|db| {
            let now = utc(self.clock.now())?;
            if untouched {
                store::cancel_untouched_build(db, slots, worker.activity_id, now)
            } else {
                store::abandon_build_with(db, slots.id, worker.activity_id, now, cause)
            }
        })
    }

    #[cfg(test)]
    pub(crate) fn finish_linked_clone(
        &self,
        worker: &PendingOperation,
        slots: PairSlots,
        proof: Option<[ArtifactEvidence; 2]>,
    ) -> Result<Status> {
        self.finish_linked_clone_with(worker, slots, proof.ok_or(None))
    }

    /// As [`VmController::finish_linked_clone`]; a failed clone may carry
    /// its worker's proven cause.
    pub(crate) fn finish_linked_clone_with(
        &self,
        worker: &PendingOperation,
        slots: PairSlots,
        outcome: Result<[ArtifactEvidence; 2], Option<FailureReason>>,
    ) -> Result<Status> {
        self.check_worker(worker)?;
        self.store.transact(|db| {
            let clock = self.clock.now();
            let (proof, cause) = match outcome {
                Ok(pair) => (Some(pair), None),
                Err(cause) => (None, cause),
            };
            store::settle_clone(
                db,
                worker.activity_id,
                slots,
                proof,
                clock.monotonic < worker.deadline,
                utc(clock)?,
                cause,
            )
        })
    }

    /// Record an already sealed template and its retained bytes.
    pub fn seal_template(
        &self,
        template: SealedTemplate,
        host: HostObservation,
        resources: Resources,
    ) -> Result<Result<(), Refusal>> {
        let closed = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("VM gate unavailable"))?;
        if closed.rejects_fresh() {
            return Ok(Err(Refusal::Shutdown));
        }
        let result = self.store.transact(|db| {
            if store::has_failed_clone_cleanup(db)? {
                return Ok(Err(Refusal::Shutdown));
            }
            store::require_headroom(db)?;
            let now = utc(self.clock.now())?;
            let capacity = match self.policy.host(host, resources) {
                Ok(capacity) => capacity,
                Err(reason) => return Ok(Err(reason)),
            };
            if template.disk.images == 0 || template.disk.staging != 0 {
                return Ok(Err(Refusal::Invalid));
            }
            if let Err(reason) = self.validate_disk(db, capacity, template.disk)? {
                return Ok(Err(reason));
            }
            if store::count(
                db,
                "SELECT (SELECT count(*) FROM vm_templates)
                + (SELECT count(*) FROM vm_build_intents WHERE state<>'sealed')",
                [],
            )? >= 4
            {
                return Ok(Err(Refusal::TemplateLimit));
            }
            let written = db.execute(
                "INSERT INTO vm_templates VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT DO NOTHING",
                params![
                    template.id,
                    template.guest as i64,
                    template.recipe_digest,
                    template.disk_digest,
                    template.vars_digest,
                    template.disk.persistent()
                ],
            )?;
            if written == 0 {
                return Ok(Err(Refusal::Busy));
            }
            let id = store::activity(db, "arxd", template.id, Action::SealTemplate, now)?;
            store::finish(db, id, Status::Succeeded, now, None)?;
            store::require_headroom(db)?;
            Ok(Ok(()))
        });
        map_full(result)
    }

    /// Extend relative to the current UTC time. A due or claimed instance
    /// cannot be revived through extension. An admitted retry returns the
    /// expiry recorded by the original request, even when its TTL changes.
    pub fn extend_lease(
        &self,
        principal: &str,
        vm: Id,
        request_id: RequestId,
        seconds: u64,
    ) -> Result<Result<i64, Refusal>> {
        let closed = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("VM gate unavailable"))?;
        if validate_principal(principal).is_err() {
            return Ok(Err(Refusal::Invalid));
        }
        let result = self.store.transact(|db| {
            let now = utc(self.clock.now())?;
            let binding = store::Binding {
                principal,
                subject: Subject::Vm(vm),
                key: request_id,
                action: Action::Extend as i64,
            };
            match binding.replay(db, now, closed.closed)? {
                Ok(Some(prior)) => {
                    ensure!(
                        prior.operation.receipt.status == Status::Succeeded,
                        "invalid extension receipt"
                    );
                    return Ok(Ok(prior
                        .lease_end
                        .ok_or_else(|| anyhow::anyhow!("missing extension result"))?));
                }
                Err(reason) => {
                    store::refusal(db, principal, vm, Action::Extend, now, reason)?;
                    return Ok(Err(reason));
                }
                Ok(None) => {}
            }
            store::purge_tombstones(db, now)?;
            let requested = lease(now, Some(seconds));
            let decision = match (owned_vm(db, principal, vm)?, requested) {
                (_, Err(reason)) => Err(reason),
                (None, _) if store::expired_status(db, principal, vm)?.is_some() => {
                    Err(Refusal::Expired)
                }
                (None, _) => Err(Refusal::NotFound),
                (Some(lease), Ok(end)) => match lease.check(now) {
                    Err(reason) => Err(reason),
                    Ok(()) if closed.custody_uncertain || store::has_failed_clone_cleanup(db)? => {
                        Err(Refusal::Shutdown)
                    }
                    Ok(()) if end < lease.expires => Err(Refusal::Invalid),
                    Ok(()) => {
                        db.execute(
                            "UPDATE vm_instances SET expires_ms=?2 WHERE id=?1",
                            params![vm, end],
                        )?;
                        let id = binding
                            .record(db, now, Some(vm), Some(end))?
                            .receipt
                            .activity_id;
                        store::finish(db, id, Status::Succeeded, now, None)?;
                        store::require_headroom(db)?;
                        Ok(end)
                    }
                },
            };
            if let Err(reason) = decision {
                store::refusal(db, principal, vm, Action::Extend, now, reason)?;
            }
            Ok(decision)
        });
        map_full(result)
    }

    /// Newly won claims only. Existing claims remain separately recoverable.
    pub fn claim_expired(&self) -> Result<Vec<ExpiryClaim>> {
        self.store
            .transact(|db| store::expiry_claims(db, utc(self.clock.now())?))
    }

    pub fn expiry_intents(&self) -> Result<Vec<ExpiryClaim>> {
        self.store.transact(|db| store::expiry_intents(db))
    }

    /// Legacy metadata settlement. Durable artifacts require reaper custody.
    pub fn finish_expiry(&self, claim: ExpiryClaim, result: CleanupResult) -> Result<()> {
        self.store.transact(|db| {
            let now = utc(self.clock.now())?;
            let current: Option<i64> = db
                .query_row(
                    "SELECT cleanup_id FROM vm_instances
                WHERE id=?1 AND cleanup_route='lease' AND cleanup_id IS NOT NULL",
                    [claim.vm],
                    |row| row.get(0),
                )
                .optional()?;
            ensure!(current == Some(claim.activity_id), "invalid expiry claim");
            if result == CleanupResult::Unconfirmed {
                db.execute(
                    "UPDATE activity SET cleanup_confirmed=0 WHERE id=?1",
                    [claim.activity_id],
                )?;
                return Ok(());
            }
            Self::require_legacy_cleanup(db, claim.vm)?;
            store::interrupt_pending(db, claim.vm, now, true)?;
            store::record_expired(db, claim, now)
        })
    }

    /// Short-lived, owner-scoped confirmation of an expired instance.
    pub fn expired_status(&self, principal: &str, vm: Id) -> Result<Option<ExpiredVm>> {
        validate_principal(principal)?;
        self.store.transact(|db| {
            store::purge_tombstones(db, utc(self.clock.now())?)?;
            store::expired_status(db, principal, vm)
        })
    }

    /// Interrupted instance work stays fenced and charged until a reaper
    /// confirms process release and removal of transient artifacts.
    pub fn recovery_intents(&self) -> Result<Vec<RecoveryClaim>> {
        self.store.transact(|db| store::recovery_intents(db))
    }

    pub fn finish_recovery(&self, claim: RecoveryClaim, result: CleanupResult) -> Result<()> {
        self.store.transact(|db| {
            let vm = claim.vm;
            let now = utc(self.clock.now())?;
            if result == CleanupResult::Unconfirmed {
                ensure!(db.execute("UPDATE activity SET cleanup_confirmed=0 WHERE id=?2
                    AND EXISTS(SELECT 1 FROM vm_instances WHERE id=?1 AND state=3 AND cleanup_id=?2)",
                    params![vm, claim.activity_id])? == 1, "invalid recovery intent");
                return Ok(());
            }
            Self::require_legacy_cleanup(db, vm)?;
            store::record_recovered(db, claim, now)
        })
    }

    fn require_legacy_cleanup(db: &Connection, vm: Id) -> Result<()> {
        ensure!(
            store::count(
                db,
                "SELECT count(*) FROM vm_generations WHERE vm_id=?1",
                [vm]
            )? == 0,
            "durable VM cleanup requires custody"
        );
        Ok(())
    }

    /// Run once after open, under the store's supervisor lock, before the
    /// foreground exposes tools. Returns false if the turn could not start.
    pub fn sweep_once(&self, runtime: &CandidateRuntime<'_>) -> Result<bool> {
        self.sweep_with(runtime, &ClosedCustody)
    }

    /// Run periodic turns after the startup sweep. The caller owns the task
    /// and joins it during drain. UTC is sampled inside each claim transaction.
    pub async fn run_reaper(
        &self,
        runtime: &CandidateRuntime<'_>,
        ticks: &mut impl ReaperTick,
        shutdown: &Shutdown,
    ) -> Result<()> {
        self.run_reaper_with(runtime, &ClosedCustody, ticks, shutdown)
            .await
    }

    pub(crate) async fn run_reaper_with(
        &self,
        runtime: &(impl ReaperRuntime + Sync),
        custody: &(impl ArtifactCustody + Sync),
        ticks: &mut impl ReaperTick,
        shutdown: &Shutdown,
    ) -> Result<()> {
        loop {
            tokio::select! {
                biased;
                _ = shutdown.cancelled() => return Ok(()),
                more = ticks.next_tick() => {
                    if !more {
                        return Ok(());
                    }
                    // Recoverable store settlement failures keep claims armed
                    // and retry on the next tick rather than ending the reaper.
                    if self.sweep_with(runtime, custody).is_err()
                        && let Ok(mut gate) = self.gate.lock()
                    {
                        gate.custody_uncertain = true;
                    }
                }
            }
        }
    }

    pub(crate) fn sweep_with(
        &self,
        runtime: &impl ReaperRuntime,
        custody: &impl ArtifactCustody,
    ) -> Result<bool> {
        let mut schedule = self.schedule.lock().unwrap();
        if schedule.yield_to_vault() {
            return Ok(false);
        }
        let mut reserved = schedule.ordinary_owed;
        let target = schedule.target;
        let lock = self.sweeps.try_lock();
        if reserved {
            // An owed tick arriving during a vault item owns the next grant.
            schedule.handoff = false;
        }
        drop(schedule);
        let mut sweeps = match lock {
            Ok(sweeps) => sweeps,
            Err(TryLockError::WouldBlock) if reserved => self
                .sweeps
                .lock()
                .map_err(|_| anyhow::anyhow!("VM reaper unavailable"))?,
            Err(TryLockError::WouldBlock) => return Ok(false),
            Err(TryLockError::Poisoned(_)) => anyhow::bail!("VM reaper unavailable"),
        };
        let (mut items, mut uncertain) = {
            let mut gate = self
                .gate
                .lock()
                .map_err(|_| anyhow::anyhow!("VM gate unavailable"))?;
            gate.custody_uncertain = true;
            self.store.transact(|db| {
                let now = utc(self.clock.now())?;
                store::expiry_claims(db, now)?;
                let mut items = store::cleanup_kinds(db)?
                    .into_iter()
                    .map(|kind| store::cleanup_work(db, kind))
                    .collect::<Result<Vec<_>>>()?
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>();
                sweeps.prepare(db, &mut items)?;
                Ok((items, store::legacy_cleanup_uncertain(db)?))
            })?
        };
        if reserved && target.is_some() {
            items.sort_by_key(|work| work.kind.vm() == target);
        }
        for work in items {
            if !reserved && self.schedule.lock().unwrap().yield_between_items() {
                // Keep custody uncertain until a complete ordinary turn.
                // The unprocessed claims are already durable.
                return Ok(true);
            }
            if !sweeps.due(work.kind, self.clock.now().monotonic) {
                uncertain = true;
                continue;
            }
            uncertain |= !self.reconcile_cleanup(&mut sweeps, &work, runtime, custody, None)?;
            if reserved {
                self.schedule.lock().unwrap().ordinary_owed = false;
                reserved = false;
            }
        }
        // Committed static generations settle without any deletion route.
        // Unproven release keeps them recovering, which fences launches.
        let claims = self
            .store
            .transact(|db| store::static_vm::recovery_claims(db))?;
        for claim in claims {
            if !reserved && self.schedule.lock().unwrap().yield_between_items() {
                return Ok(true);
            }
            let start = self.clock.now().monotonic;
            let released = start
                .checked_add(STATIC_RELEASE_BUDGET)
                .is_some_and(|deadline| {
                    runtime.static_released(claim.vm, claim.generation, deadline)
                        && self.clock.now().monotonic < deadline
                });
            if self
                .store
                .transact(|db| {
                    store::static_vm::settle_recovery(db, claim, released, utc(self.clock.now())?)
                })
                .is_err()
            {
                uncertain = true;
            }
            if reserved {
                self.schedule.lock().unwrap().ordinary_owed = false;
                reserved = false;
            }
        }
        if reserved {
            self.schedule.lock().unwrap().ordinary_owed = false;
        }
        uncertain |= self.store.transact(|db| store::static_vm::recovering(db))?;
        self.gate
            .lock()
            .map_err(|_| anyhow::anyhow!("VM gate unavailable"))?
            .custody_uncertain = uncertain;
        Ok(true)
    }

    /// The waiter that prompted a yield can finish its handoff. A new vault
    /// job waits behind the reserved ordinary turn.
    pub(crate) fn prioritize_vault_sweep(
        &self,
        vm: Id,
        acquisition_budget: Duration,
        custody_budget: Duration,
    ) -> VaultSweepPriority<'_> {
        let mut schedule = self.schedule.lock().unwrap();
        schedule.vault_ready = true;
        schedule.target = Some(vm);
        VaultSweepPriority {
            schedule: &self.schedule,
            acquisition_deadline: self.clock.now().monotonic + acquisition_budget,
            custody_budget,
            custody_deadline: None,
            retry_at: None,
        }
    }

    pub(crate) fn sweep_vault_test(
        &self,
        vm: Id,
        runtime: &CandidateRuntime<'_>,
        turn: &mut VaultSweepPriority<'_>,
        fenced: &AtomicBool,
    ) -> Result<Option<VaultTestCleanup>> {
        self.sweep_vault_test_with(vm, runtime, &ClosedCustody, turn, fenced)
    }

    pub(crate) fn sweep_vault_test_with(
        &self,
        vm: Id,
        runtime: &impl ReaperRuntime,
        custody: &impl ArtifactCustody,
        turn: &mut VaultSweepPriority<'_>,
        fenced: &AtomicBool,
    ) -> Result<Option<VaultTestCleanup>> {
        use VaultTestCleanup::{Confirmed, Inconclusive, Unconfirmed};

        let waiting_until = turn.custody_deadline.unwrap_or(turn.acquisition_deadline);
        let now = self.clock.now().monotonic;
        if fenced.load(Ordering::SeqCst) || now >= waiting_until {
            return Ok(Some(Inconclusive));
        }
        if turn.retry_at.is_some_and(|retry| now < retry) {
            return Ok(None);
        }
        let mut schedule = self.schedule.lock().unwrap();
        schedule.vault_ready = true;
        if schedule.ordinary_owed && !schedule.handoff {
            return Ok(None);
        }
        let mut sweeps = match self.sweeps.try_lock() {
            Ok(sweeps) => sweeps,
            Err(TryLockError::WouldBlock) => return Ok(None),
            Err(TryLockError::Poisoned(_)) => anyhow::bail!("VM reaper unavailable"),
        };
        let now = self.clock.now().monotonic;
        if fenced.load(Ordering::SeqCst) || now >= waiting_until {
            return Ok(Some(Inconclusive));
        }
        schedule.handoff = false;
        drop(schedule);
        let deadline = *turn
            .custody_deadline
            .get_or_insert(now + turn.custody_budget);
        let work = self.store.transact(|db| {
            let now = utc(self.clock.now())?;
            match store::vault_test_expiry(db, vm, now)? {
                Some(claim) => store::cleanup_work(db, claim.into()),
                None => Ok(None),
            }
        })?;
        let Some(mut work) = work else {
            let confirmed = self.vault_test_released(vm)? && self.clock.now().monotonic < deadline;
            return Ok(Some(if confirmed { Confirmed } else { Inconclusive }));
        };
        let now = self.clock.now().monotonic;
        if now >= deadline {
            return Ok(Some(Inconclusive));
        }
        if let Some(retry) = sweeps.retry_at(work.kind).filter(|retry| now < *retry) {
            return Ok(turn.defer_until(Some(retry), deadline));
        }
        sweeps.observe_worker(&mut work);
        let confirmed =
            self.reconcile_cleanup(&mut sweeps, &work, runtime, custody, Some(deadline))?;
        if !confirmed {
            self.gate
                .lock()
                .map_err(|_| anyhow::anyhow!("VM gate unavailable"))?
                .custody_uncertain = true;
        }
        if confirmed && self.vault_test_released(vm)? {
            return Ok(Some(if self.clock.now().monotonic < deadline {
                Confirmed
            } else {
                Unconfirmed
            }));
        }
        if self.clock.now().monotonic >= deadline {
            return Ok(Some(Unconfirmed));
        }
        Ok(turn.defer_until(sweeps.retry_at(work.kind), deadline))
    }

    fn reconcile_cleanup(
        &self,
        sweeps: &mut Sweeps,
        work: &reaper::CleanupWork,
        runtime: &impl ReaperRuntime,
        custody: &impl ArtifactCustody,
        deadline: Option<Instant>,
    ) -> Result<bool> {
        let now = || self.clock.now().monotonic;
        // The scheduler shares this path with vault cleanup. Re-read the
        // exact claim before using an owner response or asking for takeover.
        let pending_release = self
            .store
            .transact(|db| store::native::recovery::requires_release(db, work.kind))?;
        let (released, finish_by) = reaper::release(
            work,
            runtime,
            deadline,
            pending_release,
            |cutoff| custody.release_orphan(work, cutoff, &|| self.clock.now()),
            now,
        );
        if pending_release {
            // Allow even a lost acknowledgement to settle the release turn,
            // but take no disposal action on this turn's old view.
            let pending = self
                .store
                .transact(|db| store::native::recovery::requires_release(db, work.kind))?;
            sweeps.settle(work, now(), !pending);
            return Ok(false);
        }
        if released
            && work.accounted
            && !self
                .store
                .transact(|db| store::claim_disposal(db, work, utc(self.clock.now())?))?
        {
            return Ok(false);
        }
        let outcome = reaper::dispose(work, custody, released, finish_by, now);
        let confirmed = match self
            .store
            .transact(|db| store::settle_cleanup(db, work, utc(self.clock.now())?, outcome))
        {
            Ok(confirmed) => confirmed,
            Err(_) => {
                sweeps.settle(work, self.clock.now().monotonic, false);
                return Ok(false);
            }
        };
        if work.release_only && outcome.released {
            sweeps.clear_retry(work.kind);
        } else {
            sweeps.settle(work, self.clock.now().monotonic, confirmed);
        }
        Ok(confirmed)
    }

    #[cfg(test)]
    pub(crate) fn hold_sweeps_for_test(&self, wait: impl FnOnce()) {
        let _lock = self.sweeps.lock().unwrap();
        wait();
    }

    #[cfg(test)]
    pub(super) fn reaper_test_store(&self) -> &VmStore {
        &self.store
    }

    /// Seal fresh admission while letting workers settle and callers replay.
    pub fn close_admission(&self) -> Result<()> {
        let mut gate = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("VM gate unavailable"))?;
        gate.closed = true;
        Ok(())
    }

    /// Called after the supervisor has joined the runner and drained workers.
    /// Mark residual metadata interrupted without invalidating a live worker.
    pub fn recover_after_drain(&self) -> Result<()> {
        let _sweep = self
            .sweeps
            .lock()
            .map_err(|_| anyhow::anyhow!("VM reaper unavailable"))?;
        let gate = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("VM gate unavailable"))?;
        ensure!(gate.closed, "close VM admission before recovery");
        self.store
            .transact(|db| store::recover(db, utc(self.clock.now())?))
    }

    fn validate_disk(
        &self,
        db: &Connection,
        capacity: Capacity,
        disk: DiskClaim,
    ) -> Result<Result<(), Refusal>> {
        let Some(additional) = disk.total().filter(|n| i64::try_from(*n).is_ok()) else {
            return Ok(Err(Refusal::Disk));
        };
        let Some(charged) = store::disk_bytes(db)? else {
            return Ok(Err(Refusal::Disk));
        };
        Ok(self.policy.disk(capacity, charged, additional))
    }
}

fn map_full<T>(result: Result<Result<T, Refusal>>) -> Result<Result<T, Refusal>> {
    match result {
        Err(error) if store::full(&error) => Ok(Err(Refusal::StateHeadroom)),
        result => result,
    }
}

fn utc(clock: ClockReading) -> Result<i64> {
    ensure!((0..=MAX_UTC_MS).contains(&clock.utc_ms), "invalid VM clock");
    Ok(clock.utc_ms)
}

fn timeout(operation: &Operation<'_>) -> Result<u64, Refusal> {
    match operation {
        Operation::Script {
            script,
            timeout_seconds,
        } => {
            if script.is_empty()
                || script.len() > 4096
                || script.as_bytes().contains(&0)
                || !(1..=120).contains(timeout_seconds)
            {
                return Err(Refusal::Invalid);
            }
            Ok(*timeout_seconds)
        }
        _ => Ok(120),
    }
}

struct VmLease {
    state: i64,
    expires: i64,
    cleanup_uncertain: bool,
}

impl VmLease {
    fn check(&self, now: i64) -> Result<(), Refusal> {
        if self.cleanup_uncertain {
            Err(Refusal::CleanupUnconfirmed)
        } else if self.state == 4 || self.expires <= now {
            Err(Refusal::Expired)
        } else {
            Ok(())
        }
    }
}

fn owned_vm(db: &Connection, principal: &str, vm: Id) -> Result<Option<VmLease>> {
    Ok(db
        .query_row(
            "SELECT state,expires_ms,coalesce(cleanup_route<>'lease',0)
             FROM vm_instances WHERE id=?1 AND principal=?2",
            params![vm, principal],
            |row| {
                Ok(VmLease {
                    state: row.get(0)?,
                    expires: row.get(1)?,
                    cleanup_uncertain: row.get(2)?,
                })
            },
        )
        .optional()?)
}

enum Target {
    Existing,
    Static,
    Clone { template: Id, expires: i64 },
}

impl Target {
    fn insert(
        self,
        db: &Connection,
        request: &Request<'_>,
        id: i64,
        resources: Resources,
    ) -> Result<()> {
        match self {
            Self::Existing => {}
            Self::Static => return Ok(()),
            Self::Clone { template, expires } => {
                db.execute("INSERT INTO vm_instances(id,template,principal,expires_ms,dashboard_cpus,dashboard_memory)
                    VALUES(?1,?2,?3,?4,?5,?6)",
                    params![request.vm, template, request.principal, expires,
                        (resources.vcpus <= 1024).then_some(resources.vcpus),
                        (resources.guest_ram <= 9_007_199_254_740_991).then_some(resources.guest_ram)])?;
            }
        }
        if request.disk.total().is_some_and(|n| n > 0) {
            db.execute(
                "INSERT INTO vm_disk_claims VALUES(?1,?2,?3,?4,?5)",
                params![
                    id,
                    request.vm,
                    Option::<Id>::None,
                    request.disk.persistent(),
                    request.disk.staging
                ],
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod static_compose_tests {
    use super::*;

    #[test]
    fn static_compose_template_is_ineligible_for_test_clone() -> Result<()> {
        let (_dir, store, _, _) = store::backup_owner_fixture()?;
        let template = [5u8; 16];
        let digest = [6u8; 32];
        let now = 1_800_000_000_000;
        let request = Request {
            principal: "alice",
            vm: [7; 16],
            request_id: super::super::RequestId::parse(&format!("{now:016x}{:016x}", 1)).unwrap(),
            operation: Operation::Clone {
                template,
                lease_seconds: None,
            },
            disk: DiskClaim {
                images: 4096,
                ..Default::default()
            },
        };
        store.transact(|db| {
            db.execute("INSERT INTO vm_templates VALUES(?1,1,?2,?2,?2,4096)", params![template, digest])?;
            db.execute(
                "INSERT INTO vm_build_intents(id,recipe_id,recipe_digest,staging_bytes,template_id,health_verified,state)
                 VALUES(?1,?2,?3,0,?4,1,'sealed')",
                params![[4u8; 16], hex::encode(crate::vm::qualification_record::DockerRecipe::identity(digest).recipe_id), digest, template],
            )?;
            assert!(matches!(target(db, &request, now)?, Err(Refusal::NotFound)));
            db.execute("UPDATE vm_build_intents SET recipe_id=?1 WHERE template_id=?2",
                params![hex::encode(&digest[..16]), template])?;
            assert!(matches!(target(db, &request, now)?, Ok(Target::Clone { .. })));
            Ok(())
        })
    }
}

fn target(db: &Connection, request: &Request<'_>, now: i64) -> Result<Result<Target, Refusal>> {
    if let Operation::Clone {
        template,
        lease_seconds,
    } = request.operation
    {
        let expires = match lease(now, lease_seconds) {
            Ok(expires) if request.disk.images > 0 => expires,
            _ => return Ok(Err(Refusal::Invalid)),
        };
        if store::count(
            db,
            "SELECT EXISTS(SELECT 1 FROM vm_templates WHERE id=?1)",
            [template],
        )? == 0
            || store::static_vm::is_compose_template(db, template)?
        {
            return Ok(Err(Refusal::NotFound));
        }
        if store::static_vm::instance_count(db)? >= store::MAX_INSTANCES {
            return Ok(Err(Refusal::InstanceLimit));
        }
        store::purge_tombstones(db, now)?;
        if store::count(
            db,
            "SELECT EXISTS(SELECT 1 FROM vm_instances WHERE id=?1)
            OR EXISTS(SELECT 1 FROM vm_tombstones WHERE id=?1)
            OR EXISTS(SELECT 1 FROM vm_static_instances WHERE id=?1)",
            [request.vm],
        )? != 0
        {
            return Ok(Err(Refusal::Busy));
        }
        return Ok(Ok(Target::Clone { template, expires }));
    }
    if let Some(state) = store::static_vm::owned_state(db, request.principal, request.vm)? {
        return static_target(db, request, state);
    }
    let Some(lease) = owned_vm(db, request.principal, request.vm)? else {
        return Ok(Err(Refusal::NotFound));
    };
    if let Err(reason) = lease.check(now) {
        return Ok(Err(reason));
    }
    let state = lease.state;
    if !matches!(state, 1 | 2) || store::snapshot::has_pending_vm_activity(db, request.vm)? {
        return Ok(Err(Refusal::Busy));
    }
    if request.disk.images != 0 || request.disk.metadata != 0 {
        return Ok(Err(Refusal::Invalid));
    }
    if store::snapshot::unsettled(db, request.vm)? {
        return Ok(Err(Refusal::CleanupUnconfirmed));
    }
    match request.operation {
        Operation::Start if state == 1 => {
            if store::count(
                db,
                "SELECT EXISTS(SELECT 1 FROM vm_instances WHERE state IN (2,3,4))
                OR EXISTS(SELECT 1 FROM vm_static_instances WHERE state IN (0,2,3))
                OR EXISTS(SELECT 1 FROM activity WHERE action=3 AND status=0)
                OR EXISTS(SELECT 1 FROM vm_build_intents WHERE state<>'sealed')
                OR EXISTS(SELECT 1 FROM vm_operations WHERE status=0 AND binding_kind='template')",
                [],
            )? != 0
            {
                return Ok(Err(Refusal::Busy));
            }
            Ok(Ok(Target::Existing))
        }
        Operation::Stop | Operation::Script { .. } if state == 2 => Ok(Ok(Target::Existing)),
        _ => Ok(Err(Refusal::Busy)),
    }
}

pub(crate) struct CatalogEntry {
    pub kind: u8,
    pub id: Id,
    pub recipe: Option<String>,
    pub guest: Option<Guest>,
}

pub(crate) struct ActivityReceipt {
    pub id: i64,
    pub action: i64,
    pub state: i64,
    pub reason: Option<i64>,
    pub vm: Option<Id>,
    pub operation: Option<Id>,
    pub started_ms: i64,
    pub finished_ms: Option<i64>,
    pub cleanup: Option<bool>,
    pub publication_failure: Option<FailureReason>,
}

impl<C: Clock> VmController<C> {
    pub(crate) fn vault_test_template(
        &self,
        recipe: Id,
        digest: [u8; 32],
        guest: Guest,
    ) -> Result<Option<Id>> {
        self.store.transact(|db| {
            let Some((template, _)) = store::sealed_pair(db, &hex::encode(recipe), digest)? else {
                return Ok(None);
            };
            let compatible = store::count(
                db,
                "SELECT count(*) FROM vm_templates WHERE id=?1 AND guest=?2",
                params![template, guest as i64],
            )? == 1;
            Ok(compatible.then_some(template))
        })
    }

    pub(crate) fn vault_test_key(&self) -> Result<RequestId> {
        let issued_at_ms = utc(self.clock.now())?;
        let mut key = store::new_id()?;
        key[..8].copy_from_slice(&issued_at_ms.to_be_bytes());
        Ok(RequestId { key, issued_at_ms })
    }

    /// Accelerate the existing TEST lease cleanup. The reserved daemon
    /// principal cannot be authenticated by a foreground client.
    pub(crate) fn expire_vault_test(&self, vm: Id) -> Result<()> {
        self.store.transact(|db| {
            db.execute(
                "UPDATE vm_instances SET expires_ms=min(expires_ms,?2)
                 WHERE id=?1 AND principal='arxd'",
                params![vm, utc(self.clock.now())?],
            )?;
            Ok(())
        })
    }

    pub(crate) fn vault_test_released(&self, vm: Id) -> Result<bool> {
        self.store.transact(|db| {
            let retired = store::tombstone_proof(db, "arxd", vm)?
                .is_some_and(|(_, confirmed)| confirmed == Some(true));
            let charge = store::count(db, "SELECT count(*) FROM vm_disk_claims WHERE vm=?1", [vm])?;
            Ok(retired && charge == 0)
        })
    }

    pub(crate) fn foreground_catalog(
        &self,
        principal: &str,
        recipes: [Option<Id>; 3],
        after: (u8, Id),
        limit: u32,
    ) -> Result<Vec<CatalogEntry>> {
        self.store.transact(|db| {
            let mut query = db.prepare(
                "SELECT kind,id,recipe,guest FROM (
                   SELECT 1 AS kind, ?1 AS id, NULL AS recipe, NULL AS guest WHERE ?1 IS NOT NULL
                   UNION
                   SELECT 1, ?6, NULL, NULL WHERE ?6 IS NOT NULL
                   UNION
                   SELECT 1, ?7, NULL, NULL WHERE ?7 IS NOT NULL
                   UNION ALL
                   SELECT 2, b.template_id, b.recipe_id, t.guest FROM vm_build_intents b
                     JOIN vm_templates t ON t.id=b.template_id
                     WHERE b.state='sealed'
                   UNION ALL
                   SELECT 3, b.operation_id, b.recipe_id, NULL FROM vm_build_intents b
                     JOIN vm_operations o USING(operation_id)
                     WHERE o.principal=?2 AND b.state='pending' AND o.status=0
                 ) WHERE (kind,id)>(?3,?4) ORDER BY kind,id LIMIT ?5",
            )?;
            let rows = query.query_map(
                params![
                    recipes[0], principal, after.0, after.1, limit, recipes[1], recipes[2]
                ],
                |row| {
                    let kind = row.get(0)?;
                    let id = row.get(1)?;
                    let guest = match (kind, row.get::<_, Option<i64>>(3)?) {
                        (1, None) if id == Self::windows_recipe_id() => Some(Guest::WindowsX86_64),
                        (1, None) | (2, Some(1)) => Some(Guest::UbuntuX86_64),
                        (2, Some(2)) => Some(Guest::WindowsX86_64),
                        (3, None) => None,
                        _ => return Err(rusqlite::Error::InvalidQuery),
                    };
                    Ok(CatalogEntry {
                        kind,
                        id,
                        recipe: row.get(2)?,
                        guest,
                    })
                },
            )?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    pub(crate) fn foreground_activity(
        &self,
        principal: &str,
        before: Option<i64>,
        exact: Option<i64>,
        limit: u8,
    ) -> Result<Vec<ActivityReceipt>> {
        self.store.transact(|db| {
            let mut query = db.prepare(
                "SELECT a.id,a.action,a.status,a.reason,a.vm,coalesce(o.operation_id,c.id),
                        a.started_ms,a.finished_ms,a.cleanup_confirmed,
                        a.publication_failure
                   FROM activity a LEFT JOIN vm_operations o ON o.activity_id=a.id
                   LEFT JOIN vm_compose_operations c ON c.activity_id=a.id
                   WHERE a.principal=?1 AND a.actor='agent'
                     AND (?2 IS NULL OR a.id<?2) AND (?3 IS NULL OR a.id=?3)
                   ORDER BY a.id DESC LIMIT ?4",
            )?;
            let rows = query.query_map(params![principal, before, exact, limit], |row| {
                Ok(ActivityReceipt {
                    id: row.get(0)?,
                    action: row.get(1)?,
                    state: row.get(2)?,
                    reason: row.get(3)?,
                    vm: row.get(4)?,
                    operation: row.get(5)?,
                    started_ms: row.get(6)?,
                    finished_ms: row.get(7)?,
                    cleanup: row.get(8)?,
                    publication_failure: row
                        .get::<_, Option<i64>>(9)?
                        .map(FailureReason::from_code)
                        .transpose()?,
                })
            })?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Check a validated replay before any native decision or fixture lookup.
    /// All callers serialize fresh admission, cancellation and this check in
    /// the foreground supervisor gate. The eventual admission checks it again.
    ///
    /// This is a transactional write seam, not a read: `Binding::replay` can
    /// advance the persisted UTC replay high-water mark and prune expired
    /// ledger entries in the same transaction.
    pub(crate) fn foreground_replay(
        &self,
        principal: &str,
        key: RequestId,
        subject: Subject<'_>,
        action: i64,
    ) -> Result<Result<Option<store::Replay>, Refusal>> {
        // Only closed admission preempts replay here. Custody uncertainty is
        // decided by the eventual admission after scope and subject checks.
        let gate = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("VM gate unavailable"))?;
        let closed = gate.closed;
        let binding = store::Binding {
            principal,
            key,
            subject,
            action,
        };
        map_full(
            self.store
                .transact(|db| binding.replay(db, utc(self.clock.now())?, closed)),
        )
    }

    #[cfg(test)]
    pub(crate) fn foreground_operation_ref(&self, principal: &str, receipt: i64) -> Result<Id> {
        self.store.transact(|db| {
            Ok(db.query_row(
                "SELECT operation_id FROM vm_operations WHERE activity_id=?1 AND principal=?2",
                params![receipt, principal],
                |row| row.get(0),
            )?)
        })
    }

    /// Store a bounded refusal with correlation verified in the same transaction.
    ///
    /// This is a bounded, audit-only write: at most one refusal row (with its
    /// audit pruning) after the headroom check, and no operation, ledger or
    /// VM state change.
    pub(crate) fn foreground_refusal(
        &self,
        principal: &str,
        action: i64,
        vm: Option<Id>,
        reason: Refusal,
    ) -> Result<Result<(), Refusal>> {
        map_full(self.store.transact(|db| {
            store::require_headroom(db)?;
            let now = utc(self.clock.now())?;
            store::refusal_for(db, principal, vm, action, now, reason)?;
            Ok(Ok(()))
        }))
    }

    /// Read-only scope and subject-state check in the same order as fresh
    /// admission, so a foreground refusal never reports native eligibility
    /// before an unknown, foreign, expired or busy subject.
    pub(crate) fn foreground_precheck(
        &self,
        principal: &str,
        subject: ForegroundSubject,
    ) -> Result<Result<(), Precheck>> {
        if validate_principal(principal).is_err() {
            return Ok(Err(Precheck::Refused(Refusal::NotFound)));
        }
        Ok(self
            .precheck_inner(principal, subject)?
            .map_err(|reason| match reason {
                PrecheckInner::Offline => Precheck::NotReady,
                PrecheckInner::Refused(reason) => Precheck::Refused(reason),
            }))
    }

    fn precheck_inner(
        &self,
        principal: &str,
        subject: ForegroundSubject,
    ) -> Result<Result<(), PrecheckInner>> {
        let refused = |result: Result<(), Refusal>| result.map_err(PrecheckInner::Refused);
        self.store.transact(|db| {
            let now = utc(self.clock.now())?;
            let operation = match subject {
                ForegroundSubject::Template(template) => {
                    let sealed = store::count(
                        db,
                        "SELECT EXISTS(SELECT 1 FROM vm_templates WHERE id=?1)",
                        [template],
                    )? != 0;
                    return Ok(refused(if sealed {
                        Ok(())
                    } else {
                        Err(Refusal::NotFound)
                    }));
                }
                ForegroundSubject::Extend(vm) => {
                    return Ok(refused(match owned_vm(db, principal, vm)? {
                        None if store::expired_status(db, principal, vm)?.is_some() => {
                            Err(Refusal::Expired)
                        }
                        None => Err(Refusal::NotFound),
                        Some(lease) => lease.check(now),
                    }));
                }
                ForegroundSubject::Start(_) => Operation::Start,
                ForegroundSubject::Stop(_) => Operation::Stop,
                ForegroundSubject::Script(_) => Operation::Script {
                    script: "",
                    timeout_seconds: 0,
                },
            };
            let vm = subject.vm();
            if matches!(subject, ForegroundSubject::Script(_))
                && store::static_vm::owned_state(db, principal, vm)?
                    .is_some_and(|state| state != StaticState::Running)
            {
                return Ok(Err(PrecheckInner::Offline));
            }
            match owned_vm(db, principal, vm)? {
                None if store::expired_status(db, principal, vm)?.is_some() => {
                    return Ok(Err(PrecheckInner::Refused(Refusal::Expired)));
                }
                // A live, owned generation that is not running has no channel.
                Some(lease)
                    if lease.check(now).is_ok()
                        && lease.state != 2
                        && matches!(subject, ForegroundSubject::Script(_)) =>
                {
                    return Ok(Err(PrecheckInner::Offline));
                }
                _ => {}
            }
            let request = Request {
                principal,
                request_id: RequestId {
                    key: [0; 16],
                    issued_at_ms: 0,
                },
                vm,
                disk: DiskClaim::default(),
                operation,
            };
            Ok(refused(target(db, &request, now)?.map(|_| ())))
        })
    }

    /// One snapshot of up to `limit` caller-owned rows, so the foreground can
    /// ask for a look-ahead row without a second transaction.
    pub(crate) fn foreground_list(
        &self,
        principal: &str,
        after: Option<Id>,
        limit: u32,
    ) -> Result<Result<Vec<VmStatus>, Refusal>> {
        if validate_principal(principal).is_err() || !(1..=21).contains(&limit) {
            return Ok(Err(Refusal::Invalid));
        }
        self.store.transact(|db| {
            Ok(Ok(store::vm_status_rows(
                db,
                principal,
                None,
                after,
                limit,
                utc(self.clock.now())?,
            )?))
        })
    }
}

impl<C: Clock> VmController<C> {
    /// Whether the last reaper turn left custody or accounting unresolved.
    /// The foreground reports an unresolved final drain from this flag.
    pub(crate) fn foreground_custody_uncertain(&self) -> Result<bool> {
        Ok(self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("VM gate unavailable"))?
            .custody_uncertain)
    }
}

/// Why a foreground mutation cannot proceed before eligibility is checked.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Precheck {
    Refused(Refusal),
    /// A script names an owned live VM that has no running boot.
    NotReady,
}

enum PrecheckInner {
    Refused(Refusal),
    Offline,
}

/// The subject a foreground mutation names, for [`VmController::foreground_precheck`].
#[derive(Clone, Copy)]
pub(crate) enum ForegroundSubject {
    Template(Id),
    Start(Id),
    Stop(Id),
    Script(Id),
    Extend(Id),
}

impl ForegroundSubject {
    fn vm(self) -> Id {
        match self {
            Self::Template(id)
            | Self::Start(id)
            | Self::Stop(id)
            | Self::Script(id)
            | Self::Extend(id) => id,
        }
    }
}

fn static_target(
    db: &Connection,
    request: &Request<'_>,
    state: StaticState,
) -> Result<Result<Target, Refusal>> {
    if request.disk.total() != Some(0) {
        return Ok(Err(Refusal::Invalid));
    }
    if store::snapshot::has_pending_vm_activity(db, request.vm)? {
        return Ok(Err(Refusal::Busy));
    }
    if store::snapshot::unsettled(db, request.vm)? {
        return Ok(Err(Refusal::CleanupUnconfirmed));
    }
    match (&request.operation, state) {
        (Operation::Start, StaticState::Stopped) => {
            if store::static_vm::launch_busy(db)? {
                return Ok(Err(Refusal::Busy));
            }
        }
        (Operation::Stop | Operation::Script { .. }, StaticState::Running) => {}
        (Operation::Snapshot { .. } | Operation::Restore { .. }, _) => {
            return Ok(Err(Refusal::NotFound));
        }
        _ => return Ok(Err(Refusal::Busy)),
    }
    Ok(Ok(Target::Static))
}

impl<C: Clock> VmController<C> {
    pub(crate) fn foreground_host_id(&self) -> Result<Id> {
        self.store.transact(|db| store::static_vm::host_id(db))
    }

    pub(crate) fn foreground_static_rows(
        &self,
        principal: &str,
        exact: Option<Id>,
        after: Option<Id>,
        limit: u32,
    ) -> Result<Vec<store::static_vm::StaticRow>> {
        self.store
            .transact(|db| store::static_vm::rows(db, principal, exact, after, limit))
    }

    pub(crate) fn foreground_static_template(
        &self,
        template: Id,
        digest: Option<[u8; 32]>,
    ) -> Result<Result<(), Refusal>> {
        self.store.transact(|db| {
            let recipe = db
                .query_row(
                    "SELECT recipe_digest FROM vm_templates WHERE id=?1 AND guest=1",
                    [template],
                    |row| row.get::<_, [u8; 32]>(0),
                )
                .optional()?;
            Ok(match (recipe, digest) {
                (None, _) => Err(Refusal::NotFound),
                (Some(recipe), Some(digest)) if recipe == digest => Ok(()),
                _ => Err(Refusal::RecipeUnqualified),
            })
        })
    }

    #[cfg(test)]
    #[allow(clippy::too_many_arguments, clippy::type_complexity)]
    pub(crate) fn admit_static(
        &self,
        binding: store::Binding<'_>,
        name: &super::static_web::StaticName,
        template: Id,
        source: (PairSlots, [ArtifactEvidence; 2]),
        web_digest: [u8; 32],
        disk: DiskClaim,
        host: HostObservation,
        resources: Resources,
    ) -> Result<Result<(Id, PairSlots, PendingOperation, OperationRef), Refusal>> {
        let gate = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("VM gate unavailable"))?;
        map_full(self.store.transact(|db| {
            let clock = self.clock.now();
            let now = utc(clock)?;
            match binding.replay(
                db,
                now,
                gate.rejects_fresh() || store::has_failed_clone_cleanup(db)?,
            )? {
                Ok(None) => {}
                Ok(Some(_)) => return Ok(Err(Refusal::Busy)),
                Err(reason) => return Ok(Err(reason)),
            }
            if store::count(db, "SELECT count(*) FROM vm_operations WHERE status=0", [])?
                >= store::MAX_CALLER_PENDING
            {
                return Ok(Err(Refusal::PendingFull));
            }
            if store::static_vm::instance_count(db)? >= store::MAX_INSTANCES {
                return Ok(Err(Refusal::InstanceLimit));
            }
            if store::static_vm::name_occupied(db, name)? {
                return Ok(Err(Refusal::NameUnavailable));
            }
            if store::static_vm::launch_busy(db)? {
                return Ok(Err(Refusal::Busy));
            }
            if store::static_vm::is_compose_template(db, template)? {
                return Ok(Err(Refusal::RecipeUnqualified));
            }
            let Some(provenance) = store::static_vm::source(db, template, source, web_digest)?
            else {
                return Ok(Err(Refusal::RecipeUnqualified));
            };
            let capacity = match self.policy.host(host, resources) {
                Ok(capacity) => capacity,
                Err(reason) => return Ok(Err(reason)),
            };
            if let Err(reason) = self.validate_disk(db, capacity, disk)? {
                return Ok(Err(reason));
            }
            let vm = store::static_vm::allocate_id(db)?;
            let worker = self.worker(clock, 120)?;
            let reference = binding.record(db, now, Some(vm), None)?;
            let slots = store::static_vm::reserve(
                db,
                vm,
                name,
                binding.principal,
                template,
                provenance,
                disk,
            )?;
            store::require_headroom(db)?;
            Ok(Ok((
                vm,
                slots,
                PendingOperation {
                    activity_id: reference.receipt.activity_id,
                    ..worker
                },
                reference,
            )))
        }))
    }

    #[cfg(test)]
    pub(crate) fn finish_static(
        &self,
        worker: &PendingOperation,
        slots: PairSlots,
        outcome: Result<[ArtifactEvidence; 2], super::CopyFailure>,
    ) -> Result<Status> {
        self.check_worker(worker)?;
        self.store.transact(|db| {
            let clock = self.clock.now();
            // A late worker can no longer vouch for its own effects.
            let outcome = if clock.monotonic < worker.deadline {
                outcome
            } else {
                Err(super::CopyFailure::unknown(
                    FailureReason::CleanupUnconfirmed,
                ))
            };
            store::static_vm::settle_create(db, worker.activity_id, slots, outcome, utc(clock)?)
        })
    }

    /// The committed pair of an owned, stopped TEST VM, for a fresh identity
    /// check by the launcher after reopen or commit.
    #[cfg(test)]
    pub(crate) fn test_custody(
        &self,
        principal: &str,
        vm: Id,
    ) -> Result<Option<(PairSlots, [ArtifactEvidence; 2])>> {
        self.store
            .transact(|db| store::active_test_pair(db, principal, vm))
    }

    #[cfg(test)]
    pub(crate) fn has_test_generation(&self, principal: &str, vm: Id) -> Result<bool> {
        self.store
            .transact(|db| store::has_test_generation(db, principal, vm))
    }

    /// The committed pair of an owned, stopped static VM, for a fresh
    /// identity check by the launcher after reopen.
    #[cfg(test)]
    pub(crate) fn static_custody(
        &self,
        principal: &str,
        vm: Id,
    ) -> Result<Option<(PairSlots, [ArtifactEvidence; 2], Id)>> {
        self.store.transact(|db| {
            let Some((slots, evidence)) = store::static_vm::active_pair(db, principal, vm)? else {
                return Ok(None);
            };
            let identity = store::static_vm::origin_generation(db, vm)?;
            Ok(Some((slots, evidence, identity)))
        })
    }
}

/// Bound for one static release proof inside a reaper turn.
const STATIC_RELEASE_BUDGET: Duration = Duration::from_secs(1);

impl<C: Clock> VmController<C> {
    pub(crate) fn publication_intent(&self) -> Result<Option<store::publish::Intent>> {
        self.store.transact(|db| store::publish::current(db))
    }

    pub(crate) fn publication_status(
        &self,
        principal: &str,
        vm: Id,
    ) -> Result<Result<store::publish::SiteStatus, Refusal>> {
        self.store.transact(|db| {
            if store::static_vm::rows(db, principal, Some(vm), None, 1)?.is_empty() {
                return Ok(Err(Refusal::NotFound));
            }
            if let Some(intent) = store::publish::current(db)?.filter(|intent| intent.vm == vm) {
                let hosts = intent
                    .hosts
                    .into_iter()
                    .map(|host| store::publish::HostStatusView {
                        position: host.position,
                        state: host.state,
                        ambiguous: host.ambiguous,
                        route_slot: host.route.as_ref().map(|r| r.route_slot),
                        project_id: host.route.as_ref().map(|r| r.project_id.clone()),
                        service: host.route.as_ref().map(|r| r.service.clone()),
                        reason: host.reason,
                    })
                    .collect();
                return Ok(Ok(store::publish::SiteStatus {
                    phase: Some(intent.phase),
                    reason: intent.reason,
                    hosts,
                }));
            }
            let reason = db
                .query_row(
                    "SELECT publication_failure FROM vm_static_instances WHERE id=?1",
                    [vm],
                    |row| row.get::<_, Option<i64>>(0),
                )?
                .map(FailureReason::from_code)
                .transpose()?;
            Ok(Ok(store::publish::SiteStatus {
                phase: None,
                reason,
                hosts: Vec::new(),
            }))
        })
    }

    pub(crate) fn admit_publication(
        &self,
        principal: &str,
        key: RequestId,
        vm: Id,
        intent: Option<(&store::publish::Intent, &super::static_web::StaticName)>,
    ) -> Result<Result<OperationRef, Refusal>> {
        let gate = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("VM gate unavailable"))?;
        let now = utc(self.clock.now())?;
        let action = if intent.is_some() {
            PUBLISH_ACTION
        } else {
            UNPUBLISH_ACTION
        };
        let binding = store::Binding {
            principal,
            key,
            subject: Subject::Vm(vm),
            action,
        };
        map_full(self.store.transact(|db| {
            match binding.replay(db, now, gate.closed)? {
                Err(reason) => return Ok(Err(reason)),
                Ok(Some(replay)) => return Ok(Ok(replay.operation)),
                Ok(None) => {}
            }
            let mut rows = store::static_vm::rows(db, principal, Some(vm), None, 1)?;
            let Some(row) = rows.pop() else {
                return Ok(Err(Refusal::NotFound));
            };
            if store::count(
                db,
                "SELECT count(*) FROM activity WHERE status=0 AND actor='agent'",
                [],
            )? >= store::MAX_CALLER_PENDING
            {
                return Ok(Err(Refusal::PendingFull));
            }
            let current = store::publish::current(db)?;
            if let Some((intent, name)) = intent {
                if store::snapshot::unsettled(db, vm)? {
                    return Ok(Err(Refusal::Busy));
                }
                if current.is_some() {
                    return Ok(Err(Refusal::PublishCleanupPending));
                }
                if row.name != *name {
                    return Ok(Err(Refusal::PublishTargetMismatch));
                }
                if row.state != StaticState::Running
                    || store::count(
                        db,
                        "SELECT count(*) FROM activity WHERE vm=?1 AND status=0",
                        [vm],
                    )? != 0
                {
                    return Ok(Err(Refusal::NotReady));
                }
                if store::count(
                    db,
                    "SELECT count(*) FROM vm_generations WHERE static_id=?1 AND id=?2 AND active=1",
                    params![vm, intent.generation],
                )? == 0
                {
                    return Ok(Err(Refusal::NotReady));
                }
                for route in intent.hosts.iter().filter_map(|host| host.route.as_ref()) {
                    if store::count(db, "SELECT count(*) FROM vm_compose_projects p WHERE vm=?1 AND project=?2
                        AND (recovery_id IS NOT NULL
                             OR EXISTS (SELECT 1 FROM vm_compose_operations o
                                 WHERE o.vm=p.vm AND o.project=p.project AND o.receipt IS NULL AND o.closed IS NULL)
                             OR EXISTS (SELECT 1 FROM vm_compose_backup_ops b
                                 WHERE b.vm=p.vm AND b.project=p.project AND b.status='pending'))",
                        params![vm,route.project_id])? != 0 {return Ok(Err(Refusal::Busy));}
                }
                store::publish::reserve(db, intent)?;
            } else if let Some(current) = current.filter(|intent| intent.vm == vm) {
                store::publish::update(
                    db,
                    &current.nonce,
                    store::publish::Update::Phase(store::publish::Phase::Revoking, None),
                )?;
            } else if store::snapshot::unsettled(db, vm)? {
                return Ok(Err(Refusal::Busy));
            }
            if intent.is_none() {
                db.execute(
                    "UPDATE vm_static_instances SET publication_failure=NULL WHERE id=?1",
                    [vm],
                )?;
            }
            let operation = binding.record(db, now, Some(vm), None)?;
            store::require_headroom(db)?;
            Ok(Ok(operation))
        }))
    }

    pub(crate) fn update_publication(
        &self,
        nonce: &str,
        update: store::publish::Update<'_>,
    ) -> Result<()> {
        self.store
            .transact(|db| store::publish::update(db, nonce, update))
    }

    pub(crate) fn reconcile_publication(
        &self,
        intent: &store::publish::Intent,
        update: store::publish::Update<'_>,
        reason: Option<FailureReason>,
        released: bool,
    ) -> Result<()> {
        self.store.transact(|db| {
            // The audit records the effective cause, read before the release
            // removes the intent.
            let reason = store::publish::effective_cause(db, &intent.nonce, reason)?;
            store::publish::update(db, &intent.nonce, update)?;
            // One daemon receipt for loss or a completed reconciliation.
            // Retry turns with unchanged pending cleanup cannot fill the audit.
            if released || intent.phase == store::publish::Phase::Published {
                let now = utc(self.clock.now())?;
                db.execute(
                    "INSERT INTO activity(principal,actor,vm_owner,vm,action,started_ms,
                       status,finished_ms,terminal_order,cleanup_confirmed,publication_failure)
                     SELECT 'arxd','daemon',principal,id,17,?2,?3,?2,
                         (SELECT coalesce(max(terminal_order),0)+1 FROM activity),?4,?5
                     FROM vm_static_instances WHERE id=?1",
                    params![
                        intent.vm,
                        now,
                        if released { 1 } else { 2 },
                        released,
                        reason.map(|value| value as i64)
                    ],
                )?;
                store::prune(db)?;
            }
            Ok(())
        })
    }

    /// Commits `published` and the successful receipt in one transaction,
    /// only while `authorized` holds inside it and the intent is still
    /// `publishing`. Returns `false`, with no change, otherwise.
    pub(crate) fn commit_publication(
        &self,
        operation: OperationRef,
        nonce: &str,
        authorized: impl FnOnce() -> bool,
    ) -> Result<bool> {
        let now = utc(self.clock.now())?;
        self.store.transact(|db| {
            if !authorized() || !store::publish::commit(db, nonce)? {
                return Ok(false);
            }
            store::publish::finish(db, operation.receipt.activity_id, now, None, None)?;
            Ok(true)
        })
    }

    /// Publication receipts still pending, for the final drain's settlement
    /// proof.
    pub(crate) fn publication_receipts_pending(&self) -> Result<usize> {
        self.store
            .transact(|db| store::publish::pending_receipts(db))
    }

    /// Settles publication receipts that lost their terminal write; see
    /// [`store::publish::settle_orphans`].
    pub(crate) fn settle_orphaned_publications(
        &self,
        live: &std::collections::HashSet<i64>,
        unconfirmed: &std::collections::HashSet<i64>,
    ) -> Result<Vec<i64>> {
        let now = utc(self.clock.now())?;
        self.store
            .transact(|db| store::publish::settle_orphans(db, live, unconfirmed, now))
    }

    pub(crate) fn finish_publication(
        &self,
        operation: OperationRef,
        update: Option<(&str, store::publish::Update<'_>)>,
        failure: Option<FailureReason>,
        cleanup: Option<bool>,
    ) -> Result<()> {
        let now = utc(self.clock.now())?;
        self.store.transact(|db| {
            if let Some((nonce, update)) = update {
                store::publish::update(db, nonce, update)?;
            }
            store::publish::finish(db, operation.receipt.activity_id, now, failure, cleanup)
        })
    }
}

#[cfg(all(windows, target_arch = "x86_64"))]
impl<C: Clock> VmController<C> {
    pub(crate) fn native_store(&self) -> &VmStore {
        &self.store
    }

    /// Capture the sealed source and exact stopped pair in the same transaction
    /// that invalidates its release and installs the start's writer epoch.
    pub(crate) fn admit_native_start(
        &self,
        _installation: &super::test_issuer::Installation,
        request: &Request<'_>,
        host: HostObservation,
        resources: Resources,
    ) -> Result<(Admission, Option<store::snapshot::Active>)> {
        ensure!(
            matches!(request.operation, Operation::Start),
            "invalid TEST start"
        );
        validate_principal(request.principal)?;
        let gate = self
            .gate
            .try_lock()
            .map_err(|_| anyhow::anyhow!("VM gate unavailable"))?;
        self.store.try_native_transaction(|db| {
            let clock = self.clock.now();
            let active = store::snapshot::active(db, request.vm, false)?;
            let mut decision = self.admit_inner(db, request, host, resources, clock, &gate)?;
            if let Admission::Admitted(worker) = &mut decision {
                let (slots, pins) = active
                    .ok_or_else(|| anyhow::anyhow!("TEST start has no released pair"))?
                    .pair;
                let scope = Self::bound_native_workspace(db, slots.disk.generation)?;
                ensure!(
                    store::native::pair_matches(db, scope, [slots.disk, slots.vars], pins)?,
                    "TEST start has no bound released pair"
                );
                // Record the exact never-created writer epoch, so a restart
                // before the creation checkpoint still reaches recovery.
                let epoch = worker
                    .admitted()
                    .ok_or_else(|| anyhow::anyhow!("invalid TEST start"))?
                    .operation;
                store::native::recovery::admit_epoch(db, scope, epoch)?;
                worker.deadline = clock.monotonic + Duration::from_secs(360);
                return Ok((decision, active));
            }
            Ok((decision, None))
        })
    }

    /// Admit one guest script for the exact running epoch of a current TEST
    /// run owner. The run check and the durable receipt share one
    /// transaction; a receipt for any other run rolls back as busy. The
    /// worker deadline covers the guest's own timeout plus the channel's
    /// exchange slack, so a typed guest timeout still settles as a result.
    pub(crate) fn admit_native_script(
        &self,
        _installation: &super::test_issuer::Installation,
        request: &Request<'_>,
        target: store::native::release::Target,
        host: HostObservation,
        resources: Resources,
    ) -> Result<Admission> {
        let Operation::Script {
            timeout_seconds, ..
        } = request.operation
        else {
            anyhow::bail!("invalid TEST script");
        };
        ensure!(request.vm == target.vm, "invalid TEST script");
        validate_principal(request.principal)?;
        let gate = self
            .gate
            .try_lock()
            .map_err(|_| anyhow::anyhow!("VM gate unavailable"))?;
        let stale = std::cell::Cell::new(false);
        let result = self.store.try_native_transaction(|db| {
            let clock = self.clock.now();
            let mut decision = self.admit_inner(db, request, host, resources, clock, &gate)?;
            if let Admission::Admitted(worker) = &mut decision {
                if !Self::native_run_matches(db, request.principal, target, true, utc(clock)?)? {
                    stale.set(true);
                    anyhow::bail!("TEST script has no current run");
                }
                worker.deadline = clock.monotonic
                    + Duration::from_secs(timeout_seconds)
                    + super::guest::SCRIPT_EXCHANGE_SLACK;
            }
            Ok(decision)
        });
        if stale.get() {
            return Ok(Admission::Refused(Refusal::Busy));
        }
        result
    }

    /// Whether an admitted worker's receipt is still pending. A fence or
    /// recovery may have settled it first.
    pub(crate) fn native_receipt_pending(&self, worker: &PendingOperation) -> Result<bool> {
        self.check_worker(worker)?;
        self.store.transact(|db| {
            Ok(store::count(
                db,
                "SELECT count(*) FROM activity WHERE id=?1 AND status=0",
                [worker.activity_id],
            )? == 1)
        })
    }

    pub(crate) fn native_workspace_scope(&self, generation: Id) -> Result<store::native::Scope> {
        let db = self.store.try_live_connection()?;
        Self::bound_native_workspace(&db, generation)
    }

    fn bound_native_workspace(db: &Connection, generation: Id) -> Result<store::native::Scope> {
        use store::native::disposal::{GenerationDisposal, GenerationState, generation_state};
        match generation_state(db, generation)? {
            GenerationState::Bound {
                scope,
                disposal: GenerationDisposal::None,
            } => Ok(scope),
            _ => anyhow::bail!("TEST workspace is unbound"),
        }
    }

    pub(crate) fn native_clock(&self) -> ClockReading {
        self.clock.now()
    }

    /// Nonblocking monitor/projection read. An already admitted stop, an
    /// expired lease or replacement of either durable identity closes the run.
    pub(crate) fn native_run_current(
        &self,
        principal: &str,
        target: store::native::release::Target,
        running: bool,
    ) -> Result<bool> {
        let db = self.store.try_live_connection()?;
        Self::native_run_matches(&db, principal, target, running, utc(self.clock.now())?)
    }

    fn native_run_matches(
        db: &Connection,
        principal: &str,
        target: store::native::release::Target,
        running: bool,
        now: i64,
    ) -> Result<bool> {
        Ok(store::count(db,
            "SELECT count(*) FROM vm_instances v
             JOIN vm_generations g ON g.vm_id=v.id AND g.active=1 AND g.static_id IS NULL
             JOIN vm_native_generations n ON n.generation=g.id AND n.state='bound'
             JOIN vm_snapshot_release r ON r.generation=g.id AND r.disposition IS NULL
             JOIN vm_operations o ON o.operation_id=r.epoch AND o.vm=v.id AND o.action=3
             WHERE v.id=?1 AND g.id=?2 AND r.epoch=?3 AND v.principal=?4 AND o.principal=?4
               AND v.expires_ms>?5 AND v.state=?6 AND o.status=?7
               AND NOT EXISTS(SELECT 1 FROM vm_operations s WHERE s.vm=v.id AND s.action=4 AND s.status=0)",
            params![target.vm, target.generation, target.epoch, principal, now,
                if running { 2 } else { 1 }, if running { 1 } else { 0 }],
        )? == 1)
    }

    pub(crate) fn finish_native_start(
        &self,
        worker: &PendingOperation,
        principal: &str,
        target: store::native::release::Target,
        cancelled: impl Fn() -> bool,
    ) -> Result<bool> {
        self.check_worker(worker)?;
        self.store.try_native_transaction(|db| {
            let clock = self.clock.now();
            let now = utc(clock)?;
            if worker
                .admitted()
                .is_none_or(|admitted| admitted.operation != target.epoch)
                || !Self::native_run_matches(db, principal, target, false, now)?
                || store::count(
                    db,
                    "SELECT count(*) FROM vm_native_writer
                    WHERE generation=?1 AND epoch=?2 AND boot IS NOT NULL",
                    params![target.generation, target.epoch],
                )? != 1
                || clock.monotonic >= worker.deadline
                || cancelled()
            {
                return Ok(false);
            }
            let status =
                store::settle_operation(db, worker.activity_id, now, Completion::Succeeded)?;
            let clock = self.clock.now();
            ensure!(
                clock.monotonic < worker.deadline
                    && Self::native_run_matches(db, principal, target, true, utc(clock)?)?
                    && !cancelled(),
                "TEST start lost custody during commit"
            );
            Ok(status == Status::Succeeded)
        })
    }

    /// Only an extant run owner can supply the exact target. The stop receipt
    /// and interruption of an in-progress start are atomic. The ledger admits
    /// one pending operation per VM, so a pending script on the run settles
    /// as failed with `script` in the same transaction, while the owner
    /// cancels its exchange.
    pub(crate) fn admit_owned_native_stop(
        &self,
        principal: &str,
        key: RequestId,
        target: store::native::release::Target,
        script: FailureReason,
    ) -> Result<Result<(Id, Option<store::native::release::StopReceipt>), Refusal>> {
        validate_principal(principal)?;
        self.store.try_native_transaction(|db| {
            let clock = self.clock.now();
            let now = utc(clock)?;
            let binding = store::Binding {
                principal,
                key,
                subject: Subject::Vm(target.vm),
                action: Action::Stop as i64,
            };
            match binding.replay(db, now, false)? {
                Ok(Some(replay)) => return Ok(Ok((replay.operation.id, None))),
                Err(reason) => return Ok(Err(reason)),
                Ok(None) => {}
            }
            let start = db
                .query_row(
                    "SELECT o.activity_id,o.status FROM vm_instances v
                 JOIN vm_generations g ON g.vm_id=v.id AND g.active=1 AND g.static_id IS NULL
                 JOIN vm_native_generations n ON n.generation=g.id AND n.state='bound'
                 JOIN vm_snapshot_release r ON r.generation=g.id AND r.disposition IS NULL
                 JOIN vm_operations o ON o.vm=v.id AND o.operation_id=r.epoch AND o.action=3
                 WHERE v.id=?1 AND g.id=?2 AND r.epoch=?3 AND v.principal=?4 AND o.principal=?4
                   AND v.state IN (1,2,3) AND NOT EXISTS(SELECT 1 FROM vm_operations s
                       WHERE s.vm=v.id AND s.action=4 AND s.status=0)",
                    params![target.vm, target.generation, target.epoch, principal],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
                )
                .optional()?;
            let Some((activity, status)) = start else {
                return Ok(Err(Refusal::Busy));
            };
            if store::count(db, "SELECT count(*) FROM vm_operations WHERE status=0", [])?
                >= store::MAX_CALLER_PENDING
                && status != Status::Pending.code()
            {
                return Ok(Err(Refusal::PendingFull));
            }
            if status == Status::Pending.code() {
                store::finish(db, activity, Status::Interrupted, now, Some(false))?;
            }
            let pending_script = db
                .query_row(
                    "SELECT id FROM activity WHERE vm=?1 AND action=2 AND status=0",
                    [target.vm],
                    |row| row.get::<_, i64>(0),
                )
                .optional()?;
            if let Some(pending_script) = pending_script {
                store::settle_with_script(
                    db,
                    pending_script,
                    now,
                    Completion::Failed,
                    None,
                    Some(script),
                )?;
            }
            let operation = binding.record(db, now, Some(target.vm), None)?;
            let stamp = store::native::release::StopReceipt::at_admission(
                db,
                target.vm,
                operation.receipt.activity_id,
                clock.monotonic + Duration::from_secs(120),
            )?;
            store::require_headroom(db)?;
            Ok(Ok((operation.id, Some(stamp))))
        })
    }

    /// Fence the exact unresolved writer even if no operation receipt is
    /// pending. An already owned expiry/recovery claim keeps its identity.
    pub(crate) fn fence_native_run(&self, target: store::native::release::Target) -> Result<()> {
        self.store.try_native_transaction(|db| {
            let state = db
                .query_row(
                    "SELECT v.state FROM vm_instances v JOIN vm_generations g
                 ON g.vm_id=v.id AND g.active=1 AND g.static_id IS NULL
                 JOIN vm_snapshot_release r ON r.generation=g.id AND r.disposition IS NULL
                 WHERE v.id=?1 AND g.id=?2 AND r.epoch=?3",
                    params![target.vm, target.generation, target.epoch],
                    |row| row.get::<_, i64>(0),
                )
                .optional()?;
            let Some(state) = state else { return Ok(()) };
            let now = utc(self.clock.now())?;
            if matches!(state, 1 | 2) {
                let id = store::activity(db, "arxd", target.vm, Action::Recover, now)?;
                db.execute(
                    "UPDATE vm_instances SET state=3,cleanup_id=?2 WHERE id=?1",
                    params![target.vm, id],
                )?;
            }
            store::interrupt_pending(db, target.vm, now, false)
        })
    }

    /// The durable admission one native TEST effect requires, read without
    /// waiting on the store. The worker's admitted claim, fixed by the
    /// controller at admission, names the principal, request, action and
    /// operation; the row must still be pending, before the worker deadline.
    /// The non-static target generation must belong to that operation: a
    /// pending build, a creating clone, or an active generation whose
    /// unresolved writer epoch is the named one. Every named slot must be
    /// live in that generation.
    pub(crate) fn native_effect_admitted(
        &self,
        worker: &PendingOperation,
        target: super::test_issuer::NativeTarget,
        slots: &[ArtifactSlot],
    ) -> Result<bool> {
        self.check_worker(worker)?;
        let Some(admitted) = worker.admitted() else {
            return Ok(false);
        };
        let mut db = self.store.try_live_connection()?;
        let tx = db.transaction()?;
        let reading = self.clock.now();
        if reading.monotonic >= worker.deadline || target.generation == [0; 16] {
            return Ok(false);
        }
        let owned = store::count(
            &tx,
            "SELECT count(*) FROM vm_operations o JOIN vm_generations g ON g.id=?6
             WHERE o.activity_id=?1 AND o.operation_id=?2 AND o.principal=?3
               AND o.request_key=?4 AND o.action=?5 AND o.status=0 AND g.static_id IS NULL
               AND CASE WHEN ?8 IS NULL THEN
                   (g.active=0 AND g.build_id IS NOT NULL AND EXISTS(SELECT 1 FROM vm_build_intents b
                       WHERE b.id=g.build_id AND b.operation_id=o.operation_id AND b.state='pending'))
                   OR (g.active=0 AND g.vm_id=o.vm AND o.action=1 AND EXISTS(SELECT 1 FROM vm_instances i
                       WHERE i.id=o.vm AND i.principal=o.principal AND i.state=0 AND i.expires_ms>?7))
               ELSE g.active=1 AND g.vm_id=o.vm AND EXISTS(SELECT 1 FROM vm_instances i
                       WHERE i.id=o.vm AND i.principal=o.principal AND i.expires_ms>?7)
                   AND EXISTS(SELECT 1 FROM vm_snapshot_release r
                       WHERE r.generation=g.id AND r.epoch=?8 AND r.disposition IS NULL) END",
            params![
                worker.activity_id,
                admitted.operation,
                admitted.principal,
                admitted.request.key,
                admitted.action,
                target.generation,
                utc(reading)?,
                target.writer_epoch,
            ],
        )? == 1;
        if !owned {
            return Ok(false);
        }
        for slot in slots {
            if slot.generation != target.generation
                || store::count(
                    &tx,
                    "SELECT count(*) FROM vm_artifacts WHERE generation_id=?1 AND role=?2
                       AND object_key=?3 AND state IN ('planned','created','identified','active')",
                    params![slot.generation, slot.role, slot.key],
                )? != 1
            {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Recovery needs exact durable TEST custody, not a fresh selection: the
    /// bound native workspace scope of a non-static generation. A generation
    /// with writer history needs its exact writer epoch: an unresolved
    /// release epoch, or for staged work the generation's own epoch once the
    /// checked creation edge recorded its boot and file pins. Only positive
    /// proof of an untouched reservation (the v17 reservation row with no
    /// recorded origin, and no release row) recovers without an epoch; a
    /// missing reservation row proves nothing.
    pub(crate) fn native_recovery_bound(
        &self,
        scope: store::native::Scope,
        writer_epoch: Option<Id>,
    ) -> Result<bool> {
        self.store.transact(|db| {
            Ok(store::native::bound(db, scope)?
                && store::count(
                    db,
                    "SELECT count(*) FROM vm_generations g
                     LEFT JOIN vm_native_writer w ON w.generation=g.id
                     WHERE g.id=?1 AND g.static_id IS NULL AND CASE WHEN ?2 IS NULL THEN
                         g.active=0 AND NOT EXISTS(SELECT 1 FROM vm_snapshot_release r
                             WHERE r.generation=g.id)
                         AND w.epoch=g.id AND w.boot IS NULL AND w.database_file IS NULL
                         AND w.lock_file IS NULL
                     ELSE EXISTS(SELECT 1 FROM vm_snapshot_release r
                         WHERE r.generation=g.id AND r.epoch=?2 AND r.disposition IS NULL)
                         OR (g.active=0 AND ?2=g.id AND NOT EXISTS(SELECT 1
                             FROM vm_snapshot_release r WHERE r.generation=g.id)
                             AND w.epoch=?2 AND w.protocol=1 AND w.boot IS NOT NULL
                             AND w.database_file IS NOT NULL AND w.lock_file IS NOT NULL) END",
                    params![scope.generation, writer_epoch],
                )? == 1)
        })
    }
}

#[cfg(test)]
impl<C: Clock> VmController<C> {
    /// Direct store access for pressure and ledger witnesses.
    pub(crate) fn store_for_test(&self) -> &VmStore {
        &self.store
    }
}

impl<C: Clock> VmController<C> {
    pub(crate) fn foreground_snapshot_list(
        &self,
        principal: &str,
        vm: Id,
        after: Option<Id>,
        limit: u32,
    ) -> Result<Result<Vec<store::snapshot::Point>, Refusal>> {
        self.store
            .transact(|db| store::snapshot::list(db, principal, vm, after, limit))
    }

    pub(crate) fn foreground_snapshot_precheck(
        &self,
        principal: &str,
        vm: Id,
        request: &super::snapshot::Request,
    ) -> Result<Result<bool, Refusal>> {
        self.store.transact(|db| {
            let result =
                store::snapshot::precheck(db, principal, vm, request, utc(self.clock.now())?)?;
            // A Compose backup ID is outside the snapshot namespace. The
            // caller's own VM gets the explicit typed refusal; everyone else
            // keeps the uncorrelated not_found.
            if matches!(result, Err(Refusal::NotFound))
                && let super::snapshot::Request::Delete(id) = request
                && store::compose::owned_backup(db, principal, vm, *id)?
            {
                return Ok(Err(Refusal::StaticBackupDeleteForbidden));
            }
            Ok(result)
        })
    }
}

#[cfg(test)]
impl<C: Clock> VmController<C> {
    pub(crate) fn finish_fixture_stop(
        &self,
        worker: &PendingOperation,
        vm: Id,
        epoch: Id,
        pair: Option<store::snapshot::Pair>,
        disposition: super::runtime::StopDisposition,
    ) -> Result<Status> {
        use super::runtime::StopDisposition;
        self.check_worker(worker)?;
        self.store.transact(|db| {
            let now = self.clock.now();
            let before_deadline = now.monotonic < worker.deadline;
            ensure!(store::count(db,
                "SELECT count(*) FROM vm_operations WHERE activity_id=?1 AND vm=?2 AND action=4 AND status=0",
                params![worker.activity_id, vm],
            )? == 1, "invalid offline release worker");
            match disposition {
                StopDisposition::CleanRelease => {
                    let completion = if before_deadline {
                        Completion::Succeeded
                    } else {
                        Completion::CleanupUnconfirmed
                    };
                    let status =
                        store::settle_operation(db, worker.activity_id, utc(now)?, completion)?;
                    if status == Status::Succeeded
                        && let Some(pair) = pair
                    {
                        store::snapshot::prove_stop(db, vm, epoch, pair, disposition)?;
                    }
                    Ok(status)
                }
                StopDisposition::DirtyRelease => {
                    // Safely released after a failed graceful stop: settle the
                    // stop operation as unconfirmed/dirty recovery, and persist
                    // the typed dirty release disposition independently of
                    // generic cleanup confirmation.
                    let is_static = store::count(
                        db,
                        "SELECT EXISTS(SELECT 1 FROM vm_static_instances WHERE id=?1)",
                        [vm],
                    )? != 0;
                    let completion = if is_static && before_deadline {
                        Completion::Failed
                    } else {
                        Completion::CleanupUnconfirmed
                    };
                    let status = store::settle_with_script(
                        db,
                        worker.activity_id,
                        utc(now)?,
                        completion,
                        None,
                        Some(FailureReason::DirtyStop),
                    )?;
                    if let Some(pair) = pair {
                        store::snapshot::prove_stop(db, vm, epoch, pair, disposition)?;
                    }
                    Ok(status)
                }
                StopDisposition::ChargedQuarantine => store::settle_with_script(
                    db,
                    worker.activity_id,
                    utc(now)?,
                    Completion::CleanupUnconfirmed,
                    None,
                    Some(FailureReason::DirtyStop),
                ),
            }
        })
    }

    pub(crate) fn active_release_epoch(&self, vm: Id, is_static: bool) -> Result<Option<(Id, Id)>> {
        self.store
            .transact(|db| store::snapshot::active_release_epoch(db, vm, is_static))
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn admit_snapshot(
        &self,
        principal: &str,
        key: RequestId,
        vm: Id,
        action: i64,
        request: &super::snapshot::Request,
        route: Option<&super::runtime::publication::SnapshotRoute>,
        host: HostObservation,
        resources: Resources,
    ) -> Result<Result<(store::snapshot::Work, PendingOperation, OperationRef), Refusal>> {
        use super::snapshot::Request;
        use store::snapshot;
        let gate = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("VM gate unavailable"))?;
        map_full(self.store.transact(|db| {
            let clock = self.clock.now();
            let now = utc(clock)?;
            let binding = store::Binding {
                principal,
                key,
                subject: request.binding(vm),
                action,
            };
            match binding.replay(db, now, gate.closed)? {
                Ok(None) => {}
                Ok(Some(_)) => return Ok(Err(Refusal::Busy)),
                Err(reason) => return Ok(Err(reason)),
            }
            let is_static = match snapshot::precheck(db, principal, vm, request, now)? {
                Ok(is_static) => is_static,
                Err(reason) => return Ok(Err(reason)),
            };
            if is_static && route.is_none_or(|route| route.vm() != vm) {
                return Ok(Err(Refusal::SnapshotPublishNotQuiesced));
            }
            if gate.custody_uncertain || store::has_failed_clone_cleanup(db)? {
                return Ok(Err(Refusal::Shutdown));
            }
            if store::count(db, "SELECT count(*) FROM vm_operations WHERE status=0", [])?
                >= store::MAX_CALLER_PENDING
            {
                return Ok(Err(Refusal::PendingFull));
            }
            let Some(active) = snapshot::active(db, vm, is_static)? else {
                return Ok(Err(Refusal::SnapshotOfflineRequired));
            };
            if !snapshot::check_route(
                db,
                vm,
                is_static,
                active.pair.0.disk.generation,
                active.epoch,
                route,
            )? {
                return Ok(Err(Refusal::SnapshotPublishNotQuiesced));
            }
            let source = match request {
                Request::Create(_) if active.clean => active.pair,
                Request::Create(_) => return Ok(Err(Refusal::SnapshotOfflineRequired)),
                Request::Restore(id) | Request::Delete(id) => {
                    let Some(source) =
                        snapshot::selected(db, vm, *id, is_static, active.lineage, "ready")?
                    else {
                        return Ok(Err(Refusal::NotFound));
                    };
                    source
                }
            };
            let claim = if matches!(request, Request::Delete(_)) {
                DiskClaim::default()
            } else {
                use super::static_web::FullCopyReservation;
                let Some(claim) = (FullCopyReservation {
                    qcow2_bytes: source.1[0].pin.size,
                    qcow2_growth_bytes: if matches!(request, Request::Restore(_)) {
                        1 << 20
                    } else {
                        0
                    },
                    vars_bytes: source.1[1].pin.size,
                    metadata_bytes: 4096,
                    staging_bytes: 65536,
                })
                .checked_claim() else {
                    return Ok(Err(Refusal::Disk));
                };
                let capacity = match self.policy.host(host, resources) {
                    Ok(capacity) => capacity,
                    Err(reason) => return Ok(Err(reason)),
                };
                if let Err(reason) = self.validate_disk(db, capacity, claim)? {
                    return Ok(Err(reason));
                }
                claim
            };
            let worker = self.worker(clock, 15 * 60)?;
            let (work, reference) = snapshot::reserve(
                db, vm, is_static, binding, request, active, source, claim, now,
            )?;
            Ok(Ok((
                work,
                PendingOperation {
                    activity_id: reference.receipt.activity_id,
                    ..worker
                },
                reference,
            )))
        }))
    }

    fn check_snapshot_worker(
        &self,
        worker: &PendingOperation,
        work: &store::snapshot::Work,
    ) -> Result<()> {
        self.check_worker(worker)?;
        ensure!(
            worker.activity_id == work.activity && self.clock.now().monotonic < worker.deadline,
            "snapshot worker token expired"
        );
        Ok(())
    }

    pub(crate) fn snapshot_checkpoint(
        &self,
        worker: &PendingOperation,
        work: &store::snapshot::Work,
        host: HostObservation,
        resources: Resources,
    ) -> Result<()> {
        self.check_snapshot_worker(worker, work)?;
        self.store.transact(|db| {
            ensure!(
                store::snapshot::can_copy(db, work, utc(self.clock.now())?)?,
                "snapshot lease closed"
            );
            let capacity = self
                .policy
                .host(host, resources)
                .map_err(|_| anyhow::anyhow!("snapshot owner reserve crossed"))?;
            ensure!(
                self.validate_disk(db, capacity, DiskClaim::default())?
                    .is_ok(),
                "snapshot disk reserve crossed"
            );
            Ok(())
        })
    }

    pub(crate) fn snapshot_leaf_begin(
        &self,
        worker: &PendingOperation,
        work: &store::snapshot::Work,
        slot: ArtifactSlot,
    ) -> Result<()> {
        self.check_snapshot_worker(worker, work)?;
        self.store
            .transact(|db| store::snapshot::begin_leaf_create(db, work, slot))
    }

    pub(crate) fn snapshot_leaf_identity(
        &self,
        worker: &PendingOperation,
        work: &store::snapshot::Work,
        slot: ArtifactSlot,
        identity: super::paths::FileIdentity,
        directory: Id,
    ) -> Result<()> {
        self.check_snapshot_worker(worker, work)?;
        self.store
            .transact(|db| store::snapshot::record_identity(db, work, slot, identity, directory))
    }

    pub(crate) fn snapshot_leaf_complete(
        &self,
        worker: &PendingOperation,
        work: &store::snapshot::Work,
        slot: ArtifactSlot,
        evidence: ArtifactEvidence,
    ) -> Result<()> {
        self.check_snapshot_worker(worker, work)?;
        self.store
            .transact(|db| store::snapshot::identify_leaf(db, work, slot, evidence))
    }

    pub(crate) fn finish_snapshot_copy(
        &self,
        worker: &PendingOperation,
        work: &store::snapshot::Work,
        pair: [ArtifactEvidence; 2],
        route: Option<&super::runtime::publication::SnapshotRoute>,
        host: HostObservation,
        resources: Resources,
    ) -> Result<()> {
        self.check_snapshot_worker(worker, work)?;
        self.store.transact(|db| {
            let capacity = self
                .policy
                .host(host, resources)
                .map_err(|_| anyhow::anyhow!("snapshot owner reserve crossed"))?;
            ensure!(
                self.validate_disk(db, capacity, DiskClaim::default())?
                    .is_ok(),
                "snapshot disk reserve crossed"
            );
            store::snapshot::commit(db, work, pair, utc(self.clock.now())?, route)?;
            // The receipt, both leaves and any pointer/charge swap are still
            // tentative. A deadline crossed during writer acquisition or SQL
            // settlement rolls all of them back before transaction commit.
            self.check_snapshot_worker(worker, work)
        })
    }

    pub(crate) fn fail_snapshot_copy(
        &self,
        worker: &PendingOperation,
        work: &store::snapshot::Work,
        leaves: [store::snapshot::StagedLeaf; 2],
    ) -> Result<()> {
        self.check_worker(worker)?;
        ensure!(
            worker.activity_id == work.activity,
            "snapshot worker mismatch"
        );
        self.store
            .transact(|db| store::snapshot::fail_copy(db, work, leaves, utc(self.clock.now())?))
    }
}
