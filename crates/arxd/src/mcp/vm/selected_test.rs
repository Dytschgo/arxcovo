//! Install the reviewed TEST adapters under one foreground selection.

use super::*;
use crate::store::vm::native::recovery;
use crate::vm::{
    qualification::{ForegroundQualification, NativeRecovery, TEST_RESOURCES},
    reaper::CleanupWork,
    runtime::{Quiesced, ReaperRuntime, StopDisposition},
    test_issuer::{Installation, TestIssuer},
};

const GIB: u64 = 1 << 30;

pub(super) type SelectedTest = TestIssuer<'static, ForegroundQualification, ForegroundClock>;

impl Supervisor {
    pub(super) fn install_test(&mut self) {
        let Some(config) = self
            .runtime
            .as_ref()
            .and_then(VmRuntimeProfile::test_qualification)
        else {
            return;
        };
        let selected = match SelectedTest::open_configured(self.controller.clone(), &config) {
            Ok(selected) => Arc::new(selected),
            Err(reason) => {
                self.native_refusal = reason;
                return;
            }
        };
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
        self.source().test_host()
    }

    pub(super) fn dispatch(
        self: &Arc<Self>,
        caller: &VmInventory,
        gate: &mut Gate,
        action: i64,
        request: Mutation,
    ) -> Outcome<Value> {
        if !matches!(
            request.work,
            Work::Build | Work::Create(_) | Work::Start | Work::Snapshot(_)
        ) {
            return caller.refuse(
                action,
                request.vm(),
                Fault(native_reason(NativeRefusal::QualificationMissing)),
            );
        }
        let subject = request.vm();
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
        if matches!(work, Work::Start | Work::Snapshot(_))
            && !ctl.selected_test_generation(id, recipes)?
        {
            return caller.refuse(action, subject, Refusal::NotFound.into());
        }
        let sizes = [(recipes.ubuntu, 24 * GIB), (recipes.windows, 40 * GIB)];
        let vm_request = |operation, disk| Request {
            principal,
            vm: id,
            request_id: key,
            operation,
            disk,
        };
        match work {
            Work::Start => gate.start_native_run(
                supervisor,
                &Installation::selected(self),
                vm_request(Operation::Start, DiskClaim::default()),
                live_host()?,
                TEST_RESOURCES,
                move |supervisor, runtime, owner, start| {
                    let selected = installed(supervisor)?;
                    runtime.block_on(owner.run(supervisor, start, selected, selected.source()))
                },
            ),
            Work::Build => {
                let (recipe, size) = sizes
                    .into_iter()
                    .find(|(recipe, _)| recipe.recipe_id == id)
                    .ok_or(Refusal::RecipeUnqualified)?;
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
                let held = ctl
                    .selected_golden(recipe)?
                    .filter(|golden| self.source().verify_golden(ctl, golden).is_ok());
                let (slots, worker, reference) = match ctl.admit_build(
                    spec,
                    live_host()?,
                    TEST_RESOURCES,
                    |template, pair| {
                        held.filter(|golden| {
                            golden.template() == template && golden.slots() == pair
                        })
                        .map(|golden| golden.evidence())
                    },
                )?? {
                    BuildAdmission::Existing(reference) => {
                        return Ok(receipt(reference.id, None));
                    }
                    BuildAdmission::Fresh(slots, worker, reference) => (*slots, worker, reference),
                };
                let registration = supervisor.test_staging.register(slots.id, ())?;
                let principal = principal.to_owned();
                gate.enqueue_test_worker(
                    supervisor,
                    registration,
                    worker_runtime,
                    move |supervisor, runtime| {
                        let selected = installed(supervisor)?;
                        let ctl = &supervisor.controller;
                        let result = runtime.block_on(selected.source().build_test_golden(
                            ctl,
                            selected,
                            &worker,
                            slots,
                            BuildRequest {
                                principal: &principal,
                                key,
                                recipe: &name,
                                digest: recipe.recipe_sha256,
                                retained,
                                transient,
                            },
                            &supervisor.test_generations,
                            &supervisor.cancel,
                        ));
                        settled(ctl, &principal, reference.id, result.map(|_| ()))
                    },
                )?;
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
                let (golden, size) = golden.ok_or(NOT_FOUND)?;
                self.source()
                    .verify_golden(ctl, &golden)
                    .map_err(|reason| Fault(native_reason(reason)))?;
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
                    live_host()?,
                    TEST_RESOURCES,
                    golden.slots(),
                    golden.evidence(),
                )?? {
                    CloneAdmission::Existing(reference) => {
                        let lease = ctl
                            .operation_status(principal, reference.id)??
                            .expires_at_ms;
                        return Ok(receipt(reference.id, lease));
                    }
                    CloneAdmission::Fresh(vm, slots, worker, reference) => {
                        (vm, slots, worker, reference)
                    }
                };
                let registration = supervisor.test_staging.register(vm, ())?;
                let lease = ctl
                    .get_status(principal, vm)??
                    .expires_at_ms
                    .ok_or(STATE_UNAVAILABLE)?;
                let cutoff = lease_cutoff(ctl, lease);
                gate.enqueue_test_worker(
                    supervisor,
                    registration,
                    worker_runtime,
                    move |supervisor, runtime| {
                        let selected = installed(supervisor)?;
                        runtime.block_on(selected.source().clone_test(
                            &supervisor.controller,
                            selected,
                            worker,
                            slots,
                            size,
                            cutoff,
                            &supervisor.cancel,
                        ))
                    },
                )?;
                Ok(receipt(reference.id, Some(lease)))
            }
            Work::Snapshot(request) => {
                let registration = supervisor.test_staging.register(id, ())?;
                let status = ctl.get_status(principal, id)??;
                let cutoff = lease_cutoff(ctl, status.expires_at_ms.ok_or(STATE_UNAVAILABLE)?);
                let (work, worker, reference) = ctl.admit_snapshot(
                    principal,
                    key,
                    id,
                    action,
                    &request,
                    None,
                    live_host()?,
                    TEST_RESOURCES,
                )??;
                if work.candidate.is_some() {
                    let principal = principal.to_owned();
                    gate.enqueue_test_snapshot(
                        supervisor,
                        registration,
                        move |supervisor, runtime| {
                            let selected = installed(supervisor)?;
                            let ctl = &supervisor.controller;
                            let result = selected.source().snapshot_test(
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
                    )?;
                }
                Ok(receipt(reference.id, None))
            }
            _ => Err(INVALID),
        }
    }
}

impl Gate {
    fn enqueue_test_snapshot(
        &mut self,
        supervisor: &Arc<Supervisor>,
        registration: owned_worker::Registration<()>,
        copy: impl FnOnce(&Supervisor, &tokio::runtime::Handle) -> anyhow::Result<()> + Send + 'static,
    ) -> Outcome<()> {
        // The inspector uses Handle::block_on to run the checked helper.
        // Use the live foreground runtime's I/O and timer drivers, and
        // execute the copy itself on the tracked blocking pool.
        let runtime = tokio::runtime::Handle::current();
        self.enqueue_test_blocking(
            supervisor,
            registration,
            |_| {},
            move |supervisor| copy(supervisor, &runtime),
        )?;
        Ok(())
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
    fn quiesce(&self, work: &CleanupWork, _: Instant) -> Quiesced {
        let supervisor = self.supervisor;
        if !work.accounted && !work.release_only
            || !matches!(
                supervisor.observe_test_worker(
                    &supervisor.test_staging,
                    work.kind.subject(),
                    |owner| owner.is_none()
                ),
                Ok(true)
            )
            || !supervisor.reap_native_run(work.kind.subject(), &mut || true)
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
        supervisor
            .lock()
            .unwrap()
            .enqueue_test_snapshot(&supervisor, registration, move |_, runtime| {
                // Match CheckedSnapshotInspector's call on a live Tokio
                // handle, including an operation that needs its timer driver.
                runtime.block_on(async { tokio::time::sleep(Duration::from_millis(1)).await });
                inspected.send(()).unwrap();
                Ok(())
            })
            .unwrap();

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
