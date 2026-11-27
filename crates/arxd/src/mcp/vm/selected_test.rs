//! Install the reviewed TEST adapters under one foreground selection.

use super::*;
use crate::store::vm::native::recovery;
use crate::vm::{
    qualification::{ForegroundQualification, NativeRecovery, TEST_RESOURCES},
    reaper::{CleanupKind, CleanupWork},
    runtime::{Quiesced, ReaperRuntime, StopDisposition},
    test_issuer::{Installation, TestIssuer},
};

const GIB: u64 = 1 << 30;

#[cfg(not(test))]
pub(super) type Source = ForegroundQualification;

#[cfg(test)]
pub(super) enum Source {
    Native(Box<ForegroundQualification>),
    Fixture(Box<crate::vm::qualification::FixtureQualification>),
}

#[cfg(test)]
impl Source {
    fn measurements(&self) -> &dyn crate::vm::test_issuer::SelectedSource {
        match self {
            Self::Native(source) => &**source,
            Self::Fixture(source) => &**source,
        }
    }
}

#[cfg(test)]
impl crate::vm::test_issuer::SelectedSource for Source {
    fn seal_sha256(&self) -> qualification_record::Hash {
        self.measurements().seal_sha256()
    }
    fn class(&self) -> qualification_record::Class {
        self.measurements().class()
    }
    fn scope(&self) -> qualification_record::Hash {
        self.measurements().scope()
    }
    fn recipes(&self) -> qualification_record::TestRecipes {
        self.measurements().recipes()
    }
    fn owner_matches(&self, configured: &str) -> bool {
        self.measurements().owner_matches(configured)
    }
    fn sample(
        &self,
        advance: &dyn Fn() -> Result<ClockReading, crate::vm::selection_epoch::Invalidation>,
    ) -> Result<crate::vm::selection_epoch::Sample, crate::vm::selection_epoch::Invalidation> {
        self.measurements().sample(advance)
    }
}

pub(super) type SelectedTest = TestIssuer<'static, Source, ForegroundClock>;

#[cfg(test)]
mod journey;

impl Supervisor {
    pub(super) fn install_test(&mut self) {
        self.install_test_with(|controller, config| {
            let selected = TestIssuer::open_configured(controller, config)?;
            #[cfg(test)]
            let selected = selected.map_source(|source| Source::Native(Box::new(source)));
            Ok(selected)
        });
    }

    fn install_test_with(
        &mut self,
        open: impl FnOnce(
            Arc<Controller>,
            &crate::config::TestQualificationSelection,
        ) -> Result<SelectedTest, NativeRefusal>,
    ) {
        let Some(config) = self
            .runtime
            .as_ref()
            .and_then(VmRuntimeProfile::test_qualification)
        else {
            return;
        };
        match open(self.controller.clone(), &config) {
            Ok(selected) => self.install_selected(selected),
            Err(reason) => self.native_refusal = reason,
        }
    }

    fn install_selected(&mut self, selected: SelectedTest) {
        let selected = Arc::new(selected);
        let recipes = selected.recipes();
        self.recipes[0] = Some((recipes.ubuntu.recipe_id, Some(recipes.ubuntu.recipe_sha256)));
        self.recipes[2] = Some((
            recipes.windows.recipe_id,
            Some(recipes.windows.recipe_sha256),
        ));
        let host = selected.clone();
        let _ = self.native_scripts.set(native_run::NativeScripts {
            installation: Installation::selected(&selected),
            resources: TEST_RESOURCES,
            host: Arc::new(move || {
                host.live_host().unwrap_or(HostObservation {
                    owner_idle: None,
                    logical_cpus: None,
                    available_ram: None,
                    capacity: None,
                })
            }),
        });
        self.selected_test = Some(selected);
    }

    pub(super) fn test_cleanup(&self) -> Option<TestCleanup<'_>> {
        Some(TestCleanup {
            supervisor: self,
            custody: NativeRecovery::open(
                self.controller.native_store(),
                self.custody.as_ref()?.configured_state(),
                &self.test_generations,
            )
            .ok()?,
        })
    }
}

