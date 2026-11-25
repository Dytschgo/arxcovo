//! Foreground TEST qualification and operation ownership.

use super::*;
use crate::config::TestQualificationSelection;
use crate::vm::paths::{Artifact, ArtifactRole, FileIdentity};
use crate::vm::qemu_args::{CommandPlan, PlanKind};
use crate::vm::qualification::{
    BootReservation, ForegroundQualification, HeldSlot, InstallerReservation, InstallerSettlement,
    LaunchRequest, LaunchTarget, NativeLaunch, ProductionWorkspace, QualifiedBoot, SlotWriter,
};
use crate::vm::qualification_record::{
    Class, Hash, Instant2, RecipeIdentity, TestRecipes, clocks_agree,
};
use crate::vm::runtime::{
    InstallerBudget, InstallerPhase, InstallerRecipe, InstallerStage, NativeEffectBinding,
    StopDisposition, test_issuer::Intent,
};
use arx_guest::protocol::ScriptStatus;
use std::collections::HashMap;
use tokio::sync::mpsc;

const MISSING: NativeRefusal = NativeRefusal::QualificationMissing;
const PATH: NativeRefusal = NativeRefusal::PathCustodyUnverified;
const GIB: u64 = 1 << 30;
const MEMORY: usize = (6 * GIB) as usize;
const CPU_RATE: u32 = 2500;
const RESOURCES: Resources = Resources {
    vcpus: 2,
    guest_ram: 4 * GIB,
    overhead_ram: 3 * GIB,
};

pub(super) struct SelectedTest {
    qualification: ForegroundQualification,
    seal_sha256: Hash,
    epoch: Epoch,
    invalidated: AtomicBool,
    running: Mutex<HashMap<Id, Running>>,
    removed: Mutex<std::collections::HashSet<RemovedSlot>>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct RemovedSlot {
    generation: Id,
    key: Id,
    directory: Id,
    file: Id,
    volume: u64,
}

#[derive(Clone, Copy)]
struct Witness {
    generation: Id,
    operation: Id,
    id: Id,
}

struct Running {
    queue: mpsc::Sender<(OwnerWork, Work)>,
    control: RunControl,
    epoch: Id,
    stop_worker: Option<PendingOperation>,
    witness: Option<Witness>,
}

#[derive(Clone, Default)]
struct RunControl {
    cancel: Shutdown,
    stop: Shutdown,
    retry: Arc<tokio::sync::Notify>,
}

impl Running {
    fn request_stop(&mut self) {
        if self.witness.is_none() {
            self.control.cancel.cancel();
        }
        self.witness = None;
        self.control.stop.cancel();
        self.control.retry.notify_one();
    }
}

struct OwnerWork {
    principal: String,
    key: RequestId,
    operation: Id,
    action: i64,
    worker: PendingOperation,
}

struct Epoch {
    id: Id,
    boot_marker: Hash,
    start: Instant2,
    monotonic: Instant,
}

impl Epoch {
    fn valid(&self, now: Instant2, boot_marker: Hash, reading: ClockReading) -> bool {
        let elapsed = reading.monotonic.checked_duration_since(self.monotonic);
        boot_marker == self.boot_marker
            && clocks_agree(self.start, now)
            && elapsed.is_some_and(|elapsed| {
                now.mono_ms
                    .checked_sub(self.start.mono_ms)
                    .is_some_and(|boot_elapsed| {
                        elapsed.abs_diff(Duration::from_millis(boot_elapsed)) <= runtime::FRESHNESS
                    })
            })
            && u64::try_from(reading.utc_ms / 1_000).is_ok_and(|utc| utc.abs_diff(now.utc) <= 1)
    }
}

impl SelectedTest {
    pub(super) fn open(
        controller: &Controller,
        config: TestQualificationSelection,
        acquire: impl FnOnce(
            Arc<crate::store::vm::VmStore>,
            &str,
            &str,
            &crate::vm::qualification_record::Selection,
        ) -> Result<ForegroundQualification, NativeRefusal>,
    ) -> Result<Self, NativeRefusal> {
        let qualification = acquire(
            controller.native_store(),
            config.enrollment.as_str(),
            config.dossier.as_str(),
            &config.selection,
        )?;
        let fresh = qualification.fresh()?;
        let reading = controller
            .advance_native_clock()
            .map_err(|_| MISSING)?
            .ok_or(MISSING)?;
        let evidence = qualification.evidence();
        if evidence.seal.binding.class != Class::Test
            || evidence.recipes.ubuntu.recipe_id == WEB_RECIPE_ID
            || evidence.recipes.windows.recipe_id != Controller::windows_recipe_id()
        {
            return Err(MISSING);
        }
        let epoch = Epoch {
            id: crate::store::vm::new_id().map_err(|_| MISSING)?,
            boot_marker: fresh.boot_marker,
            start: fresh.now,
            monotonic: reading.monotonic,
        };
        if !epoch.valid(fresh.now, fresh.boot_marker, reading) {
            return Err(MISSING);
        }
        Ok(Self {
            qualification,
            seal_sha256: config.selection.seal_sha256,
            epoch,
            invalidated: AtomicBool::new(false),
            running: Mutex::default(),
            removed: Mutex::default(),
        })
    }

    fn refresh_epoch(&self, controller: &Controller) -> Result<Instant, NativeRefusal> {
        if self.invalidated.load(Ordering::Acquire) {
            return Err(MISSING);
        }
        let check = || {
            let captured = Instant::now();
            let fresh = self.qualification.fresh()?;
            let reading = controller
                .advance_native_clock()
                .map_err(|_| MISSING)?
                .ok_or(MISSING)?;
            if !self.epoch.valid(fresh.now, fresh.boot_marker, reading) {
                return Err(MISSING);
            }
            runtime::fresh_observation(captured, Instant::now(), MISSING)?;
            Ok(captured)
        };
        check().inspect_err(|_| self.invalidated.store(true, Ordering::Release))
    }

    pub(super) fn recipes(&self) -> TestRecipes {
        self.qualification.evidence().recipes
    }

    pub(super) fn sweep(&self, controller: &Controller) -> anyhow::Result<()> {
        let reaper = self.reaper(controller);
        controller.sweep_with(&reaper, &reaper).map(|_| ())
    }

    pub(super) fn reaper<'a>(&'a self, controller: &'a Controller) -> NativeReaper<'a> {
        NativeReaper {
            state: self,
            controller,
        }
    }

