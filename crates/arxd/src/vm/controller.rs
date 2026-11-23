use super::{model::*, policy::*};
use crate::store::{
    validate_principal,
    vm::{self as store, VmStore},
};
use anyhow::{Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};

/// Foreground VM-profile metadata. A successful admission commits intent;
/// there is no runtime, transport, or process launcher in this controller.
pub struct VmController<C> {
    store: VmStore,
    policy: VmPolicy,
    clock: C,
    closed: Mutex<bool>,
    worker_binding: Arc<()>,
}

impl<C: Clock> VmController<C> {
    pub fn open(vm_state_root: &Path, policy: VmPolicy, clock: C) -> Result<Self> {
        let store = VmStore::open(vm_state_root, utc(clock.now())?)?;
        Ok(Self {
            store,
            policy,
            clock,
            closed: Mutex::new(false),
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
            .closed
            .lock()
            .map_err(|_| anyhow::anyhow!("VM gate unavailable"))?;
        let result = self.store.transact(|db| {
            let clock = self.clock.now();
            let now = utc(clock)?;
            let action = request.operation.action();

            let decision = self.admit_inner(db, &request, host, resources, clock, *closed)?;
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
        closed: bool,
    ) -> Result<Admission> {
        let now = clock.utc_ms;
        let action = request.operation.action();
        let binding = store::Binding {
            principal: request.principal,
            key: request.request_id,
            subject: Subject::Vm(request.vm),
            action: action as i64,
        };
        match binding.replay(db, now, closed)? {
            Ok(Some(prior)) => return Ok(Admission::Existing(prior.operation.receipt)),
            Err(reason) => return Ok(Admission::Refused(reason)),
            Ok(None) => {}
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
            .closed
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
                *closed,
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
            .closed
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
            match binding.replay(db, now, *closed)? {
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
                proof.filter(|_| clock.monotonic < worker.deadline),
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
            .closed
            .lock()
            .map_err(|_| anyhow::anyhow!("VM gate unavailable"))?;
        if *closed {
            return Ok(Err(Refusal::Shutdown));
        }
        let result = self.store.transact(|db| {
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
            .closed
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
            match binding.replay(db, now, *closed)? {
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
            let row = owned_vm(db, principal, vm)?;
            let decision = match (row, requested) {
                (_, Err(reason)) => Err(reason),
                (None, _) if store::expired_status(db, principal, vm)?.is_some() => {
                    Err(Refusal::Expired)
                }
                (None, _) => Err(Refusal::NotFound),
                (Some((state, expires)), _) if state == 4 || expires <= now => {
                    Err(Refusal::Expired)
                }
                (Some((_, expires)), Ok(end)) if end < expires => Err(Refusal::Invalid),
                (Some(_), Ok(end)) => {
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

    /// A caller may settle a teardown only after proving owned workers are
    /// stopped and every attributable artifact was removed.
    pub fn finish_expiry(&self, claim: ExpiryClaim, result: CleanupResult) -> Result<()> {
        self.store.transact(|db| {
            let now = utc(self.clock.now())?;
            let current: Option<i64> = db
                .query_row(
                    "SELECT cleanup_id FROM vm_instances
                WHERE id=?1 AND state=4 AND cleanup_id IS NOT NULL",
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
            let pending: Option<i64> = db
                .query_row(
                    "SELECT id FROM activity WHERE vm=?1 AND status=0 AND action<=6",
                    [claim.vm],
                    |row| row.get(0),
                )
                .optional()?;
            if let Some(id) = pending {
                store::finish(db, id, Status::Interrupted, now, Some(true))?;
            }
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
            ensure!(
                db.execute(
                    "UPDATE vm_instances SET state=1,cleanup_id=NULL
                     WHERE id=?1 AND state=3 AND cleanup_id=?2",
                    params![vm, claim.activity_id]
                )? == 1,
                "invalid recovery intent"
            );
            db.execute("DELETE FROM vm_snapshots WHERE vm=?1 AND ready=0", [vm])?;
            db.execute(
                "DELETE FROM vm_disk_claims WHERE vm=?1 AND persistent=0",
                [vm],
            )?;
            db.execute("UPDATE vm_disk_claims SET staging=0 WHERE vm=?1", [vm])?;
            store::finish(db, claim.activity_id, Status::Succeeded, now, Some(true))
        })
    }

    /// Close admission and reconcile pending work in the same lock order used
    /// by admission. Cleanup intents survive process shutdown.
    pub fn shutdown(&self) -> Result<()> {
        let mut closed = self
            .closed
            .lock()
            .map_err(|_| anyhow::anyhow!("VM gate unavailable"))?;
        *closed = true;
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

fn owned_vm(db: &Connection, principal: &str, vm: Id) -> Result<Option<(i64, i64)>> {
    Ok(db
        .query_row(
            "SELECT state,expires_ms FROM vm_instances WHERE id=?1 AND principal=?2",
            params![vm, principal],
            |row| Ok((row.get(0)?, row.get(1)?)),
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
    let Some((state, expires)) = owned_vm(db, request.principal, request.vm)? else {
        return Ok(Err(Refusal::NotFound));
    };
    if state == 4 || expires <= now {
        return Ok(Err(Refusal::Expired));
    }
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