impl SelectedTest {
    fn live_host(&self) -> Result<HostObservation, NativeRefusal> {
        self.refresh()?;
        #[cfg(test)]
        if let Source::Fixture(source) = self.source() {
            return source.test_host();
        }
        self.native_source().test_host()
    }

    fn native_source(&self) -> &ForegroundQualification {
        #[cfg(not(test))]
        return self.source();
        #[cfg(test)]
        match self.source() {
            Source::Native(source) => source,
            Source::Fixture(_) => unreachable!("fixture handled at worker entry"),
        }
    }

    fn confirm_golden(
        &self,
        ctl: &Controller,
        golden: &SealedGolden,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), NativeRefusal> {
        #[cfg(test)]
        if let Source::Fixture(source) = self.source() {
            return source.confirm_golden(ctl, golden, cancelled);
        }
        self.native_source().confirm_golden(ctl, golden, cancelled)
    }

    /// Every refusal before a durable commit is filed through the caller's
    /// audited refusal, except those `admit_build` and `extend_lease` file
    /// in their own transaction. After a commit, a failure before the worker
    /// owns the intent settles that intent as cancelled.
    pub(super) fn dispatch(
        self: &Arc<Self>,
        caller: &VmInventory,
        gate: &mut Gate,
        action: i64,
        request: Mutation,
    ) -> Outcome<Value> {
        if !matches!(
            request.work,
            Work::Build | Work::Create(_) | Work::Start | Work::Extend(_) | Work::Snapshot(_)
        ) {
            return caller.refuse(
                action,
                request.vm(),
                Fault(native_reason(NativeRefusal::QualificationMissing)),
            );
        }
        let subject = request.vm();
        let refuse = |reason: Fault| caller.refuse::<Value>(action, subject, reason);
        let native = |reason: NativeRefusal| refuse(Fault(native_reason(reason)));
        let live_host = || {
            self.live_host()
                .or_else(|reason| caller.refuse(action, subject, Fault(native_reason(reason))))
        };
        let supervisor = &caller.supervisor;
        let ctl = &supervisor.controller;
        let principal = caller.principal.as_str();
        let Mutation { key, id, work } = request;
        gate.test_workers_open(supervisor)?;
        let recipes = self.recipes();
        // A caller-owned Static, unselected or historic VM already passed
        // the ownership precheck; only the selected runtime is missing.
        if matches!(work, Work::Start | Work::Extend(_) | Work::Snapshot(_))
            && !ctl.selected_test_vm(id, recipes)?
        {
            return native(NativeRefusal::QualificationMissing);
        }
        let sizes = [(recipes.ubuntu, 24 * GIB), (recipes.windows, 40 * GIB)];
        let vm_request = |operation, disk| Request {
            principal,
            vm: id,
            request_id: key,
            operation,
            disk,
        };
        let cancelled = || supervisor.cancel.is_cancelled();
        match work {
            Work::Start => {
                let host = live_host()?;
                gate.start_native_run(
                    supervisor,
                    &Installation::selected(self),
                    vm_request(Operation::Start, DiskClaim::default()),
                    host,
                    TEST_RESOURCES,
                    move |supervisor, runtime, owner, start| {
                        let selected = installed(supervisor)?;
                        #[cfg(test)]
                        if let Source::Fixture(source) = selected.source() {
                            return runtime.block_on(journey::run(
                                source, supervisor, owner, start, selected,
                            ));
                        }
                        runtime.block_on(owner.run(
                            supervisor,
                            start,
                            selected,
                            selected.native_source(),
                        ))
                    },
                )
                // The start's own admission files nothing; a closed
                // supervisor is not a refusal of this request.
                .or_else(|reason| {
                    if reason.0 == SHUTDOWN.0 {
                        Err(reason)
                    } else {
                        refuse(reason)
                    }
                })
            }
            Work::Extend(ttl) => {
                // A lease extension changes no host resource or artifact, so
                // only the selection epoch must be current, not idleness.
                if let Err(reason) = self.refresh() {
                    return native(reason);
                }
                let end = ctl.extend_lease(principal, id, key, ttl)??;
                let replay = ctl
                    .foreground_replay(principal, key, Subject::Vm(id), action)??
                    .ok_or(STATE_UNAVAILABLE)?;
                Ok(receipt(replay.operation.id, Some(end)))
            }
            Work::Build => {
                let Some((recipe, size)) =
                    sizes.into_iter().find(|(recipe, _)| recipe.recipe_id == id)
                else {
                    return refuse(Refusal::RecipeUnqualified.into());
                };
                let name = hex::encode(id);
                let retained = DiskClaim {
                    images: size + GIB,
                    ..DiskClaim::default()
                };
                let transient = DiskClaim {
                    images: size + GIB,
                    staging: if id == recipes.windows.recipe_id {
                        8 * GIB
                    } else {
                        GIB
                    },
                    metadata: 4096,
                };
                let spec = BuildRequest {
                    principal,
                    key,
                    recipe: &name,
                    digest: recipe.recipe_sha256,
                    retained,
                    transient,
                };
                // Live idleness first, then only a bounded identity check
                // of a reusable sealed pair under the gate. Every clone and
                // start re-hashes the held golden before any write.
                let host = live_host()?;
                let held = ctl
                    .selected_golden(recipe)?
                    .filter(|golden| self.confirm_golden(ctl, golden, &cancelled).is_ok());
                let (slots, worker, reference) =
                    match ctl.admit_build(spec, host, TEST_RESOURCES, |template, pair| {
                        held.filter(|golden| {
                            golden.template() == template && golden.slots() == pair
                        })
                        .map(|golden| golden.evidence())
                    })?? {
                        BuildAdmission::Existing(reference) => {
                            return Ok(receipt(reference.id, None));
                        }
                        BuildAdmission::Fresh(slots, worker, reference) => {
                            (*slots, worker, reference)
                        }
                    };
                let registration = match supervisor.test_staging.register(slots.id, ()) {
                    Ok(registration) => registration,
                    Err(reason) => {
                        let _ = ctl.fail_build_with(&worker, slots, true, None);
                        return Err(reason);
                    }
                };
                let principal = principal.to_owned();
                gate.spawn_test_worker(supervisor, registration, move |supervisor, runtime| {
                    let selected = installed(supervisor)?;
                    let ctl = &supervisor.controller;
                    let request = BuildRequest {
                        principal: &principal,
                        key,
                        recipe: &name,
                        digest: recipe.recipe_sha256,
                        retained,
                        transient,
                    };
                    #[cfg(test)]
                    if let Source::Fixture(source) = selected.source() {
                        let result = runtime.block_on(source.build_test_golden(
                            ctl,
                            selected,
                            &worker,
                            slots,
                            request,
                            &supervisor.test_generations,
                            &supervisor.cancel,
                        ));
                        return settled(ctl, &principal, reference.id, result.map(|_| ()));
                    }
                    let result = runtime.block_on(selected.native_source().build_test_golden(
                        ctl,
                        selected,
                        &worker,
                        slots,
                        request,
                        &supervisor.test_generations,
                        &supervisor.cancel,
                    ));
                    settled(ctl, &principal, reference.id, result.map(|_| ()))
                });
                Ok(receipt(reference.id, None))
            }
            Work::Create(ttl) => {
                let mut golden = None;
                for (recipe, size) in sizes {
                    if let Some(found) = ctl
                        .selected_golden(recipe)?
                        .filter(|found| found.template() == id)
                    {
                        golden = Some((found, size));
                        break;
                    }
                }
                let Some((golden, size)) = golden else {
                    return refuse(NOT_FOUND);
                };
                // The ledger pair and its bound workspace are checked by
                // the lookup above. The clone worker holds and hashes the
                // golden, bounded and cancellable, before any clone write.
                let host = live_host()?;
                let disk = DiskClaim {
                    images: size + GIB,
                    metadata: 4096,
                    staging: 0,
                };
                let operation = Operation::Clone {
                    template: id,
                    lease_seconds: Some(ttl),
                };
                let (vm, slots, worker, reference) = match ctl.admit_linked_clone(
                    vm_request(operation, disk),
                    host,
                    TEST_RESOURCES,
                    golden.slots(),
                    golden.evidence(),
                )? {
                    Err(reason) => return refuse(reason.into()),
                    Ok(CloneAdmission::Existing(reference)) => {
                        let lease = ctl
                            .operation_status(principal, reference.id)??
                            .expires_at_ms;
                        return Ok(receipt(reference.id, lease));
                    }
                    Ok(CloneAdmission::Fresh(vm, slots, worker, reference)) => {
                        (vm, slots, worker, reference)
                    }
                };
                let handoff = (|| {
                    let lease = ctl
                        .get_status(principal, vm)??
                        .expires_at_ms
                        .ok_or(STATE_UNAVAILABLE)?;
                    Ok::<_, Fault>((supervisor.test_staging.register(vm, ())?, lease))
                })();
                let (registration, lease) = match handoff {
                    Ok(handoff) => handoff,
                    Err(reason) => {
                        // Untouched: nothing was staged for this clone.
                        let _ = ctl.finish_native_clone(
                            &worker,
                            slots,
                            Err(FailureReason::Cancelled),
                            ClonePublication {
                                golden: None,
                                effect_cutoff: ctl.native_clock().monotonic,
                                cancelled: &|| true,
                            },
                        );
                        return Err(reason);
                    }
                };
                let cutoff = lease_cutoff(ctl, lease);
                let principal = principal.to_owned();
                gate.spawn_test_worker(supervisor, registration, move |supervisor, runtime| {
                    let selected = installed(supervisor)?;
                    let ctl = &supervisor.controller;
                    #[cfg(test)]
                    if let Source::Fixture(source) = selected.source() {
                        let result = runtime.block_on(source.clone_test(
                            ctl,
                            selected,
                            worker,
                            slots,
                            cutoff,
                            &supervisor.cancel,
                        ));
                        return settled(ctl, &principal, reference.id, result);
                    }
                    let result = runtime.block_on(selected.native_source().clone_test(
                        ctl,
                        selected,
                        worker,
                        slots,
                        size,
                        cutoff,
                        &supervisor.cancel,
                    ));
                    settled(ctl, &principal, reference.id, result)
                });
                Ok(receipt(reference.id, Some(lease)))
            }
            Work::Snapshot(request) => {
                // Everything fallible precedes the durable admission. The
                // inspector needs the live foreground runtime's drivers.
                let Ok(runtime) = tokio::runtime::Handle::try_current() else {
                    return refuse(STATE_UNAVAILABLE);
                };
                let registration = match supervisor.test_staging.register(id, ()) {
                    Ok(registration) => registration,
                    Err(reason) => return refuse(reason),
                };
                let lease = match ctl.get_status(principal, id)? {
                    Ok(status) => status.expires_at_ms.ok_or(STATE_UNAVAILABLE)?,
                    Err(reason) => return refuse(reason.into()),
                };
                let cutoff = lease_cutoff(ctl, lease);
                let host = live_host()?;
                let (work, worker, reference) = match ctl.admit_snapshot(
                    principal,
                    key,
                    id,
                    action,
                    &request,
                    None,
                    host,
                    TEST_RESOURCES,
                )? {
                    Ok(admitted) => admitted,
                    Err(reason) => return refuse(reason.into()),
                };
                if work.candidate.is_some() {
                    let principal = principal.to_owned();
                    gate.spawn_test_snapshot(
                        supervisor,
                        registration,
                        runtime,
                        move |supervisor, runtime| {
                            let selected = installed(supervisor)?;
                            let ctl = &supervisor.controller;
                            #[cfg(test)]
                            if let Source::Fixture(source) = selected.source() {
                                let result = source.snapshot_test(
                                    ctl,
                                    selected,
                                    worker,
                                    work,
                                    cutoff,
                                    &supervisor.cancel,
                                    runtime,
                                );
                                return settled(ctl, &principal, reference.id, result);
                            }
                            let result = selected.native_source().snapshot_test(
                                ctl,
                                selected,
                                worker,
                                work,
                                cutoff,
                                &supervisor.cancel,
                                runtime,
                            );
                            settled(ctl, &principal, reference.id, result)
                        },
                    );
                }
                Ok(receipt(reference.id, None))
            }
            _ => Err(INVALID),
        }
    }
}