    pub(super) fn capability(&self, controller: &Controller) -> Result<Value, NativeRefusal> {
        self.refresh_epoch(controller)?;
        Ok(catalog(self.seal_sha256, self.recipes()))
    }
}

fn catalog(seal_sha256: Hash, recipes: TestRecipes) -> Value {
    json!({
        "available": true,
        "selected_test_qualification": {
            "sha256": hex::encode(seal_sha256),
            "recipes": [
                {"guest": "ubuntu", "recipe_id": hex::encode(recipes.ubuntu.recipe_id),
                    "recipe_sha256": hex::encode(recipes.ubuntu.recipe_sha256)},
                {"guest": "windows", "recipe_id": hex::encode(recipes.windows.recipe_id),
                    "recipe_sha256": hex::encode(recipes.windows.recipe_sha256)}
            ]
        }
    })
}

impl SelectedTest {
    pub(super) fn dispatch(
        &self,
        caller: &VmInventory,
        gate: &mut Gate,
        action: i64,
        mutation: Mutation,
    ) -> Outcome<Value> {
        let controller = &caller.supervisor.controller;
        let subject = mutation.vm();
        let refused = |reason| caller.refuse(action, subject, Fault(native_reason(reason)));
        let host = if matches!(mutation.work, Work::Stop) {
            HostObservation {
                owner_idle: None,
                logical_cpus: None,
                available_ram: None,
                capacity: None,
            }
        } else {
            if let Err(reason) = self.refresh_epoch(controller) {
                return refused(reason);
            }
            match self.qualification.observe_host(MEMORY) {
                Ok((_, host)) => host,
                Err(reason) => return refused(reason),
            }
        };
        let id = mutation.id;
        let key = mutation.key;
        let supervisor = caller.supervisor.clone();
        let principal = caller.principal.clone();
        let owner_work = |worker, operation| OwnerWork {
            principal: principal.clone(),
            key,
            operation,
            action,
            worker,
        };
        match mutation.work {
            Work::Build => {
                let recipe = [self.recipes().ubuntu, self.recipes().windows]
                    .into_iter()
                    .find(|recipe| recipe.recipe_id == id)
                    .ok_or(Refusal::RecipeUnqualified)?;
                let name = hex::encode(id);
                let (retained, transient) = build_claims(self.guest(recipe));
                let request = BuildRequest {
                    principal: &principal,
                    key,
                    recipe: &name,
                    digest: recipe.recipe_sha256,
                    retained,
                    transient,
                };
                let (slots, worker, operation) =
                    match controller.admit_build(request, host, RESOURCES, |_, pair| {
                        self.sealed_evidence(pair, Instant::now() + Duration::from_secs(30))
                            .ok()
                    })?? {
                        BuildAdmission::Existing(operation) => {
                            return Ok(receipt(operation.id, None));
                        }
                        BuildAdmission::Fresh(slots, worker, operation) => {
                            (*slots, worker, operation)
                        }
                    };
                let work = owner_work(worker, operation.id);
                spawn_owned(gate, supervisor, move |native, supervisor, executor| {
                    let outcome = executor.block_on(native.build(supervisor, &work, slots, recipe));
                    match outcome {
                        Ok(proof) => {
                            let image_bytes = proof.iter().map(|file| file.pin.size).sum::<u64>();
                            let aligned = (image_bytes + 65535) & !65535;
                            let persistent = aligned.min(retained.images);
                            let request = BuildRequest {
                                principal: &work.principal,
                                key: work.key,
                                recipe: &name,
                                digest: recipe.recipe_sha256,
                                retained: DiskClaim {
                                    images: persistent,
                                    ..retained
                                },
                                transient: DiskClaim {
                                    images: transient.images + retained.images - persistent,
                                    ..transient
                                },
                            };
                            if supervisor
                                .controller
                                .finish_build(&work.worker, slots, request, proof)
                                .is_ok()
                            {
                                return Ok(());
                            }
                            supervisor.controller.fail_build_with(
                                &work.worker,
                                slots,
                                false,
                                Some(FailureReason::SealFailed),
                            )
                        }
                        Err(reason) => supervisor.controller.fail_build_with(
                            &work.worker,
                            slots,
                            false,
                            Some(reason),
                        ),
                    }
                });
                Ok(receipt(operation.id, None))
            }
            Work::Create(ttl) => {
                let (recipe, golden) = controller
                    .sealed_test_pair_for_template(id, self.recipes())?
                    .ok_or(NOT_FOUND)?;
                let proof = self
                    .sealed_evidence(golden, Instant::now() + Duration::from_secs(30))
                    .map_err(|reason| Fault(native_reason(reason)))?;
                let disk = DiskClaim {
                    images: image_size(self.guest(recipe)) + GIB,
                    metadata: 4096,
                    staging: 0,
                };
                let request = Request {
                    principal: &principal,
                    request_id: key,
                    vm: [0; 16],
                    disk,
                    operation: Operation::Clone {
                        template: id,
                        lease_seconds: Some(ttl),
                    },
                };
                let expires = || {
                    controller
                        .foreground_replay(
                            &principal,
                            key,
                            Subject::Template(
                                id,
                                Sha256::new()
                                    .chain_update(b"arxcovo.clone.ttl.v1\0")
                                    .chain_update(ttl.to_be_bytes())
                                    .finalize()
                                    .into(),
                            ),
                            action,
                        )
                        .ok()
                        .and_then(Result::ok)
                        .flatten()
                        .and_then(|prior| prior.lease_end)
                };
                let (slots, worker, operation) = match controller
                    .admit_linked_clone(request, host, RESOURCES, golden, proof)??
                {
                    CloneAdmission::Existing(operation) => {
                        return Ok(receipt(operation.id, expires()));
                    }
                    CloneAdmission::Fresh(_, slots, worker, operation) => {
                        (slots, worker, operation)
                    }
                };
                let lease = expires();
                let work = owner_work(worker, operation.id);
                spawn_owned(gate, supervisor, move |native, supervisor, executor| {
                    let result = executor.block_on(native.clone_pair(
                        supervisor,
                        &work,
                        slots,
                        (golden, proof),
                        recipe,
                    ));
                    supervisor.controller.finish_linked_clone_with(
                        &work.worker,
                        slots,
                        result.map_err(|_| Some(FailureReason::CloneFailed)),
                    )?;
                    Ok(())
                });
                Ok(receipt(operation.id, lease))
            }
            Work::Start | Work::Script(..) | Work::Stop => {
                self.dispatch_vm(caller, gate, action, mutation, host)
            }
            Work::Extend(ttl) => {
                let end = controller.extend_lease(&principal, id, key, ttl)??;
                let digest = Subject::Vm(id);
                let replay = controller
                    .foreground_replay(&principal, key, digest, action)??
                    .ok_or(STATE_UNAVAILABLE)?;
                Ok(receipt(replay.operation.id, Some(end)))
            }
            Work::Snapshot(ref request) => {
                self.dispatch_snapshot(caller, gate, action, &mutation, request, host)
            }
            _ => refused(MISSING),
        }
    }

    fn guest(&self, recipe: RecipeIdentity) -> Guest {
        if recipe == self.recipes().windows {
            Guest::WindowsX86_64
        } else {
            Guest::UbuntuX86_64
        }
    }

    fn dispatch_vm(
        &self,
        caller: &VmInventory,
        gate: &mut Gate,
        action: i64,
        mutation: Mutation,
        host: HostObservation,
    ) -> Outcome<Value> {
        let controller = &caller.supervisor.controller;
        let principal = &caller.principal;
        let vm = mutation.id;
        let key = mutation.key;
        let mut running = self.running.lock().map_err(|_| STATE_UNAVAILABLE)?;
        let instance = running.get_mut(&vm);
        let active = match mutation.work {
            Work::Start if instance.is_none() => Some(
                controller
                    .native_start_pair(principal, vm)?
                    .ok_or(Refusal::Busy)?,
            ),
            Work::Stop
                if instance
                    .as_ref()
                    .is_some_and(|item| item.stop_worker.is_none()) =>
            {
                None
            }
            Work::Script(..) if instance.as_ref().is_some_and(|item| item.witness.is_some()) => {
                None
            }
            _ => return caller.refuse(action, Some(vm), NOT_READY),
        };
        let operation = match &mutation.work {
            Work::Start => Operation::Start,
            Work::Stop => Operation::Stop,
            Work::Script(script, timeout) => Operation::Script {
                script,
                timeout_seconds: u64::from(*timeout),
            },
            _ => return Err(INVALID),
        };
        let admission = controller.admit_native_operation(
            Request {
                principal,
                request_id: key,
                vm,
                operation,
                disk: DiskClaim::default(),
            },
            host,
            RESOURCES,
            instance
                .as_ref()
                .filter(|item| item.witness.is_none())
                .map(|item| item.epoch),
        )?;
        let worker = match admission {
            Admission::Admitted(worker) => worker,
            Admission::Existing(receipt) => {
                return Ok(super::receipt(
                    controller.foreground_operation_ref(principal, receipt.activity_id)?,
                    None,
                ));
            }
            Admission::Refused(reason) => return Err(reason.into()),
        };
        let operation = controller.foreground_operation_ref(principal, worker.activity_id())?;
        if let Some(active) = active {
            let (queue, receiver) = mpsc::channel(1);
            let control = RunControl::default();
            running.insert(
                vm,
                Running {
                    queue,
                    control: control.clone(),
                    epoch: operation,
                    stop_worker: None,
                    witness: None,
                },
            );
            drop(running);
            let work = OwnerWork {
                principal: principal.clone(),
                key,
                operation,
                action,
                worker,
            };
            spawn_owned(
                gate,
                caller.supervisor.clone(),
                move |native, supervisor, executor| {
                    let result = executor
                        .block_on(native.run_vm(supervisor, vm, &work, active, receiver, control));
                    let mut running = native
                        .running
                        .lock()
                        .map_err(|_| anyhow::anyhow!("TEST run custody lost"))?;
                    if let Some(instance) = running.remove(&vm)
                        && let Some(stop) = instance.stop_worker
                    {
                        supervisor
                            .controller
                            .finish_native_stop(&stop, vm, instance.epoch)?;
                    }
                    result
                },
            );
        } else if let Some(instance) = instance {
            if matches!(mutation.work, Work::Stop) {
                instance.stop_worker = Some(worker);
                instance.request_stop();
                return Ok(receipt(operation, None));
            }
            if let Err(error) = instance.queue.try_send((
                OwnerWork {
                    principal: principal.clone(),
                    key,
                    operation,
                    action,
                    worker,
                },
                mutation.work,
            )) {
                let (worker, _) = error.into_inner();
                controller.finish(&worker.worker, Completion::CleanupUnconfirmed)?;
                instance.control.cancel.cancel();
                return Err(STATE_UNAVAILABLE);
            }
        }
        Ok(receipt(operation, None))
    }

