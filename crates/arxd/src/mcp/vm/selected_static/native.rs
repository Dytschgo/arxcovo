//! Admission and owned worker lifetime for the selected static recipe.
use super::*;
use crate::store::vm::Binding;
use crate::vm::{
    qualification::{StaticDossier, StaticMachine, StaticRecipe, TEST_RESOURCES},
    runtime::StopDisposition,
    static_web::FullCopyReservation,
};
use std::sync::OnceLock;
use tokio::{runtime::Builder, sync::mpsc};

const GIB: u64 = 1 << 30;
const COPY_CEILING: u64 = 34 * GIB;
const RETAINED: DiskClaim = DiskClaim {
    images: COPY_CEILING,
    metadata: 4096,
    staging: 0,
};
const TRANSIENT: DiskClaim = DiskClaim {
    images: COPY_CEILING,
    metadata: 4096,
    staging: 128 << 20,
};

pub(in crate::mcp::vm) struct Run {
    stop: mpsc::UnboundedSender<PendingOperation>,
    origin: OnceLock<Origin>,
}

impl Run {
    pub(in crate::mcp::vm) fn origin(&self) -> Option<&Origin> {
        self.origin
            .get()
            .filter(|origin| !origin.retired.is_retired())
    }
}

impl SelectedStatic {
    fn dossier(&self) -> Option<&StaticDossier> {
        match &self.0 {
            Source::Native(dossier) => Some(dossier),
            #[cfg(test)]
            Source::Fixture(_) => None,
        }
    }

    pub(in crate::mcp::vm) fn native_web_digest(&self) -> Option<Hash> {
        Some(StaticRecipe::Web.identity(self.dossier()?).recipe_sha256)
    }