impl Gate {
    /// The tracked worker of a build or clone this gate hold admitted.
    fn spawn_test_worker(
        &mut self,
        supervisor: &Arc<Supervisor>,
        registration: owned_worker::Registration<()>,
        work: impl FnOnce(&Supervisor, &tokio::runtime::Runtime) -> anyhow::Result<()> + Send + 'static,
    ) {
        self.spawn_test_blocking(
            supervisor,
            registration,
            |_| {},
            move |supervisor| work(supervisor, &worker_runtime()?),
        );
    }

    /// The inspector uses Handle::block_on to run the checked helper. Use
    /// the live foreground runtime's I/O and timer drivers, and execute the
    /// copy itself on the tracked blocking pool.
    fn spawn_test_snapshot(
        &mut self,
        supervisor: &Arc<Supervisor>,
        registration: owned_worker::Registration<()>,
        runtime: tokio::runtime::Handle,
        copy: impl FnOnce(&Supervisor, &tokio::runtime::Handle) -> anyhow::Result<()> + Send + 'static,
    ) {
        self.spawn_test_blocking(
            supervisor,
            registration,
            |_| {},
            move |supervisor| copy(supervisor, &runtime),
        );
    }
}

fn installed(supervisor: &Supervisor) -> anyhow::Result<&SelectedTest> {
    supervisor
        .selected_test
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("native TEST runtime unavailable"))
}