    fn dispatch_snapshot(
        &self,
        caller: &VmInventory,
        gate: &mut Gate,
        action: i64,
        mutation: &Mutation,
        request: &SnapshotRequest,
        host: HostObservation,
    ) -> Outcome<Value> {
        let controller = &caller.supervisor.controller;
        let (work, worker, operation) = controller.admit_snapshot(
            &caller.principal,
            mutation.key,
            mutation.id,
            action,
            request,
            None,
            host,
            RESOURCES,
        )??;
        if work.is_static {
            return Err(STATE_UNAVAILABLE);
        }
        if work.candidate.is_some() {
            let owner = OwnerWork {
                principal: caller.principal.clone(),
                key: mutation.key,
                operation: operation.id,
                action,
                worker,
            };
            spawn_owned(
                gate,
                caller.supervisor.clone(),
                move |native, supervisor, executor| {
                    use crate::store::vm::snapshot::StagedLeaf;
                    let mut leaves = [StagedLeaf::Untouched; 2];
                    if native
                        .copy_snapshot(supervisor, executor, &owner, &work, &mut leaves)
                        .is_err()
                    {
                        supervisor
                            .controller
                            .fail_snapshot_copy(&owner.worker, &work, leaves)?;
                    }
                    Ok(())
                },
            );
        }
        Ok(receipt(operation.id, None))
    }

    fn copy_snapshot(
        &self,
        supervisor: &Supervisor,
        executor: &tokio::runtime::Runtime,
        owner: &OwnerWork,
        work: &crate::store::vm::snapshot::Work,
        leaves: &mut [crate::store::vm::snapshot::StagedLeaf; 2],
    ) -> anyhow::Result<()> {
        use crate::store::vm::snapshot::{Pair, StagedLeaf};
        use snapshot::{
            ActiveRelease, Compatibility, CopyPlan, Destination, Golden, Leaf, Limits, Lineage,
            StoredPair,
        };
        let controller = &supervisor.controller;
        let native_error = |_| anyhow::anyhow!("TEST snapshot custody unavailable");
        anyhow::ensure!(
            !work.is_static && work.active.origin == work.active.template,
            "TEST snapshot lineage"
        );
        let by = owner.worker.deadline();
        let refresh = || self.refresh_epoch(controller);
        let slots = work
            .candidate
            .ok_or_else(|| anyhow::anyhow!("TEST snapshot candidate missing"))?;
        let intent = self.intent(supervisor, owner, slots.disk.generation, &refresh);
        let destination = self
            .reserve_workspace(&intent, &[slots.disk, slots.vars])
            .map_err(native_error)?;
        let active_ws = self
            .qualification
            .generation_workspace(work.active.pair.0.disk.generation, false)
            .map_err(native_error)?;
        let source_ws = (work.action == 6)
            .then(|| {
                self.qualification
                    .generation_workspace(work.source.0.disk.generation, false)
            })
            .transpose()
            .map_err(native_error)?;
        let source_ref = source_ws.as_ref().unwrap_or(&active_ws);
        let active = open_pair(&active_ws, work.active.pair.0, work.active.pair.1, by)
            .map_err(native_error)?;
        let source = source_ws
            .as_ref()
            .map(|ws| open_pair(ws, work.source.0, work.source.1, by))
            .transpose()
            .map_err(native_error)?;
        let (golden, evidence) = work
            .active
            .golden
            .ok_or_else(|| anyhow::anyhow!("TEST snapshot golden missing"))?;
        let golden_ws = self
            .qualification
            .generation_workspace(golden.disk.generation, false)
            .map_err(native_error)?;
        let (golden_disk, _) = open_pair(&golden_ws, golden, evidence, by).map_err(native_error)?;
        let golden_path = golden_ws.slot_path(golden.disk).map_err(native_error)?;
        let layout = Golden {
            object: golden.disk.key,
            path: golden_path,
            identity: evidence[0].identity,
            pin: evidence[0].pin,
        };
        let size = crate::vm::qemu_img::inspect_header(
            &active.0.header().map_err(native_error)?,
            Some(&layout.path),
        )
        .map_err(|_| anyhow::anyhow!("TEST snapshot backing refused"))?;
        let firmware = self.qualification.firmware_pin().map_err(native_error)?;
        let to_stored = |workspace: &ProductionWorkspace,
                         (slots, [disk, vars]): Pair|
         -> Result<_, NativeRefusal> {
            let leaf = |slot: ArtifactSlot, proof: ArtifactEvidence| -> Result<_, NativeRefusal> {
                Ok(Leaf {
                    key: slot.key,
                    path: workspace.slot_path(slot)?,
                    identity: proof.identity,
                    pin: proof.pin,
                })
            };
            Ok(StoredPair {
                compatibility: Compatibility {
                    host: work.active.host,
                    vm: work.vm,
                    lineage: Lineage::Test {
                        template: work.active.template,
                        golden: layout.clone(),
                    },
                    recipe: work.active.recipe,
                    firmware,
                    virtual_disk_bytes: size,
                    vars_bytes: vars.pin.size,
                },
                generation: slots.disk.generation,
                disk: leaf(slots.disk, disk)?,
                vars: leaf(slots.vars, vars)?,
            })
        };
        let active_pair = to_stored(&active_ws, work.active.pair).map_err(native_error)?;
        let selected = source_ws
            .as_ref()
            .map(|ws| to_stored(ws, work.source).map(|pair| (work.target, pair)))
            .transpose()
            .map_err(native_error)?;
        let action = if work.action == 5 {
            snapshot::Action::Capture {
                snapshot: work.target,
            }
        } else {
            snapshot::Action::Restore {
                snapshot: work.target,
            }
        };
        let plan = CopyPlan::new(
            snapshot::Intent {
                activity_id: work.activity,
                action,
                active: active_pair,
                active_writer_epoch: work.active.epoch,
                release: ActiveRelease {
                    generation: work.active.pair.0.disk.generation,
                    writer_epoch: work.active.epoch,
                    disposition: if work.active.clean {
                        StopDisposition::CleanRelease
                    } else {
                        StopDisposition::DirtyRelease
                    },
                    endpoints_released: true,
                },
                selected,
                destination: Destination {
                    generation: slots.disk.generation,
                    disk_key: slots.disk.key,
                    vars_key: slots.vars.key,
                },
            },
            self.qualification.selected_store_path(),
            destination.binding().workspace.volume,
            Limits {
                vm_points: work.points[0],
                host_points: work.points[1],
                max_file_bytes: 41 * GIB,
                max_virtual_bytes: 40 * GIB,
                restore_growth_bytes: if work.action == 6 { 1 << 20 } else { 0 },
                staging_bytes: 65536,
                metadata_bytes: 4096,
            },
        )
        .map_err(|_| anyhow::anyhow!("TEST snapshot plan refused"))?;
        anyhow::ensure!(
            plan.additional_claim().total() == work.claim.total(),
            "TEST snapshot claim mismatch"
        );
        let mut make = |index, slot| -> anyhow::Result<SlotWriter> {
            let permit = intent
                .stage(destination.binding(), slot)
                .map_err(native_error)?;
            controller.snapshot_leaf_begin(&owner.worker, work, slot)?;
            leaves[index] = StagedLeaf::Unknown;
            let file = destination
                .create_writer(slot, permit)
                .map_err(native_error)?;
            let identity = file.held().identity();
            let directory = destination.binding().workspace.file;
            controller.snapshot_leaf_identity(&owner.worker, work, slot, identity, directory)?;
            leaves[index] = StagedLeaf::Identified(identity, directory);
            Ok(file)
        };
        let disk = make(0, slots.disk)?;
        let vars = make(1, slots.vars)?;
        let ids = [disk.held().identity(), vars.held().identity()];
        let mut copy = NativeCopy {
            native: self,
            supervisor,
            owner,
            work,
            executor,
            active: [active.0, active.1],
            source: source.map(|(disk, vars)| [disk, vars]),
            golden: golden_disk,
            source_ws: source_ref,
            destination: &destination,
            staging: [Some(disk), Some(vars)],
            written: [None, None],
            exclusions: Vec::new(),
            last_check: None,
        };
        let cancelled = || supervisor.cancel.is_cancelled();
        let mut verified = snapshot::copy_pair(&plan, &owner.worker, &mut copy, ids, &cancelled)
            .map_err(|_| anyhow::anyhow!("TEST snapshot copy refused"))?;
        let pair = verified.pair();
        let directory = destination.binding().workspace.file;
        let proof = [&pair.disk, &pair.vars].map(|file| ArtifactEvidence {
            identity: file.identity,
            directory,
            pin: file.pin,
        });
        for (slot, evidence) in [slots.disk, slots.vars].into_iter().zip(proof) {
            controller.snapshot_leaf_complete(&owner.worker, work, slot, evidence)?;
        }
        verified
            .recheck()
            .map_err(|_| anyhow::anyhow!("TEST snapshot recheck refused"))?;
        let (_, host) = self
            .qualification
            .observe_host(MEMORY)
            .map_err(native_error)?;
        controller.finish_snapshot_copy(&owner.worker, work, proof, None, host, RESOURCES)
    }

