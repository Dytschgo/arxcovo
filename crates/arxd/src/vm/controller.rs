use super::{
    model::*,
    policy::*,
    reaper::{self, ArtifactCustody, ClosedCustody, ReaperTick, Sweeps},
    runtime::{CandidateRuntime, ReaperRuntime},
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
    sync::{Arc, Mutex, TryLockError},
    time::Duration,
};

/// Foreground VM-profile metadata. A successful admission commits intent;
/// there is no runtime, transport, or process launcher in this controller.
pub struct VmController<C> {
    store: VmStore,
    policy: VmPolicy,
    clock: C,
    gate: Mutex<AdmissionGate>,
    sweeps: Mutex<Sweeps>,
    worker_binding: Arc<()>,
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
    pub fn open(vm_state_root: &Path, policy: VmPolicy, clock: C) -> Result<Self> {
        let store = VmStore::open(vm_state_root, utc(clock.now())?)?;
        Ok(Self {
            store,
            policy,
            clock,
            gate: Mutex::default(),
            sweeps: Mutex::default(),
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
        let capacity = match self.policy.host(host, resources) {
            Ok(capacity) => capacity,
            Err(reason) => return Ok(Admission::Refused(reason)),
        };
        if let Err(reason) = self.validate_disk(db, capacity, request.disk)? {
            return Ok(Admission::Refused(reason));
        }
        let target = match target(db, request, now)? {
            Ok(target) => target,
            Err(reason) => return Ok(Admission::Refused(reason)),
        };
        let id = binding
            .record(db, now, Some(request.vm), None)?
            .receipt
            .activity_id;
        target.insert(db, request, id)?;
        store::require_headroom(db)?;
        Ok(Admission::Admitted(PendingOperation {
            activity_id: id,
            deadline,
            binding: Arc::clone(&self.worker_binding),
        }))
    }

    pub fn finish(
        &self,
        operation: &PendingOperation,
        mut completion: Completion,
    ) -> Result<Status> {
        ensure!(
            Arc::ptr_eq(&self.worker_binding, &operation.binding),
            "invalid VM worker handle"
        );
        self.store.transact(|db| {
            ensure!(store::count(db,
                "SELECT count(*) FROM vm_operations WHERE activity_id=?1 AND binding_kind IN ('recipe','template')",
                [operation.activity_id],
            )? == 0, "durable VM worker requires artifact settlement");
            let now = self.clock.now();
            if now.monotonic >= operation.deadline {
                completion = Completion::CleanupUnconfirmed;
            }
            store::settle_operation(db, operation.activity_id, utc(now)?, completion)
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
            store::settle_with_script(db, worker.activity_id, utc(clock)?, completion, script)
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
            Ok(store::expired_status(db, principal, id)?
                .map(|row| VmStatus {
                    id,
                    template: None,
                    guest: None,
                    state: "expired",
                    ready: false,
                    expires_at_ms: None,
                    expired_at_ms: Some(row.expired_at_ms),
                    cleanup: Some("confirmed"),
                    snapshots: [],
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
                subject: Subject::Recipe(request.recipe, request.digest),
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
        // 60 minutes for install plus bounded detached verification and release.
        let pending = self.worker(clock, 3_900)?;
        let operation = binding.record(db, now, None, None)?;
        let slots = store::reserve_build(db, request, operation, peak)?;
        store::require_headroom(db)?;
        Ok(Ok(BuildAdmission::Fresh(
            Box::new(slots),
            PendingOperation {
                activity_id: operation.receipt.activity_id,
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
                let operation = binding.record(db, now, Some(request.vm), None)?;
                target.insert(db, &request, operation.receipt.activity_id)?;
                let slots = store::plan_clone(db, request.vm)?;
                store::require_headroom(db)?;
                Ok(Ok(CloneAdmission::Fresh(request.vm, slots, PendingOperation {
                    activity_id: operation.receipt.activity_id, ..pending
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
            store::publish_build(db, worker.activity_id, slots, request, proof, now)?;
            Ok(Status::Succeeded)
        })
    }

    pub(crate) fn fail_build(
        &self,
        worker: &PendingOperation,
        slots: BuildSlots,
        untouched: bool,
    ) -> Result<()> {
        self.check_worker(worker)?;
        self.store.transact(|db| {
            let now = utc(self.clock.now())?;
            if untouched {
                store::cancel_untouched_build(db, slots, worker.activity_id, now)
            } else {
                store::abandon_build(db, slots.id, worker.activity_id, now)
            }
        })
    }

    pub(crate) fn finish_linked_clone(
        &self,
        worker: &PendingOperation,
        slots: PairSlots,
        proof: Option<[ArtifactEvidence; 2]>,
    ) -> Result<Status> {
        self.check_worker(worker)?;
        self.store.transact(|db| {
            let clock = self.clock.now();
            store::settle_clone(
                db,
                worker.activity_id,
                slots,
                proof,
                clock.monotonic < worker.deadline,
                utc(clock)?,
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
    /// foreground exposes tools. Returns false if another sweep is in flight.
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
                    self.sweep_with(runtime, custody)?;
                }
            }
        }
    }

    pub(crate) fn sweep_with(
        &self,
        runtime: &impl ReaperRuntime,
        custody: &impl ArtifactCustody,
    ) -> Result<bool> {
        let mut sweeps = match self.sweeps.try_lock() {
            Ok(sweeps) => sweeps,
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
                let items = store::cleanup_kinds(db)?
                    .into_iter()
                    .map(|kind| store::cleanup_work(db, kind))
                    .collect::<Result<Vec<_>>>()?
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>();
                Ok((items, store::legacy_cleanup_uncertain(db)?))
            })?
        };
        sweeps.prepare(&mut items);
        for work in items {
            if !sweeps.due(work.kind, self.clock.now().monotonic) {
                uncertain = true;
                continue;
            }
            let proved = reaper::reconcile(&work, runtime, custody, || self.clock.now().monotonic);
            let confirmed = self
                .store
                .transact(|db| store::settle_cleanup(db, &work, utc(self.clock.now())?, proved))?;
            uncertain |= !confirmed;
            sweeps.settle(&work, self.clock.now().monotonic, confirmed);
        }
        self.gate
            .lock()
            .map_err(|_| anyhow::anyhow!("VM gate unavailable"))?
            .custody_uncertain = uncertain;
        Ok(true)
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
    Clone { template: Id, expires: i64 },
    Snapshot(Id),
}

impl Target {
    fn insert(self, db: &Connection, request: &Request<'_>, id: i64) -> Result<()> {
        let snapshot = match self {
            Self::Existing => None,
            Self::Clone { template, expires } => {
                db.execute("INSERT INTO vm_instances(id,template,principal,expires_ms) VALUES(?1,?2,?3,?4)",
                    params![request.vm, template, request.principal, expires])?;
                None
            }
            Self::Snapshot(snapshot) => {
                db.execute(
                    "INSERT INTO vm_snapshots(vm,id) VALUES(?1,?2)",
                    params![request.vm, snapshot],
                )?;
                Some(snapshot)
            }
        };
        if request.disk.total().is_some_and(|n| n > 0) {
            db.execute(
                "INSERT INTO vm_disk_claims VALUES(?1,?2,?3,?4,?5)",
                params![
                    id,
                    request.vm,
                    snapshot,
                    request.disk.persistent(),
                    request.disk.staging
                ],
            )?;
        }
        Ok(())
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
        {
            return Ok(Err(Refusal::NotFound));
        }
        if store::count(db, "SELECT count(*) FROM vm_instances", [])? >= store::MAX_INSTANCES {
            return Ok(Err(Refusal::InstanceLimit));
        }
        store::purge_tombstones(db, now)?;
        if store::count(
            db,
            "SELECT EXISTS(SELECT 1 FROM vm_instances WHERE id=?1)
            OR EXISTS(SELECT 1 FROM vm_tombstones WHERE id=?1)",
            [request.vm],
        )? != 0
        {
            return Ok(Err(Refusal::Busy));
        }
        return Ok(Ok(Target::Clone { template, expires }));
    }
    let Some(lease) = owned_vm(db, request.principal, request.vm)? else {
        return Ok(Err(Refusal::NotFound));
    };
    if let Err(reason) = lease.check(now) {
        return Ok(Err(reason));
    }
    let state = lease.state;
    if !matches!(state, 1 | 2)
        || store::count(
            db,
            "SELECT EXISTS(SELECT 1 FROM activity
        WHERE vm=?1 AND status=0 AND action<=6)",
            [request.vm],
        )? != 0
    {
        return Ok(Err(Refusal::Busy));
    }
    if !matches!(request.operation, Operation::Snapshot { .. }) && request.disk.images != 0 {
        return Ok(Err(Refusal::Invalid));
    }
    if !matches!(request.operation, Operation::Snapshot { .. }) && request.disk.metadata != 0 {
        return Ok(Err(Refusal::Invalid));
    }
    match request.operation {
        Operation::Snapshot { snapshot } if state == 1 && request.disk.images > 0 => {
            if store::count(
                db,
                "SELECT count(*) FROM vm_snapshots WHERE vm=?1",
                [request.vm],
            )? >= 4
                || store::count(db, "SELECT count(*) FROM vm_snapshots", [])? >= 16
            {
                return Ok(Err(Refusal::SnapshotLimit));
            }
            if store::count(
                db,
                "SELECT EXISTS(SELECT 1 FROM vm_snapshots WHERE vm=?1 AND id=?2)",
                params![request.vm, snapshot],
            )? != 0
            {
                return Ok(Err(Refusal::Busy));
            }
            Ok(Ok(Target::Snapshot(snapshot)))
        }
        Operation::Restore { snapshot } if state == 1 && request.disk.staging > 0 => {
            if store::count(
                db,
                "SELECT EXISTS(SELECT 1 FROM vm_snapshots WHERE vm=?1 AND id=?2 AND ready=1)",
                params![request.vm, snapshot],
            )? == 0
            {
                return Ok(Err(Refusal::NotFound));
            }
            Ok(Ok(Target::Existing))
        }
        Operation::Start if state == 1 => {
            if store::count(
                db,
                "SELECT EXISTS(SELECT 1 FROM vm_instances WHERE state IN (2,3,4))
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
}

impl<C: Clock> VmController<C> {
    pub(crate) fn foreground_catalog(
        &self,
        principal: &str,
        recipe: Option<Id>,
        after: (u8, Id),
        limit: u32,
    ) -> Result<Vec<CatalogEntry>> {
        self.store.transact(|db| {
            let mut query = db.prepare(
                "SELECT kind,id,recipe FROM (
                   SELECT 1 AS kind, ?1 AS id, NULL AS recipe WHERE ?1 IS NOT NULL
                   UNION ALL
                   SELECT 2, b.template_id, b.recipe_id FROM vm_build_intents b
                     WHERE b.state='sealed'
                   UNION ALL
                   SELECT 3, b.operation_id, b.recipe_id FROM vm_build_intents b
                     JOIN vm_operations o USING(operation_id)
                     WHERE o.principal=?2 AND b.state='pending' AND o.status=0
                 ) WHERE (kind,id)>(?3,?4) ORDER BY kind,id LIMIT ?5",
            )?;
            let rows =
                query.query_map(params![recipe, principal, after.0, after.1, limit], |row| {
                    Ok(CatalogEntry {
                        kind: row.get(0)?,
                        id: row.get(1)?,
                        recipe: row.get(2)?,
                    })
                })?;
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
                "SELECT a.id,a.action,a.status,a.reason,a.vm,o.operation_id,
                        a.started_ms,a.finished_ms,a.cleanup_confirmed
                   FROM activity a LEFT JOIN vm_operations o ON o.activity_id=a.id
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
                })
            })?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Check a validated replay before any native decision or fixture lookup.
    /// All callers serialize fresh admission, cancellation and this check in
    /// the foreground supervisor gate. The eventual admission checks it again.
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

    /// Use the existing bounded v6 audit vocabulary and only correlate an ID
    /// after the same transaction proves caller ownership.
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
            let vm = if let Some(vm) = vm {
                (owned_vm(db, principal, vm)?.is_some()
                    || store::expired_status(db, principal, vm)?.is_some())
                .then_some(vm)
            } else {
                None
            };
            if action == BUILD_ACTION || vm.is_some() {
                store::refusal_for(db, principal, vm, action, now, reason)?;
            }
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