    pub(in crate::mcp::vm) fn dispatch(
        &self,
        caller: &VmInventory,
        gate: &mut Gate,
        action: i64,
        request: Mutation,
    ) -> Outcome<Value> {
        let supervisor = &caller.supervisor;
        let controller = &supervisor.controller;
        let principal = caller.principal.as_str();
        let subject = request.vm();
        let refuse = |reason| caller.refuse(action, subject, reason);
        let m4 = supervisor.selected_test.as_ref().ok_or(STATE_UNAVAILABLE)?;
        let dossier = self.dossier().ok_or(STATE_UNAVAILABLE)?;
        let native = || StaticMachine {
            m4: m4.native_source(),
            dossier,
            cancel: &supervisor.cancel,
        };
        gate.test_workers_open(supervisor)?;
        let Mutation { key, id, work } = request;
        let vm_request = |operation| Request {
            principal,
            vm: id,
            request_id: key,
            operation,
            disk: DiskClaim::default(),
        };
        let unavailable = |reason| Fault::new(native_reason(reason));
        let live = || native().live().map(|(_, host)| host).map_err(unavailable);
        let goldens = || {
            [StaticRecipe::Web, StaticRecipe::Compose]
                .into_iter()
                .map(|recipe| controller.selected_golden(recipe.identity(dossier)))
        };
        match work {
            Work::Build => {
                let recipe = if id == WEB_RECIPE_ID {
                    StaticRecipe::Web
                } else {
                    StaticRecipe::Compose
                };
                let selected = recipe.identity(dossier);
                if id != selected.recipe_id {
                    return refuse(NOT_FOUND);
                }
                let host = live()?;
                let name = hex::encode(id);
                let spec = BuildRequest {
                    principal,
                    key,
                    recipe: &name,
                    digest: selected.recipe_sha256,
                    retained: RETAINED,
                    transient: TRANSIENT,
                };
                let golden = controller
                    .selected_golden(selected)?
                    .filter(|golden| native().check_golden(golden));
                let (slots, worker, reference) = match controller.admit_build(
                    spec,
                    host,
                    TEST_RESOURCES,
                    |template, slots| {
                        golden
                            .filter(|golden| {
                                golden.template() == template && golden.slots() == slots
                            })
                            .map(|golden| golden.evidence())
                    },
                )?? {
                    BuildAdmission::Existing(reference) => return Ok(receipt(reference.id, None)),
                    BuildAdmission::Fresh(slots, worker, reference) => (*slots, worker, reference),
                };
                let held = match supervisor.test_staging.register(slots.id, ()) {
                    Ok(held) => held,
                    Err(reason) => {
                        controller.fail_build(&worker, slots, true)?;
                        return Err(reason);
                    }
                };
                let principal = principal.to_owned();
                gate.spawn_test_blocking(
                    supervisor,
                    held,
                    |_| {},
                    move |supervisor| {
                        let machine = installed(supervisor)?;
                        let controller = &supervisor.controller;
                        let spec = BuildRequest {
                            principal: &principal,
                            recipe: &name,
                            key,
                            digest: selected.recipe_sha256,
                            retained: RETAINED,
                            transient: TRANSIENT,
                        };
                        let runtime = match worker_runtime() {
                            Ok(runtime) => runtime,
                            Err(_) => return controller.fail_build(&worker, slots, true),
                        };
                        runtime.block_on(machine.build(
                            controller,
                            &worker,
                            slots,
                            spec,
                            recipe,
                            &supervisor.test_generations,
                        ))
                    },
                );
                Ok(receipt(reference.id, None))
            }
            Work::StaticCreate(name) => {
                let host = live()?;
                let mut source = None;
                for candidate in goldens() {
                    if let Some(golden) = candidate?.filter(|golden| golden.template() == id) {
                        source = Some(golden);
                        break;
                    }
                }
                let Some(golden) = source else {
                    return refuse(NOT_FOUND);
                };
                let [disk, vars] = golden.evidence();
                let charge = FullCopyReservation {
                    qcow2_bytes: disk.pin.size,
                    qcow2_growth_bytes: COPY_CEILING.saturating_sub(disk.pin.size),
                    vars_bytes: vars.pin.size,
                    staging_bytes: 4096,
                    metadata_bytes: 4096,
                }
                .checked_claim()
                .ok_or(STATE_UNAVAILABLE)?;
                let binding = Binding {
                    principal,
                    key,
                    subject: static_subject(id, &name),
                    action,
                };
                let (vm, slots, worker, reference) = match controller.admit_static(
                    binding,
                    &name,
                    id,
                    (golden.slots(), golden.evidence()),
                    (
                        StaticRecipe::Web.identity(dossier).recipe_sha256,
                        Some(&dossier.evidence),
                    ),
                    charge,
                    host,
                    TEST_RESOURCES,
                )? {
                    Ok(admitted) => admitted,
                    Err(reason) => return refuse(reason.into()),
                };
                let held = match supervisor.test_staging.register(vm, ()) {
                    Ok(held) => held,
                    Err(reason) => {
                        controller.finish_static(
                            &worker,
                            slots,
                            Err(CopyFailure::untouched(FailureReason::Cancelled)),
                        )?;
                        return Err(reason);
                    }
                };
                gate.spawn_test_blocking(
                    supervisor,
                    held,
                    |_| {},
                    move |supervisor| {
                        installed(supervisor)?.copy(&supervisor.controller, &worker, slots, &golden)
                    },
                );
                Ok(receipt(reference.id, None))
            }
            Work::Start => {
                let host = live()?;
                let Some(row) = controller
                    .foreground_static_rows(principal, Some(id), None, 1)?
                    .pop()
                else {
                    return refuse(NOT_FOUND);
                };
                let recipe = [StaticRecipe::Web, StaticRecipe::Compose]
                    .into_iter()
                    .find(|recipe| {
                        row.provenance.recipe_digest == recipe.identity(dossier).recipe_sha256
                    })
                    .ok_or(unavailable(NativeRefusal::QualificationMissing))?;
                let (stop, stops) = mpsc::unbounded_channel();
                let run = Arc::new(Run {
                    stop,
                    origin: OnceLock::new(),
                });
                let held = supervisor.static_runs.register(id, run.clone())?;
                let worker =
                    match controller.admit(vm_request(Operation::Start), host, TEST_RESOURCES)? {
                        Admission::Admitted(worker) => worker,
                        Admission::Existing(prior) => {
                            return Ok(receipt(
                                controller
                                    .foreground_operation_ref(principal, prior.activity_id)?,
                                None,
                            ));
                        }
                        Admission::Refused(reason) => return Err(reason.into()),
                    };
                let public =
                    controller.foreground_operation_ref(principal, worker.activity_id())?;
                let principal = principal.to_owned();
                gate.spawn_test_blocking(
                    supervisor,
                    held,
                    move |supervisor| {
                        let _ = supervisor.controller.static_run_lost(id);
                    },
                    move |supervisor| {
                        let runtime = match worker_runtime() {
                            Ok(runtime) => runtime,
                            Err(_) => {
                                return supervisor
                                    .controller
                                    .finish(&worker, Completion::Failed)
                                    .map(|_| ());
                            }
                        };
                        runtime.block_on(run_static(
                            supervisor, run, stops, id, &principal, recipe, &worker,
                        ))
                    },
                );
                Ok(receipt(public, None))
            }
            Work::Stop => {
                let target = supervisor
                    .static_runs
                    .with_entry(id, |run| run.cloned())?
                    .filter(|run| run.origin.get().is_some())
                    .ok_or(NOT_READY)?;
                let stopped = HostObservation {
                    owner_idle: None,
                    logical_cpus: None,
                    available_ram: None,
                    capacity: None,
                };
                let worker =
                    match controller.admit(vm_request(Operation::Stop), stopped, TEST_RESOURCES)? {
                        Admission::Admitted(worker) => worker,
                        Admission::Existing(prior) => {
                            return Ok(receipt(
                                controller
                                    .foreground_operation_ref(principal, prior.activity_id)?,
                                None,
                            ));
                        }
                        Admission::Refused(reason) => return Err(reason.into()),
                    };
                let public =
                    controller.foreground_operation_ref(principal, worker.activity_id())?;
                if let Err(failed) = target.stop.send(worker) {
                    controller.finish(&failed.0, Completion::CleanupUnconfirmed)?;
                }
                Ok(receipt(public, None))
            }
            _ => refuse(Refusal::Disabled.into()),
        }
    }
}