    pub(super) fn project_status(
        &self,
        controller: &Controller,
        principal: &str,
        vm: Id,
        mut row: Value,
    ) -> Value {
        let witness = self.running.lock().ok().and_then(|running| {
            running.get(&vm)?.witness.filter(|witness| {
                row["state"] == "running"
                    && witness.matches(
                        controller
                            .native_running_epoch(principal, vm)
                            .ok()
                            .flatten(),
                    )
            })
        });
        row["ready"] = json!(witness.is_some());
        if row["guest"] == "windows"
            && let Some(witness) = witness
        {
            row["native_boot_witness"] = json!({
                "source": "guest_ready_handshake", "vm_id": hex::encode(vm),
                "generation": hex::encode(witness.generation), "operation_id": hex::encode(witness.operation),
                "witness_id": hex::encode(witness.id)
            });
        }
        row
    }

    fn witness(&self, vm: Id, value: Option<Witness>) -> anyhow::Result<()> {
        self.running
            .lock()
            .map_err(|_| anyhow::anyhow!("TEST run custody lost"))?
            .get_mut(&vm)
            .ok_or_else(|| anyhow::anyhow!("TEST run custody lost"))?
            .witness = value;
        Ok(())
    }

    async fn run_vm(
        &self,
        supervisor: &Supervisor,
        vm: Id,
        work: &OwnerWork,
        active: crate::store::vm::snapshot::Active,
        mut queue: mpsc::Receiver<(OwnerWork, Work)>,
        control: RunControl,
    ) -> anyhow::Result<()> {
        let cancel = control.cancel.clone();
        let controller = &supervisor.controller;
        let recipe = [self.recipes().ubuntu, self.recipes().windows]
            .into_iter()
            .find(|id| id.recipe_sha256 == active.recipe);
        let prepare_by = work.worker.deadline() - crate::vm::guest::INSTALLED_BOOT_BUDGET;
        let prepared = || -> Result<_, NativeRefusal> {
            let recipe = recipe.ok_or(MISSING)?;
            let (pair, evidence) = active.pair;
            let workspace = self
                .qualification
                .generation_workspace(pair.disk.generation, false)?;
            // Verify both stopped bytes before the generation can write again.
            open_pair(&workspace, pair, evidence, prepare_by)?;
            let (golden, proof) = active.golden.ok_or(PATH)?;
            let golden_ws = self
                .qualification
                .generation_workspace(golden.disk.generation, false)?;
            let (golden_disk, _) = open_pair(&golden_ws, golden, proof, prepare_by)?;
            let backing = golden_disk.artifact(ArtifactRole::Golden)?;
            Ok((recipe, workspace, golden_disk, backing))
        };
        let fail_start = || -> anyhow::Result<()> {
            controller.finish_native_start(
                &work.worker,
                work.operation,
                Completion::CleanupUnconfirmed,
            )?;
            Ok(())
        };
        let Ok((recipe, workspace, _golden_pin, backing)) = prepared() else {
            return fail_start();
        };
        let (pair, evidence) = active.pair;
        let ids = evidence.map(|file| file.identity);
        let refresh = || {
            if supervisor.cancel.is_cancelled() || cancel.is_cancelled() {
                return Err(MISSING);
            }
            let captured = self.refresh_epoch(controller)?;
            if !controller
                .native_vm_unexpired(&work.principal, vm)
                .map_err(|_| MISSING)?
            {
                return Err(MISSING);
            }
            Ok(captured)
        };
        let intent = self.intent(supervisor, work, pair.disk.generation, &refresh);
        let Ok(qualified) = self.qualification.prepare_boot(BootReservation {
            workspace: &workspace,
            recipe,
            phase: None,
            pair,
            identities: ids,
            backing: Some(&backing),
            staged: None,
        }) else {
            return fail_start();
        };
        let planner = qualified.planner();
        let Ok(plan) = planner.plan() else {
            return fail_start();
        };
        if Instant::now() >= prepare_by {
            return fail_start();
        }
        let launch = self.launch(
            &intent,
            &workspace,
            plan,
            qualified.cwd(),
            Some(pair),
            None,
            Some(&qualified),
            Some(recipe),
            &[pair.disk, pair.vars],
        );
        let Ok(mut launch) = launch else {
            return fail_start();
        };
        let boot_refresh = || {
            if Instant::now() >= work.worker.deadline() {
                return Err(MISSING);
            }
            refresh()
        };
        let failed_release = match launch.boot(&cancel, boot_refresh, || true).await {
            Err(failure) => {
                controller.finish_native_start(
                    &work.worker,
                    work.operation,
                    Completion::CleanupUnconfirmed,
                )?;
                Some((failure.disposition, failure.released.map(|held| *held)))
            }
            Ok(()) => {
                let ready = {
                    let mut running = self
                        .running
                        .lock()
                        .map_err(|_| anyhow::anyhow!("TEST run custody lost"))?;
                    let instance = running
                        .get_mut(&vm)
                        .ok_or_else(|| anyhow::anyhow!("TEST run custody lost"))?;
                    let witness = crate::store::vm::new_id()
                        .ok()
                        .filter(|id| {
                            *id != work.operation
                                && !cancel.is_cancelled()
                                && !supervisor.cancel.is_cancelled()
                        })
                        .map(|id| Witness {
                            generation: pair.disk.generation,
                            operation: work.operation,
                            id,
                        });
                    let completion = if witness.is_some() {
                        Completion::Succeeded
                    } else {
                        Completion::CleanupUnconfirmed
                    };
                    if controller.finish_native_start(&work.worker, work.operation, completion)?
                        == Status::Succeeded
                    {
                        instance.witness = witness;
                    }
                    instance.witness.is_some()
                };
                if ready {
                    let mut interval = tokio::time::interval(Duration::from_secs(1));
                    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                    let mut failed_release = None;
                    loop {
                        tokio::select! {
                            biased;
                            _ = supervisor.cancel.cancelled() => break,
                            _ = cancel.cancelled() => break,
                            _ = control.stop.cancelled() => break,
                            work = queue.recv() => match work {
                                Some((worker, Work::Script(script, timeout))) => {
                                    let script_refresh = || {
                                        if Instant::now() >= worker.worker.deadline() { return Err(MISSING); }
                                        refresh()
                                    };
                                    let script_intent = self.intent(supervisor, &worker, pair.disk.generation, &script_refresh);
                                    let Ok(authority) = script_intent.authorize(workspace.binding(), &[pair.disk, pair.vars]) else {
                                        controller.finish_script(&worker.worker, ScriptResult::Unavailable)?;
                                        continue;
                                    };
                                    match launch.run_script(authority, script, timeout, &cancel, script_refresh, || true).await {
                                        Ok(result) => { controller.finish_script(&worker.worker, script_result(result))?; }
                                        Err(failure) => {
                                            self.witness(vm, None)?;
                                            controller.finish_with_cause(&worker.worker, Completion::CleanupUnconfirmed, Some(FailureReason::ExecutionUnknown))?;
                                            failed_release = Some((failure.disposition, failure.released.map(|held| *held)));
                                            break;
                                        }
                                    }
                                }
                                _ => break,
                            },
                            _ = interval.tick() => {
                                if let Err(failure) = launch.ping(&cancel, refresh, || true).await {
                                    failed_release = Some((failure.disposition, failure.released.map(|held| *held)));
                                    break;
                                }
                            }
                        }
                    }
                    failed_release
                } else {
                    None
                }
            }
        };
        self.witness(vm, None)?;
        queue.close();
        while let Ok((queued, _)) = queue.try_recv() {
            controller.finish_script(&queued.worker, ScriptResult::Unavailable)?;
        }
        let settlement = match failed_release {
            Some(failed) => failed,
            None => launch.stop_generation(&cancel, || true).await,
        };
        reconcile_release(
            settlement,
            &control.retry,
            |(disposition, held)| {
                if held
                    .as_ref()
                    .is_some_and(|held| held.matches(workspace.binding(), pair, ids))
                    && let Ok(files) = read_pair(
                        &workspace,
                        pair,
                        ids,
                        Instant::now() + Duration::from_secs(20),
                    )
                {
                    let mut running = self
                        .running
                        .lock()
                        .map_err(|_| anyhow::anyhow!("TEST run custody lost"))?;
                    let instance = running
                        .get_mut(&vm)
                        .ok_or_else(|| anyhow::anyhow!("TEST run custody lost"))?;
                    if instance
                        .stop_worker
                        .as_ref()
                        .is_some_and(|worker| Instant::now() < worker.deadline())
                        && controller
                            .native_running_epoch(&work.principal, vm)
                            .ok()
                            .flatten()
                            == Some((pair.disk.generation, work.operation))
                        && let Some(stop) = &instance.stop_worker
                    {
                        controller.finish_offline_stop(
                            stop,
                            vm,
                            work.operation,
                            Some((pair, files)),
                            *disposition,
                        )?;
                        instance.stop_worker.take();
                    } else {
                        controller.native_record_release(
                            vm,
                            work.operation,
                            (pair, files),
                            *disposition,
                        )?;
                    }
                    return Ok(true);
                }
                Ok(false)
            },
            || {
                controller.native_quarantine(vm, work.operation)?;
                {
                    let mut running = self
                        .running
                        .lock()
                        .map_err(|_| anyhow::anyhow!("TEST run custody lost"))?;
                    let instance = running
                        .get_mut(&vm)
                        .ok_or_else(|| anyhow::anyhow!("TEST run custody lost"))?;
                    if let Some(stop) = &instance.stop_worker {
                        controller.finish_native_stop(stop, vm, work.operation)?;
                        instance.stop_worker.take();
                    }
                }
                Ok(())
            },
            async || launch.stop_generation(&cancel, || true).await,
        )
        .await
    }