fn lease_cutoff(ctl: &Controller, expires: i64) -> Instant {
    let now = ctl.native_clock();
    let remaining = u64::try_from(expires.saturating_sub(now.utc_ms)).unwrap_or(0);
    now.monotonic
        .checked_add(Duration::from_millis(remaining))
        .unwrap_or(now.monotonic)
}

fn worker_runtime() -> std::io::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
}

fn settled(
    ctl: &Controller,
    principal: &str,
    id: Id,
    result: anyhow::Result<()>,
) -> anyhow::Result<()> {
    match result {
        Err(_)
            if ctl
                .operation_status(principal, id)?
                .is_ok_and(|status| status.state != Status::Pending) =>
        {
            Ok(())
        }
        result => result,
    }
}

pub(super) struct TestCleanup<'a> {
    supervisor: &'a Supervisor,
    pub(super) custody: NativeRecovery<'a>,
}

impl ReaperRuntime for TestCleanup<'_> {
    fn quiesce(&self, work: &CleanupWork, join_by: Instant) -> Quiesced {
        let supervisor = self.supervisor;
        let vm = work.kind.subject();
        // Expiry and recovery end a live owner and give its release one
        // bounded join. Other cleanup never interrupts a live run.
        let ending = matches!(
            work.kind,
            CleanupKind::Expiry { .. } | CleanupKind::Recovery { .. }
        );
        if !work.accounted && !work.release_only
            || !matches!(
                supervisor
                    .observe_test_worker(&supervisor.test_staging, vm, |owner| owner.is_none()),
                Ok(true)
            )
            || ending && !supervisor.end_native_run(vm, join_by)
            || !supervisor.reap_native_run(vm, &mut || true)
        {
            return Quiesced::Unconfirmed;
        }
        match supervisor
            .controller
            .native_store()
            .read_only(|db| recovery::requires_release(db, work.kind))
        {
            Ok(true) => Quiesced::Orphaned,
            Ok(false) => Quiesced::Offline,
            Err(_) => Quiesced::Unconfirmed,
        }
    }

    fn with_orphan_fence(&self, work: &CleanupWork, recover: &mut dyn FnMut() -> bool) -> bool {
        let supervisor = self.supervisor;
        supervisor.with_test_orphan_fence(
            &supervisor.test_staging,
            work.kind.subject(),
            &mut || supervisor.reap_native_run(work.kind.subject(), recover),
        )
    }

    fn graceful_stop(&self, _: &CleanupWork, _: Instant) -> StopDisposition {
        StopDisposition::ChargedQuarantine
    }

    fn force_reap(&self, _: &CleanupWork, _: Instant) -> StopDisposition {
        StopDisposition::ChargedQuarantine
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_and_unverified_opt_in_refuse_foreground_admission_without_charge() {
        use crate::mcp::vm::tests::{RECIPE, UTC, key, request};
        use std::sync::atomic::AtomicI64;

        for enabled in [false, true] {
            let root = crate::service::application::tests::private_temp();
            let path = |leaf| root.path().join(leaf).to_str().unwrap().replace('\\', "/");
            let state = root.path().to_str().unwrap().replace('\\', "/");
            let volume = &state[..3];
            let mut raw = json!({
                "enabled":true,
                "owner_sid":"S-1-12-1-1-2-3-4",
                "supervisor_sid":"S-1-12-1-1-2-3-4",
                "bundle":"E:/fixture-bundle","volume":volume,"state":state,
                "disk_ceiling_bytes":10 * GIB,"manifest":"qemu-11.1.3-windows-x64",
                "execution_profile":"current_user"
            });
            if enabled {
                raw["test_qualification"] = json!({
                    "enrollment":path("missing.json"),"dossier":path("dossier"),
                    "sha256":hex::encode([1; 32]),"scope":hex::encode([2; 32]),
                });
            }
            let runtime = VmRuntimeProfile::parse(&serde_json::to_vec(&raw).unwrap()).unwrap();
            let mut supervisor = Supervisor::new(
                root.path(),
                VmPolicy::default(),
                ForegroundClock {
                    utc: Some(Arc::new(AtomicI64::new(UTC))),
                },
                Some(RECIPE),
                NativeRefusal::QualificationMissing,
                Some(runtime),
            )
            .unwrap();
            Arc::get_mut(&mut supervisor).unwrap().install_test();

            let connection = supervisor.connection("alice".into());
            let inventory = connection.inventory();
            let catalog = inventory
                .dispatch(request("vm_catalog", json!({})), || false)
                .unwrap();
            assert_eq!(catalog["capability"]["available"], false);
            assert!(
                inventory
                    .dispatch(
                        request(
                            "vm_template_build",
                            json!({
                                "recipe_id":hex::encode(RECIPE.0),"request_id":key(1),
                            })
                        ),
                        || false
                    )
                    .is_err()
            );
            let store = supervisor.controller.native_store();
            store
                .read_only(|db| {
                    for table in ["vm_operations", "vm_disk_claims", "vm_generations"] {
                        assert_eq!(
                            crate::store::vm::count(
                                db,
                                &format!("SELECT count(*) FROM {table}"),
                                []
                            )?,
                            0
                        );
                    }
                    Ok(())
                })
                .unwrap();
            assert!(
                std::fs::read_dir(root.path())
                    .unwrap()
                    .all(|entry| { entry.unwrap().file_type().unwrap().is_file() })
            );
        }
    }

    #[tokio::test]
    async fn snapshot_worker_drives_inspection_on_active_foreground_runtime() {
        let root = crate::service::application::tests::private_temp();
        let supervisor = Supervisor::new(
            root.path(),
            VmPolicy::default(),
            ForegroundClock { utc: None },
            None,
            NativeRefusal::QualificationMissing,
            None,
        )
        .unwrap();
        let (inspected, confirmed) = tokio::sync::oneshot::channel();
        let registration = supervisor.test_staging.register([1; 16], ()).unwrap();
        supervisor.lock().unwrap().spawn_test_snapshot(
            &supervisor,
            registration,
            tokio::runtime::Handle::try_current().unwrap(),
            move |_, runtime| {
                // Match CheckedSnapshotInspector's call on a live Tokio
                // handle, including an operation that needs its timer driver.
                runtime.block_on(async { tokio::time::sleep(Duration::from_millis(1)).await });
                inspected.send(()).unwrap();
                Ok(())
            },
        );

        tokio::time::timeout(Duration::from_secs(5), confirmed)
            .await
            .unwrap()
            .unwrap();
        supervisor.close();
        supervisor
            .drain_workers(Duration::from_secs(5))
            .await
            .unwrap();
    }
}