fn worker_runtime() -> std::io::Result<tokio::runtime::Runtime> {
    Builder::new_current_thread().enable_all().build()
}

fn installed(supervisor: &Supervisor) -> anyhow::Result<StaticMachine<'_>> {
    let unavailable = || anyhow::anyhow!("qualified static worker unavailable");
    let dossier = supervisor
        .static_selection()
        .and_then(SelectedStatic::dossier)
        .ok_or_else(unavailable)?;
    let m4 = supervisor
        .selected_test
        .as_ref()
        .ok_or_else(unavailable)?
        .native_source();
    Ok(StaticMachine {
        m4,
        dossier,
        cancel: &supervisor.cancel,
    })
}

#[allow(
    clippy::too_many_arguments,
    reason = "the worker owns one admitted static boot"
)]
async fn run_static(
    supervisor: &Supervisor,
    run: Arc<Run>,
    mut stops: mpsc::UnboundedReceiver<PendingOperation>,
    vm: Id,
    principal: &str,
    recipe: StaticRecipe,
    worker: &PendingOperation,
) -> anyhow::Result<()> {
    let machine = installed(supervisor)?;
    let controller = &supervisor.controller;
    let (workspace, pair, generation) = match machine.hold_static(principal, vm, recipe) {
        Ok(held) => held,
        Err(_) => {
            return controller
                .finish(worker, Completion::CleanupUnconfirmed)
                .map(|_| ());
        }
    };
    let started = match machine
        .start(&workspace, pair, generation, recipe, worker.deadline())
        .await
    {
        Ok(boot) => boot,
        Err(_) => {
            return controller
                .finish(worker, Completion::CleanupUnconfirmed)
                .map(|_| ());
        }
    };
    let boot = started.boot;
    let origin = started.origin;
    let route = machine.dossier.retirement();
    let current = || {
        !supervisor.cancel.is_cancelled()
            && !route.is_cancelled()
            && machine.dossier.require_current().is_ok()
            && boot.current()
    };
    // Keep the attributed transport open for the web guest as well. EOF
    // can end its service and the nginx process in that service's cgroup.
    let mut pipe = Some(started.pipe);
    let activated: anyhow::Result<_> = async {
        if recipe == StaticRecipe::Compose {
            let pipe = pipe.take().expect("the boot owns one serial pipe");
            let sessions = tokio::select! {
                biased;
                () = route.cancelled() => anyhow::bail!("static route retired"),
                () = supervisor.cancel.cancelled() => anyhow::bail!("static boot cancelled"),
                sessions = tokio::time::timeout_at(worker.deadline().into(), compose::PreparedStaticSessions::negotiate(pipe, &origin, started.wire_boot)) => sessions??,
            };
            let channel = sessions.activate(controller, worker, &supervisor.compose, &supervisor.compose_backup, vm, current)?;
            Ok(Some(channel))
        } else {
            controller.finish_static_web_start(worker, vm, &origin, current)?;
            Ok(None)
        }
    }.await;
    let channel = match activated {
        Ok(channel) => channel,
        Err(_) => {
            origin.retired.retire();
            return boot
                .stop(|_| {
                    controller
                        .finish(worker, Completion::CleanupUnconfirmed)
                        .map(|_| ())
                })
                .await;
        }
    };
    anyhow::ensure!(
        run.origin.set(origin.clone()).is_ok(),
        "duplicate static boot"
    );
    let stop = loop {
        tokio::select! {
            biased;
            worker = stops.recv() => break worker,
            () = route.cancelled() => break None,
            () = supervisor.cancel.cancelled() => break None,
            () = tokio::time::sleep(Duration::from_secs(1)) => {
                if !current() || channel.as_ref().is_some_and(|channel| !channel.is_open()) {
                    break None;
                }
            }
        }
    };
    origin.retired.retire();
    stops.close();
    let stop = stop.or_else(|| stops.try_recv().ok());
    boot.stop(|disposition| settle_static_stop(controller, vm, stop.as_ref(), disposition))
        .await
}

fn settle_static_stop(
    controller: &Controller,
    vm: Id,
    stop: Option<&PendingOperation>,
    disposition: StopDisposition,
) -> anyhow::Result<()> {
    if let Some(stop) = stop {
        let completion = match disposition {
            StopDisposition::CleanRelease => Completion::Succeeded,
            StopDisposition::DirtyRelease => Completion::Failed,
            StopDisposition::ChargedQuarantine => Completion::CleanupUnconfirmed,
        };
        controller.finish(stop, completion)?;
    } else {
        controller.static_run_lost(vm)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