    fn sealed_evidence(
        &self,
        pair: PairSlots,
        deadline: Instant,
    ) -> Result<[ArtifactEvidence; 2], NativeRefusal> {
        self.qualification.read_sealed_pair(pair, deadline)
    }

    fn intent<'a>(
        &'a self,
        supervisor: &'a Supervisor,
        work: &'a OwnerWork,
        generation: Id,
        refresh: &'a dyn Fn() -> Result<Instant, NativeRefusal>,
    ) -> Intent<'a, ForegroundClock> {
        Intent {
            controller: &supervisor.controller,
            worker: &work.worker,
            binding: NativeEffectBinding {
                principal: &work.principal,
                request: work.key,
                operation: work.operation,
                action: work.action,
                generation,
            },
            epoch: self.epoch.id,
            seal_sha256: self.seal_sha256,
            cancel: &supervisor.cancel,
            refresh,
        }
    }

    fn reserve_workspace(
        &self,
        intent: &Intent<'_, ForegroundClock>,
        slots: &[ArtifactSlot],
    ) -> Result<ProductionWorkspace, NativeRefusal> {
        if !intent
            .controller
            .validate_native_effect(intent.worker, &intent.binding, slots)
            .map_err(|_| MISSING)?
            || intent.cancel.is_cancelled()
        {
            return Err(MISSING);
        }
        self.qualification.observe_host(MEMORY)?;
        self.qualification
            .generation_workspace(intent.binding.generation, true)
    }

    #[allow(clippy::too_many_arguments)]
    fn launch<'a>(
        &'a self,
        intent: &Intent<'_, ForegroundClock>,
        workspace: &'a ProductionWorkspace,
        plan: CommandPlan<'a>,
        cwd: &'a paths::LocalPath,
        pair: Option<PairSlots>,
        stage: Option<InstallerStage>,
        boot: Option<&'a QualifiedBoot<'a>>,
        recipe: Option<RecipeIdentity>,
        slots: &[ArtifactSlot],
    ) -> Result<NativeLaunch<'a>, NativeRefusal> {
        let permit = intent.launch(
            workspace.binding(),
            LaunchTarget {
                plan: &plan,
                cwd,
                pair,
                installer: stage,
                recipe,
                memory: MEMORY,
                cpu_rate: CPU_RATE,
            },
            slots,
        )?;
        NativeLaunch::hold(
            &self.qualification,
            LaunchRequest {
                plan,
                boot_inputs: boot,
                cwd,
                workspace,
                pair,
                installer: stage,
                memory: MEMORY,
                cpu_rate: CPU_RATE,
            },
            permit,
            intent.refresh,
        )
    }

    async fn create_disk(
        &self,
        intent: &Intent<'_, ForegroundClock>,
        workspace: &ProductionWorkspace,
        slot: ArtifactSlot,
        size: u64,
        golden: Option<(&Artifact, &[u8])>,
    ) -> Result<(FileIdentity, ArtifactEvidence), NativeRefusal> {
        let (disk, offline) =
            workspace.fresh_disk(slot, intent.stage(workspace.binding(), slot)?)?;
        let disk_arg = disk.artifact(ArtifactRole::Staging)?;
        let id = disk.identity();
        let helper = self.qualification.helper_image()?;
        let cwd = self.qualification.command_cwd()?;
        let plan =
            crate::vm::qemu_img::create(&offline, &helper, &disk_arg, golden, size, size + GIB)
                .map_err(|_| PATH)?;
        let launch = self.launch(
            intent,
            workspace,
            plan,
            &cwd,
            None,
            None,
            None,
            None,
            &[slot],
        )?;
        let by = tokio::time::Instant::from_std(intent.worker.deadline())
            .min(tokio::time::Instant::now() + Duration::from_secs(30));
        let result = launch
            .run_helper(intent.cancel, by, intent.refresh, || true)
            .await;
        let mut inputs = vec![
            (ArtifactRole::QemuImg, native_identity(&helper)?),
            (ArtifactRole::Staging, id),
        ];
        if let Some((golden, _)) = golden {
            inputs.push((ArtifactRole::Golden, native_identity(golden)?));
        }
        let proof = result.released.ok_or(PATH)?;
        if result.disposition != StopDisposition::CleanRelease
            || !proof.matches_helper(workspace.binding(), PlanKind::Mutate, &inputs)
        {
            return Err(PATH);
        }
        result.output?;
        let mut disk = workspace.open_slot(slot, id, false)?;
        let backing = golden
            .map(|(golden, _)| golden.path_for(ArtifactRole::Golden))
            .transpose()
            .map_err(|_| PATH)?;
        if crate::vm::qemu_img::inspect_header(&disk.header()?, backing) != Ok(size) {
            return Err(PATH);
        }
        let evidence = disk.evidence(size + GIB, intent.worker.deadline())?;
        intent
            .controller
            .native_helper_created(intent.worker, slot, evidence)
            .map_err(|_| PATH)?;
        drop(proof);
        Ok((id, evidence))
    }

    async fn clone_pair(
        &self,
        supervisor: &Supervisor,
        work: &OwnerWork,
        slots: PairSlots,
        (golden, evidence): (PairSlots, [ArtifactEvidence; 2]),
        recipe: RecipeIdentity,
    ) -> Result<[ArtifactEvidence; 2], NativeRefusal> {
        let refresh = || self.refresh_epoch(&supervisor.controller);
        let intent = self.intent(supervisor, work, slots.disk.generation, &refresh);
        let workspace = self.reserve_workspace(&intent, &[slots.disk, slots.vars])?;
        let backing_workspace = self
            .qualification
            .generation_workspace(golden.disk.generation, false)?;
        let (golden_disk, golden_vars) =
            open_pair(&backing_workspace, golden, evidence, work.worker.deadline())?;
        let backing = golden_disk.artifact(ArtifactRole::Golden)?;
        let header = golden_disk.header()?;
        let (_, disk) = self
            .create_disk(
                &intent,
                &workspace,
                slots.disk,
                image_size(self.guest(recipe)),
                Some((&backing, &header)),
            )
            .await?;
        let mut vars = workspace.create_slot(
            slots.vars,
            &golden_vars.vars()?,
            intent.stage(workspace.binding(), slots.vars)?,
        )?;
        let vars = vars.evidence(16 << 20, work.worker.deadline())?;
        Ok([disk, vars])
    }

    async fn install(
        &self,
        intent: &Intent<'_, ForegroundClock>,
        reservation: BootReservation<'_>,
        stage: InstallerStage,
    ) -> Result<[ArtifactEvidence; 2], NativeRefusal> {
        let workspace = reservation.workspace;
        let pair = reservation.pair;
        let ids = reservation.identities;
        let recipe = reservation.recipe;
        let qualified = self.qualification.prepare_boot(reservation)?;
        let planner = qualified.planner();
        let mut launch = self.launch(
            intent,
            workspace,
            planner.plan()?,
            qualified.cwd(),
            Some(pair),
            Some(stage),
            Some(&qualified),
            Some(recipe),
            &[pair.disk, pair.vars],
        )?;
        let settlement = launch
            .run_installer(intent.cancel, intent.refresh, || true)
            .await?;
        let InstallerSettlement::Released {
            generation,
            finalization,
        } = settlement
        else {
            return Err(NativeRefusal::InstallerPhaseMismatch);
        };
        if !generation.matches(workspace.binding(), pair, ids) {
            return Err(PATH);
        }
        let proof = read_pair(workspace, pair, ids, stage.deadline().into_std())?;
        if stage.phase() == InstallerPhase::WindowsFinalize
            && finalization
                .ok_or(PATH)?
                .observe(pair, proof, stage.deadline().into_std())?
                != (true, true)
        {
            return Err(NativeRefusal::InstallerPhaseMismatch);
        }
        Ok(proof)
    }

    async fn verify_image(
        &self,
        intent: &Intent<'_, ForegroundClock>,
        reservation: BootReservation<'_>,
        budget: &InstallerBudget,
        before_finalize: bool,
    ) -> Result<[ArtifactEvidence; 2], NativeRefusal> {
        let workspace = reservation.workspace;
        let pair = reservation.pair;
        let ids = reservation.identities;
        let recipe = reservation.recipe;
        let qualified = self.qualification.prepare_boot(reservation)?;
        let planner = qualified.planner();
        let mut launch = self.launch(
            intent,
            workspace,
            planner.plan()?,
            qualified.cwd(),
            Some(pair),
            None,
            Some(&qualified),
            Some(recipe),
            &[pair.disk, pair.vars],
        )?;
        let deadline = budget.clip(Duration::from_secs(4 * 60));
        let refresh = || {
            if tokio::time::Instant::now() >= deadline {
                return Err(NativeRefusal::InstallerDeadlineExpired);
            }
            (intent.refresh)()
        };
        launch
            .boot(intent.cancel, refresh, || true)
            .await
            .map_err(|failure| failure.reason)?;
        let mut healthy = true;
        for script in self
            .qualification
            .installer_checks(recipe, before_finalize)?
        {
            let authority = intent.authorize(workspace.binding(), &[pair.disk, pair.vars])?;
            match launch
                .run_script(authority, script, 30, intent.cancel, refresh, || true)
                .await
            {
                Ok((ScriptStatus::Exited, Some(0))) => {}
                Ok(_) => {
                    healthy = false;
                    break;
                }
                Err(failure) => return Err(failure.reason),
            }
        }
        let (disposition, release) = launch.stop(intent.cancel, || true).await;
        if disposition != StopDisposition::CleanRelease
            || !release
                .as_ref()
                .is_some_and(|proof| proof.matches(workspace.binding(), pair, ids))
            || !healthy
        {
            return Err(NativeRefusal::GuestEndpointUnverified);
        }
        read_pair(workspace, pair, ids, deadline.into_std())
    }

    async fn build(
        &self,
        supervisor: &Supervisor,
        work: &OwnerWork,
        slots: BuildSlots,
        recipe: RecipeIdentity,
    ) -> Result<[ArtifactEvidence; 2], FailureReason> {
        let controller = &supervisor.controller;
        let refresh = || self.refresh_epoch(controller);
        let primary = slots.pair;
        let verify = slots.verification;
        let intent = self.intent(supervisor, work, primary.disk.generation, &refresh);
        let verification = self.intent(supervisor, work, verify.disk.generation, &refresh);
        let is_windows = self.guest(recipe) == Guest::WindowsX86_64;
        let size = image_size(self.guest(recipe));
        let phase = if is_windows {
            InstallerPhase::WindowsSetup
        } else {
            InstallerPhase::UbuntuInstall
        };
        let budget = InstallerBudget::start(if is_windows {
            InstallerRecipe::Windows
        } else {
            InstallerRecipe::Ubuntu
        });
        let limit = Duration::from_secs(60 * 60);
        let stage = budget
            .stage(phase, limit)
            .ok_or(FailureReason::InstallFailed)?;
        let workspace = self
            .reserve_workspace(&intent, &[primary.disk, primary.vars, slots.seed])
            .map_err(|_| FailureReason::StagingFailed)?;
        let verification_ws = self
            .reserve_workspace(&verification, &[verify.disk, verify.vars])
            .map_err(|_| FailureReason::StagingFailed)?;
        controller
            .build_evidence(
                &work.worker,
                slots.id,
                &[
                    (primary.disk, None),
                    (primary.vars, None),
                    (slots.seed, None),
                    (verify.disk, None),
                    (verify.vars, None),
                ],
            )
            .map_err(|_| FailureReason::StagingFailed)?;
        let staged = self
            .qualification
            .stage_installer(
                InstallerReservation {
                    workspace: &workspace,
                    recipe,
                    phase,
                    pair: primary,
                    seed: slots.seed,
                    deadline: stage.deadline().into_std(),
                },
                |target| {
                    if target.seal_sha256 != self.seal_sha256
                        || target.recipe != recipe
                        || target.phase != phase
                        || target.deadline != stage.deadline().into_std()
                        || target.workspace != workspace.binding()
                        || ![primary.vars, slots.seed].contains(&target.slot)
                    {
                        return Err(MISSING);
                    }
                    intent.stage(target.workspace, target.slot)
                },
            )
            .map_err(|_| FailureReason::StagingFailed)?;
        let [(_, seed), (_, vars)] = staged.artifacts();
        controller
            .build_evidence(&work.worker, slots.id, &[(slots.seed, Some(seed))])
            .map_err(|_| FailureReason::StagingFailed)?;
        let (disk, _) = self
            .create_disk(&intent, &workspace, primary.disk, size, None)
            .await
            .map_err(|_| FailureReason::StagingFailed)?;
        let ids = [disk, vars.identity];
        let mut golden = self
            .install(
                &intent,
                BootReservation {
                    workspace: &workspace,
                    recipe,
                    phase: Some(phase),
                    pair: primary,
                    identities: ids,
                    backing: None,
                    staged: Some(&staged),
                },
                stage,
            )
            .await
            .map_err(|_| FailureReason::InstallFailed)?;
        drop(staged);
        if is_windows {
            self.verify_image(
                &intent,
                BootReservation {
                    workspace: &workspace,
                    recipe,
                    phase: None,
                    pair: primary,
                    identities: ids,
                    backing: None,
                    staged: None,
                },
                &budget,
                true,
            )
            .await
            .map_err(|_| FailureReason::HealthFailed)?;
            let phase = InstallerPhase::WindowsFinalize;
            let stage = budget
                .stage(phase, limit)
                .ok_or(FailureReason::InstallFailed)?;
            golden = self
                .install(
                    &intent,
                    BootReservation {
                        workspace: &workspace,
                        recipe,
                        phase: Some(phase),
                        pair: primary,
                        identities: ids,
                        backing: None,
                        staged: None,
                    },
                    stage,
                )
                .await
                .map_err(|_| FailureReason::InstallFailed)?;
        }
        let (golden_disk, golden_vars) =
            open_pair(&workspace, primary, golden, work.worker.deadline())
                .map_err(|_| FailureReason::SealFailed)?;
        let backing = golden_disk
            .artifact(ArtifactRole::Golden)
            .map_err(|_| FailureReason::SealFailed)?;
        let header = golden_disk
            .header()
            .map_err(|_| FailureReason::SealFailed)?;
        let (disk, _) = self
            .create_disk(
                &verification,
                &verification_ws,
                verify.disk,
                size,
                Some((&backing, &header)),
            )
            .await
            .map_err(|_| FailureReason::CloneFailed)?;
        let vars = verification_ws
            .create_slot(
                verify.vars,
                &golden_vars.vars().map_err(|_| FailureReason::CloneFailed)?,
                verification
                    .stage(verification_ws.binding(), verify.vars)
                    .map_err(|_| FailureReason::CloneFailed)?,
            )
            .map_err(|_| FailureReason::CloneFailed)?;
        let ids = [disk, vars.identity()];
        drop(vars);
        let proof = self
            .verify_image(
                &verification,
                BootReservation {
                    workspace: &verification_ws,
                    recipe,
                    phase: None,
                    pair: verify,
                    identities: ids,
                    backing: Some(&backing),
                    staged: None,
                },
                &budget,
                false,
            )
            .await
            .map_err(|_| FailureReason::HealthFailed)?;
        controller
            .build_evidence(
                &work.worker,
                slots.id,
                &[
                    (primary.disk, Some(golden[0])),
                    (primary.vars, Some(golden[1])),
                    (verify.disk, Some(proof[0])),
                    (verify.vars, Some(proof[1])),
                ],
            )
            .map_err(|_| FailureReason::SealFailed)?;
        workspace
            .remove_released_slot(slots.seed, seed.identity, seed.directory)
            .map_err(|_| FailureReason::SealFailed)?;
        for (slot, evidence) in [(verify.disk, proof[0]), (verify.vars, proof[1])] {
            verification_ws
                .remove_released_slot(slot, evidence.identity, evidence.directory)
                .map_err(|_| FailureReason::SealFailed)?;
        }
        Ok(golden)
    }
}

impl Witness {
    fn matches(self, current: Option<(Id, Id)>) -> bool {
        current == Some((self.generation, self.operation))
            && self.id != [0; 16]
            && self.id != self.operation
    }
}

#[cfg(test)]
mod tests;

fn native_identity(artifact: &Artifact) -> Result<FileIdentity, NativeRefusal> {
    crate::vm::runtime::test_issuer::artifact_identity(artifact).ok_or(PATH)
}

/// A bounded attempt can finish without proof. Keep the owned launch in the
/// caller until evidence is recorded, and let a stop or reaper wake a retry.
async fn reconcile_release<T>(
    mut settlement: T,
    wake: &tokio::sync::Notify,
    mut record: impl FnMut(&T) -> anyhow::Result<bool>,
    mut quarantine: impl FnMut() -> anyhow::Result<()>,
    mut retry: impl AsyncFnMut() -> T,
) -> anyhow::Result<()> {
    let mut delay = Duration::from_secs(2);
    loop {
        if record(&settlement)? {
            return Ok(());
        }
        drop(settlement);
        quarantine()?;
        tokio::select! {
            _ = wake.notified() => {}
            _ = tokio::time::sleep(delay) => {}
        }
        delay = (delay * 2).min(Duration::from_secs(60));
        settlement = retry().await;
    }
}

fn spawn_owned(
    gate: &mut Gate,
    supervisor: Arc<Supervisor>,
    work: impl FnOnce(&SelectedTest, &Supervisor, &tokio::runtime::Runtime) -> anyhow::Result<()>
    + Send
    + 'static,
) {
    while let Some(done) = gate.native_jobs.try_join_next() {
        if !matches!(done, Ok(Ok(()))) {
            supervisor.fence();
        }
    }
    gate.native_jobs.spawn_blocking(move || {
        let native = supervisor
            .native_test
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("TEST epoch lost"))?;
        let executor = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let result = work(native, &supervisor, &executor);
        if result.is_err() {
            supervisor.fence();
        }
        result
    });
}

fn image_size(guest: Guest) -> u64 {
    match guest {
        Guest::UbuntuX86_64 => 24 * GIB,
        Guest::WindowsX86_64 => 40 * GIB,
    }
}

fn script_result(result: (ScriptStatus, Option<u32>)) -> ScriptResult {
    match result {
        (ScriptStatus::Exited, Some(code)) => ScriptResult::Exited(code),
        (ScriptStatus::Unavailable, _) => ScriptResult::Unavailable,
        (ScriptStatus::TimedOut, _) => ScriptResult::TimedOut,
        (ScriptStatus::Terminated, _) => ScriptResult::Terminated,
        _ => ScriptResult::LaunchFailed,
    }
}

fn build_claims(guest: Guest) -> (DiskClaim, DiskClaim) {
    let retained = image_size(guest) + GIB;
    let peak = match guest {
        Guest::UbuntuX86_64 => 51 * GIB,
        Guest::WindowsX86_64 => 83 * GIB,
    };
    (
        DiskClaim {
            images: retained,
            staging: 0,
            metadata: 0,
        },
        DiskClaim {
            images: peak - retained,
            metadata: 0,
            staging: 0,
        },
    )
}

fn open_pair(
    workspace: &ProductionWorkspace,
    pair: PairSlots,
    evidence: [ArtifactEvidence; 2],
    deadline: Instant,
) -> Result<(HeldSlot, HeldSlot), NativeRefusal> {
    let [disk, vars] = evidence;
    let mut disk_slot = workspace.open_slot(pair.disk, disk.identity, false)?;
    let mut vars_slot = workspace.open_slot(pair.vars, vars.identity, false)?;
    if disk_slot.evidence(41 * GIB, deadline)? != disk
        || vars_slot.evidence(16 << 20, deadline)? != vars
    {
        return Err(PATH);
    }
    Ok((disk_slot, vars_slot))
}

fn read_pair(
    workspace: &ProductionWorkspace,
    pair: PairSlots,
    ids: [FileIdentity; 2],
    deadline: Instant,
) -> Result<[ArtifactEvidence; 2], NativeRefusal> {
    let mut disk = workspace.open_slot(pair.disk, ids[0], false)?;
    let mut vars = workspace.open_slot(pair.vars, ids[1], false)?;
    Ok([
        disk.evidence(41 * GIB, deadline)?,
        vars.evidence(16 << 20, deadline)?,
    ])
}

struct NativeCopy<'a> {
    native: &'a SelectedTest,
    supervisor: &'a Supervisor,
    owner: &'a OwnerWork,
    work: &'a crate::store::vm::snapshot::Work,
    executor: &'a tokio::runtime::Runtime,
    active: [HeldSlot; 2],
    source: Option<[HeldSlot; 2]>,
    golden: HeldSlot,
    source_ws: &'a ProductionWorkspace,
    destination: &'a ProductionWorkspace,
    staging: [Option<SlotWriter>; 2],
    written: [Option<HeldSlot>; 2],
    exclusions: Vec<runtime::OfflineLease>,
    last_check: Option<Instant>,
}

pub(super) struct NativeReaper<'a> {
    state: &'a SelectedTest,
    controller: &'a Controller,
}

impl runtime::ReaperRuntime for NativeReaper<'_> {
    fn quiesce(&self, work: &reaper::CleanupWork, deadline: Instant) -> runtime::Quiesced {
        let unknown = runtime::Quiesced::Unconfirmed;
        let Ok(mut running) = self.state.running.lock() else {
            return unknown;
        };
        if let Some(vm) = work.kind.vm()
            && let Some(item) = running.get_mut(&vm)
        {
            if !matches!(work.kind, reaper::CleanupKind::Snapshot { .. }) {
                item.witness = None;
                item.control.cancel.cancel();
                item.control.retry.notify_one();
            }
            return unknown;
        }
        drop(running);
        if Instant::now() >= deadline || !self.controller.native_cleanup_safe(work).unwrap_or(false)
        {
            return unknown;
        }
        runtime::Quiesced::Offline
    }

    fn graceful_stop(&self, _: &reaper::CleanupWork, _: Instant) -> StopDisposition {
        StopDisposition::ChargedQuarantine
    }

    fn force_reap(&self, _: &reaper::CleanupWork, _: Instant) -> StopDisposition {
        StopDisposition::ChargedQuarantine
    }
}

impl reaper::ArtifactCustody for NativeReaper<'_> {
    fn reconcile(&self, work: &reaper::CleanupWork, deadline: Instant) -> bool {
        let run = || -> Result<(), NativeRefusal> {
            let mut removed = self.state.removed.lock().map_err(|_| PATH)?;
            let mut workspaces = HashMap::new();
            let mut selected = Vec::new();
            for asset in &work.artifacts {
                if Instant::now() >= deadline {
                    return Err(PATH);
                }
                let role = match asset.role.as_str() {
                    "disk" => "disk",
                    "vars" => "vars",
                    _ => return Err(PATH),
                };
                let identity = asset.identity.ok_or(PATH)?;
                let directory = asset.directory.ok_or(PATH)?;
                let receipt = RemovedSlot {
                    generation: asset.generation,
                    key: asset.key,
                    directory,
                    file: identity.file,
                    volume: identity.volume,
                };
                if !asset.retain && removed.contains(&receipt) {
                    continue;
                }
                let slot = ArtifactSlot {
                    generation: asset.generation,
                    role,
                    key: asset.key,
                };
                use std::collections::hash_map::Entry;
                let ws = match workspaces.entry(slot.generation) {
                    Entry::Occupied(entry) => entry.into_mut(),
                    Entry::Vacant(entry) => entry.insert(
                        self.state
                            .qualification
                            .generation_workspace(slot.generation, false)?,
                    ),
                };
                if ws.binding().workspace.file != directory {
                    return Err(PATH);
                }
                let handle = ws.open_slot(slot, identity, false)?;
                selected.push((handle, slot, identity, directory, asset.retain, receipt));
            }
            for (handle, slot, identity, directory, retain, receipt) in selected {
                if Instant::now() >= deadline {
                    return Err(PATH);
                }
                drop(handle);
                if !retain {
                    workspaces[&slot.generation].remove_released_slot(slot, identity, directory)?;
                    if removed.len() >= 128 {
                        removed.clear();
                    }
                    removed.insert(receipt);
                }
            }
            Ok(())
        };
        run().is_ok()
    }
}

impl NativeCopy<'_> {
    fn leaf(&self, side: snapshot::Side, index: usize) -> Result<&HeldSlot, snapshot::Refusal> {
        match side {
            snapshot::Side::Active => Ok(&self.active[index]),
            snapshot::Side::Source => Ok(&self.source.as_ref().unwrap_or(&self.active)[index]),
            snapshot::Side::Staging => self.written[index]
                .as_ref()
                .or_else(|| self.staging[index].as_ref().map(SlotWriter::held))
                .ok_or(snapshot::Refusal::Custody),
        }
    }
}

impl snapshot::CopyCustody for NativeCopy<'_> {
    fn checkpoint(
        &mut self,
        _: &snapshot::CopyPlan,
        worker: &PendingOperation,
        budget: &snapshot::CopyBudget<'_>,
    ) -> Result<(), snapshot::Refusal> {
        budget.check()?;
        if self
            .last_check
            .is_some_and(|last| last.elapsed() < Duration::from_secs(1))
        {
            return Ok(());
        }
        self.native
            .refresh_epoch(&self.supervisor.controller)
            .map_err(|_| snapshot::Refusal::Custody)?;
        let (captured, host) = self
            .native
            .qualification
            .observe_host(MEMORY)
            .map_err(|_| snapshot::Refusal::Custody)?;
        self.supervisor
            .controller
            .snapshot_checkpoint(worker, self.work, host, RESOURCES)
            .map_err(|_| snapshot::Refusal::Custody)?;
        self.last_check = Some(captured);
        Ok(())
    }

    fn observe(
        &mut self,
        side: snapshot::Side,
        budget: &snapshot::CopyBudget<'_>,
    ) -> Result<[snapshot::Observation; 2], snapshot::Refusal> {
        budget.check()?;
        let read = |index| {
            self.leaf(side, index)?
                .observation()
                .map_err(|_| snapshot::Refusal::Identity)
        };
        Ok([read(0)?, read(1)?])
    }

    fn read(
        &mut self,
        side: snapshot::Side,
        role: snapshot::FileRole,
        offset: u64,
        bytes: &mut [u8],
        budget: &snapshot::CopyBudget<'_>,
    ) -> Result<(), snapshot::Refusal> {
        budget.check()?;
        self.leaf(side, usize::from(role == snapshot::FileRole::Vars))?
            .read(offset, bytes)
            .map_err(|_| snapshot::Refusal::Io)
    }

    fn write_staging(
        &mut self,
        role: snapshot::FileRole,
        offset: u64,
        bytes: &[u8],
        budget: &snapshot::CopyBudget<'_>,
    ) -> Result<(), snapshot::Refusal> {
        budget.check()?;
        self.staging[usize::from(role == snapshot::FileRole::Vars)]
            .as_mut()
            .ok_or(snapshot::Refusal::Custody)?
            .write(offset, bytes)
            .map_err(|_| snapshot::Refusal::Io)
    }

    fn golden_header(
        &mut self,
        golden: &snapshot::Golden,
        bytes: &mut [u8],
        budget: &snapshot::CopyBudget<'_>,
    ) -> Result<(), snapshot::Refusal> {
        budget.check()?;
        if self
            .golden
            .observation()
            .map_err(|_| snapshot::Refusal::Identity)?
            .identity
            != golden.identity
        {
            return Err(snapshot::Refusal::Identity);
        }
        self.golden
            .read(0, bytes)
            .map_err(|_| snapshot::Refusal::Io)
    }

    fn inspect(
        &mut self,
        info: &crate::vm::qemu_args::SnapshotInfo<'_>,
        output: &mut [u8],
        budget: &snapshot::CopyBudget<'_>,
    ) -> Result<usize, snapshot::Refusal> {
        budget.check()?;
        let fail = |_| snapshot::Refusal::Inspection;
        let dest = self.work.candidate.ok_or(snapshot::Refusal::Custody)?.disk;
        let (workspace, slot) = if self.destination.slot_path(dest).map_err(fail)? == *info.path() {
            (self.destination, dest)
        } else {
            (self.source_ws, self.work.source.0.disk)
        };
        if workspace.slot_path(slot).map_err(fail)? != *info.path() {
            return Err(snapshot::Refusal::Inspection);
        }
        let id = native_identity(info.disk()).map_err(fail)?;
        let held = workspace.open_slot(slot, id, false).map_err(fail)?;
        let offline = held.prove_offline(workspace).map_err(fail)?;
        let image = self.native.qualification.helper_image().map_err(fail)?;
        let cwd = self.native.qualification.command_cwd().map_err(fail)?;
        let plan = crate::vm::qemu_img::offline_snapshot_info(&offline, &image, info)
            .map_err(|_| snapshot::Refusal::Inspection)?;
        let inputs = runtime::test_issuer::effect_identities(&plan).map_err(fail)?;
        let refresh = || self.native.refresh_epoch(&self.supervisor.controller);
        let intent = self
            .native
            .intent(self.supervisor, self.owner, slot.generation, &refresh);
        let launch = self
            .native
            .launch(&intent, workspace, plan, &cwd, None, None, None, None, &[])
            .map_err(fail)?;
        let result = self.executor.block_on(
            launch.run_helper(
                &self.supervisor.cancel,
                tokio::time::Instant::from_std(budget.deadline())
                    .min(tokio::time::Instant::now() + Duration::from_secs(30)),
                refresh,
                || true,
            ),
        );
        let proof = result.released.ok_or(snapshot::Refusal::Inspection)?;
        if result.disposition != StopDisposition::CleanRelease
            || !proof.matches_helper(workspace.binding(), PlanKind::InspectWritable, &inputs)
        {
            return Err(snapshot::Refusal::Inspection);
        }
        let bytes = result.output.map_err(fail)?;
        output
            .get_mut(..bytes.len())
            .ok_or(snapshot::Refusal::Inspection)?
            .copy_from_slice(&bytes);
        drop(proof);
        self.exclusions.push(offline);
        Ok(bytes.len())
    }

    fn flush_staging(
        &mut self,
        budget: &snapshot::CopyBudget<'_>,
    ) -> Result<(), snapshot::Refusal> {
        for (index, writer) in self.staging.iter_mut().enumerate() {
            budget.check()?;
            self.written[index] = Some(
                writer
                    .take()
                    .ok_or(snapshot::Refusal::Custody)?
                    .flush()
                    .map_err(|_| snapshot::Refusal::Io)?,
            );
        }
        Ok(())
    }
}
