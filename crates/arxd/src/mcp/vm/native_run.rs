//! Fixture-gated ownership of one installed TEST start and its release.
use super::*;
use crate::{
    store::vm::{
        self as store,
        native::{
            Scope,
            release::{StopReceipt, Target},
        },
    },
    vm::{
        qualification::{
            BootReservation, ForegroundQualification, LaunchRequest, LaunchTarget, NativeLaunch,
            ProductionWorkspace, ReleasedGeneration, StartCreation, StartPair, StartRefusal,
            WorkspaceBinding,
        },
        test_issuer::{Installation, NativeTarget, SelectedSource, TestIssuer},
    },
};
use tokio::{runtime::Runtime, sync::Notify};

const PATH: NativeRefusal = NativeRefusal::PathCustodyUnverified;
const REFUSED: NativeRefusal = NativeRefusal::QualificationMissing;
const SETUP_AND_READY: Duration = Duration::from_secs(60).saturating_add(guest::GUEST_READY_BUDGET);
const OBSERVE: Duration = Duration::from_secs(20);
/// Short backoff while a concurrent dispatch, such as a `vm_get` poll, holds
/// the foreground gate at the moment a validated boot is ready to commit.
const GATE_RETRY: Duration = Duration::from_millis(10);

pub(super) struct Run {
    principal: String,
    target: Target,
    cancel: Shutdown,
    state: Mutex<State>,
    retry: Notify,
}

#[derive(Default)]
struct State {
    witness: Option<Id>,
    ended: bool,
    stop: Option<Arc<StopReceipt>>,
}

impl Run {
    pub(super) fn new(principal: &str, target: Target) -> Self {
        Self {
            principal: principal.to_owned(),
            target,
            cancel: Shutdown::default(),
            state: Mutex::default(),
            retry: Notify::new(),
        }
    }

    #[cfg(test)]
    pub(super) fn stop_signalled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    #[cfg(test)]
    pub(super) async fn retry_signalled(&self) -> bool {
        tokio::time::timeout(Duration::from_millis(50), self.retry.notified())
            .await
            .is_ok()
    }

    #[cfg(test)]
    pub(super) fn witnessed(&self) -> bool {
        self.state.lock().is_ok_and(|state| state.witness.is_some())
    }

    fn withdraw(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.ended = true;
            state.witness = None;
        }
    }

    fn publish(&self, id: Id, supervisor: &Supervisor) -> bool {
        self.state.lock().is_ok_and(|mut state| {
            if state.ended || self.cancel.is_cancelled() || supervisor.cancel.is_cancelled() {
                return false;
            }
            state.witness = Some(id);
            true
        })
    }

    fn stop(&self, receipt: StopReceipt) -> Outcome<()> {
        let mut state = self.state.lock().map_err(|_| STATE_UNAVAILABLE)?;
        state.ended = true;
        state.witness = None;
        state.stop = Some(Arc::new(receipt));
        self.cancel.cancel();
        self.retry.notify_one();
        Ok(())
    }

    fn project(&self, supervisor: &Supervisor, mut row: Value) -> Value {
        row["ready"] = json!(false);
        match supervisor
            .controller
            .native_run_current(&self.principal, self.target, true)
        {
            Ok(true) if !supervisor.cancel.is_cancelled() && row["state"] == "running" => {
                if let Ok(state) = self.state.lock()
                    && !state.ended
                    && !self.cancel.is_cancelled()
                    && let Some(id) = state.witness
                {
                    row["ready"] = json!(true);
                    if row["guest"] == "windows" {
                        row["native_boot_witness"] = json!({
                            "source": "guest_ready_handshake",
                            "vm_id": hex::encode(self.target.vm),
                            "generation": hex::encode(self.target.generation),
                            "operation_id": hex::encode(self.target.epoch),
                            "witness_id": hex::encode(id),
                        });
                    }
                }
            }
            // A start is still allowed to publish later. Loss of a previously
            // published epoch, including expiry, ends this owner permanently.
            Ok(false) => {
                if let Ok(mut state) = self.state.lock()
                    && state.witness.take().is_some()
                {
                    state.ended = true;
                    self.cancel.cancel();
                }
            }
            _ => {}
        }
        row
    }
}

impl VmInventory {
    pub(super) fn project_native_run(&self, vm: Id, row: Value) -> Value {
        let owner = self
            .supervisor
            .native_runs
            .with_entry(vm, |owner| owner.cloned());
        match owner {
            Ok(Some(owner)) if owner.principal == self.principal => {
                owner.project(&self.supervisor, row)
            }
            Ok(_) => row,
            Err(_) => {
                let mut row = row;
                row["ready"] = json!(false);
                row
            }
        }
    }

    /// This exact route runs under the foreground gate before the generic
    /// lifecycle state check and before the fresh-admission shutdown fence.
    pub(super) fn stop_subject(&self, args: Option<&Map<String, Value>>) -> Option<Outcome<Value>> {
        let mut args = Args::new(args?.clone());
        let request = Mutation::parse(ToolName::Stop, &mut args).ok()?;
        let owner = match self
            .supervisor
            .native_runs
            .with_entry(request.id, |owner| owner.cloned())
        {
            Ok(Some(owner)) if owner.principal == self.principal => owner,
            Ok(_) => return None,
            Err(reason) => return Some(Err(reason)),
        };
        let action = Action::Stop as i64;
        Some(
            (|| match self.supervisor.controller.admit_owned_native_stop(
                &self.principal,
                request.key,
                owner.target,
            )? {
                Ok((operation, stop)) => {
                    match stop {
                        Some(stop) => owner.stop(stop)?,
                        // A replayed stop nudges a teardown that is still
                        // retrying its release.
                        None => owner.retry.notify_one(),
                    }
                    Ok(receipt(operation, None))
                }
                Err(reason) => self.refuse(action, request.vm(), reason.into()),
            })(),
        )
    }
}

pub(super) struct Start {
    worker: PendingOperation,
    active: store::snapshot::Active,
}

// The activation chunk supplies the selected source on the tracked worker.
// Until then, only fixtures can construct the installation required here.
#[allow(dead_code, reason = "TEST installation is closed until 7c 16")]
impl Gate {
    /// The caller holds the foreground gate. A fresh start commits its pending
    /// operation and writer epoch only after its run slot is reserved and the
    /// worker queue is open, so no refusal can follow the durable write. A
    /// held slot still replays the exact request without a new write.
    pub(super) fn start_native_run(
        &mut self,
        supervisor: &Arc<Supervisor>,
        installation: &Installation,
        request: Request<'_>,
        host: HostObservation,
        resources: Resources,
        work: impl FnOnce(&Supervisor, &Runtime, &Run, Start) -> anyhow::Result<()> + Send + 'static,
    ) -> Outcome<Value> {
        if self.closed || supervisor.fenced.load(Ordering::SeqCst) {
            return Err(SHUTDOWN);
        }
        let slot = match supervisor.native_runs.reserve_owner(request.vm) {
            Ok(slot) => slot,
            Err(reason) => return replay_start(supervisor, &request)?.ok_or(reason),
        };
        self.test_workers_open(supervisor)?;
        let (decision, active) =
            supervisor
                .controller
                .admit_native_start(installation, &request, host, resources)?;
        let worker = match decision {
            Admission::Admitted(worker) => worker,
            Admission::Existing(_) => {
                return replay_start(supervisor, &request)?.ok_or(STATE_UNAVAILABLE);
            }
            Admission::Refused(reason) => return Err(reason.into()),
        };
        let active = active.ok_or(STATE_UNAVAILABLE)?;
        let epoch = worker.admitted().ok_or(STATE_UNAVAILABLE)?.operation;
        let target = Target {
            vm: request.vm,
            generation: active.pair.0.disk.generation,
            epoch,
        };
        let owner = Arc::new(Run::new(request.principal, target));
        let task = owner.clone();
        let enqueued = slot.install(owner.clone()).and_then(|()| {
            self.enqueue_test_worker(
                supervisor,
                slot,
                || {
                    tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                },
                move |supervisor, runtime| match work(
                    supervisor,
                    runtime,
                    &task,
                    Start { worker, active },
                ) {
                    Ok(()) => Ok(()),
                    Err(error) => {
                        task.withdraw();
                        supervisor.controller.fence_native_run(task.target)?;
                        Err(error)
                    }
                },
            )
        });
        if let Err(reason) = enqueued {
            // Rollback-safe handoff: no worker owns the committed epoch, so
            // fence it for ordinary recovery before refusing.
            owner.withdraw();
            let _ = supervisor.controller.fence_native_run(target);
            return Err(reason);
        }
        Ok(receipt(epoch, None))
    }
}

#[allow(dead_code, reason = "TEST installation is closed until 7c 16")]
fn replay_start(supervisor: &Supervisor, request: &Request<'_>) -> Outcome<Option<Value>> {
    Ok(supervisor
        .controller
        .foreground_replay(
            request.principal,
            request.request_id,
            Subject::Vm(request.vm),
            Action::Start as i64,
        )??
        .map(|previous| receipt(previous.operation.id, previous.lease_end)))
}

#[allow(dead_code, reason = "TEST installation is closed until 7c 16")]
impl Run {
    async fn run<S: SelectedSource>(
        &self,
        supervisor: &Supervisor,
        start: Start,
        issuer: &TestIssuer<'_, S, ForegroundClock>,
        qualification: &ForegroundQualification<'_>,
    ) -> anyhow::Result<()> {
        let controller = &supervisor.controller;
        let pair = StartPair {
            slots: start.active.pair.0,
            evidence: start.active.pair.1,
        };
        let expected = binding(controller.native_workspace_scope(self.target.generation)?);
        let workspace = qualification.hold_start_workspace(expected);
        if let Ok(workspace) = workspace {
            // An already spent preparation window refuses at the first check.
            let prepare_by = start
                .worker
                .deadline()
                .checked_sub(SETUP_AND_READY)
                .unwrap_or_else(Instant::now);
            let cancelled = || self.cancel.is_cancelled() || supervisor.cancel.is_cancelled();
            let fail = |reason| {
                StartRefusal::before_create(Some(&workspace), expected, Some(pair), reason)
            };
            let result = async {
                workspace
                    .check_start_pair(pair, prepare_by, &cancelled)
                    .map_err(fail)?;
                let (slots, evidence) = start.active.golden.ok_or_else(|| fail(PATH))?;
                let golden = binding(
                    controller
                        .native_workspace_scope(slots.disk.generation)
                        .map_err(|_| fail(PATH))?,
                );
                let backing = qualification
                    .hold_start_workspace(golden)
                    .and_then(|golden| {
                        golden.start_backing((slots.disk, evidence[0]), prepare_by, &cancelled)
                    })
                    .map_err(fail)?;
                let recipes = qualification.evidence().recipes;
                let recipe = [recipes.ubuntu, recipes.windows]
                    .into_iter()
                    .find(|recipe| recipe.recipe_sha256 == start.active.recipe)
                    .ok_or_else(|| fail(REFUSED))?;
                let boot = qualification
                    .prepare_boot(BootReservation {
                        workspace: &workspace,
                        recipe,
                        phase: None,
                        pair: pair.slots,
                        identities: pair.evidence.map(|file| file.identity),
                        backing: Some(&backing),
                        staged: None,
                    })
                    .map_err(fail)?;
                let planner = boot.planner();
                let plan = planner.plan().map_err(fail)?;
                let intent = issuer
                    .intent(
                        &start.worker,
                        NativeTarget {
                            generation: self.target.generation,
                            writer_epoch: Some(self.target.epoch),
                        },
                        &self.cancel,
                    )
                    .map_err(fail)?;
                let permit = intent
                    .launch(
                        expected,
                        LaunchTarget {
                            plan: &plan,
                            writer_epoch: self.target.epoch,
                            cwd: boot.cwd(),
                            pair: Some(pair.slots),
                            installer: None,
                            recipe: Some(recipe),
                            helper: None,
                            memory: NativeLaunch::START_MEMORY,
                            cpu_rate: NativeLaunch::START_CPU_RATE,
                        },
                    )
                    .map_err(fail)?;
                match NativeLaunch::hold(
                    qualification,
                    LaunchRequest {
                        plan,
                        boot_inputs: Some(&boot),
                        cwd: boot.cwd(),
                        workspace: &workspace,
                        writer_epoch: self.target.epoch,
                        pair: Some(pair),
                        installer: None,
                        helper_target: None,
                        memory: NativeLaunch::START_MEMORY,
                        cpu_rate: NativeLaunch::START_CPU_RATE,
                        prepare_by,
                        cancelled: &cancelled,
                    },
                    permit,
                    || self.observe_run(supervisor, false, issuer.epoch()),
                ) {
                    Ok(mut launch) => {
                        self.live(supervisor, &start.worker, &mut launch, issuer.epoch())
                            .await
                    }
                    Err(refusal) => {
                        self.settle_refusal(supervisor, refusal, |_| Err(PATH))
                            .await
                    }
                }
                Ok::<_, StartRefusal<'_>>(())
            }
            .await;
            if let Err(refusal) = result {
                self.settle_refusal(supervisor, refusal, |_| Err(PATH))
                    .await;
            }
        } else {
            let refusal = StartRefusal::before_create(None, expected, Some(pair), PATH);
            self.settle_refusal(supervisor, refusal, |expected| {
                qualification.hold_start_workspace(expected)
            })
            .await;
        }
        Ok(())
    }

    fn observe_run(
        &self,
        supervisor: &Supervisor,
        running: bool,
        epoch: &selection_epoch::SelectionEpoch,
    ) -> Result<Instant, NativeRefusal> {
        let captured = Instant::now();
        if epoch.effects_revoked()
            || self.cancel.is_cancelled()
            || supervisor.cancel.is_cancelled()
            || !supervisor
                .controller
                .native_run_current(&self.principal, self.target, running)
                .map_err(|_| REFUSED)?
        {
            return Err(REFUSED);
        }
        Ok(captured)
    }

    async fn live(
        &self,
        supervisor: &Supervisor,
        worker: &PendingOperation,
        launch: &mut NativeLaunch<'_>,
        epoch: &selection_epoch::SelectionEpoch,
    ) {
        let refresh = || self.observe_run(supervisor, false, epoch);
        if launch
            .boot(&self.cancel, worker.deadline(), refresh)
            .await
            .is_ok()
            && self
                .commit_ready(
                    supervisor,
                    worker,
                    || store::new_id().ok(),
                    || launch.check_live(refresh).is_ok(),
                )
                .await
        {
            // The final liveness sample immediately preceded the commit; the
            // first periodic ping follows one period later.
            let period = Duration::from_secs(1);
            let mut ping = tokio::time::interval_at(tokio::time::Instant::now() + period, period);
            ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tokio::select! {
                    biased;
                    _ = supervisor.cancel.cancelled() => break,
                    _ = self.cancel.cancelled() => break,
                    _ = ping.tick() => {}
                }
                if launch
                    .ping(&self.cancel, || self.observe_run(supervisor, true, epoch))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        }
        let teardown = Shutdown::default();
        self.settle_live(
            supervisor,
            async || launch.stop_generation(&teardown, || true).await.1,
            async |proof| self.settle_proof(supervisor, &proof).await,
        )
        .await;
    }

    /// Withdraws the witness before the first teardown attempt, then keeps
    /// the same launch, its handles and join evidence through bounded retries.
    pub(super) async fn settle_live<P>(
        &self,
        supervisor: &Supervisor,
        mut stop: impl AsyncFnMut() -> Option<P>,
        settle: impl AsyncFnOnce(P),
    ) {
        self.withdraw();
        loop {
            if let Some(proof) = stop().await {
                settle(proof).await;
                break;
            }
            if !self.wait_retry(supervisor).await {
                break;
            }
        }
    }

    /// Commits the exact start under the foreground gate, then publishes a
    /// fresh witness. Every attempt first takes the final production
    /// observation (child, Job, host and selection) outside the gate, then
    /// commits with no await in between. Gate contention backs off until the
    /// start deadline, and the next attempt observes again.
    pub(super) async fn commit_ready(
        &self,
        supervisor: &Supervisor,
        worker: &PendingOperation,
        new_id: impl FnOnce() -> Option<Id>,
        mut live: impl FnMut() -> bool,
    ) -> bool {
        let Some(id) = new_id().filter(|id| *id != self.target.epoch && *id != [0; 16]) else {
            return false;
        };
        let cancelled = || self.cancel.is_cancelled() || supervisor.cancel.is_cancelled();
        loop {
            if !live() {
                return false;
            }
            if let Ok(_gate) = supervisor.gate.try_lock() {
                return supervisor
                    .controller
                    .finish_native_start(worker, &self.principal, self.target, cancelled)
                    .is_ok_and(|committed| committed && self.publish(id, supervisor));
            }
            if cancelled() || Instant::now() + GATE_RETRY >= worker.deadline() {
                return false;
            }
            tokio::time::sleep(GATE_RETRY).await;
        }
    }

    async fn settle_refusal(
        &self,
        supervisor: &Supervisor,
        mut refusal: StartRefusal<'_>,
        mut acquire: impl FnMut(WorkspaceBinding) -> Result<ProductionWorkspace, NativeRefusal>,
    ) {
        self.withdraw();
        loop {
            match refusal.creation() {
                // An unconfirmed creation only terminates its Job here.
                StartCreation::Created | StartCreation::Unconfirmed => {
                    let _ = refusal
                        .settle_created(tokio::time::Instant::now() + Duration::from_secs(2))
                        .await;
                }
                StartCreation::NeverCreated => {}
            }
            if let Some(held) = refusal.observe(Instant::now() + OBSERVE, &|| false, &mut acquire)
                && let Some(proof) = held.release()
            {
                self.settle_proof(supervisor, proof).await;
                return;
            }
            if !refusal.can_progress() {
                // No later observation can prove this release. Hand the
                // unresolved epoch to recovery instead of pinning the lease
                // and registration until drain.
                self.hand_off(supervisor).await;
                return;
            }
            if !self.wait_retry(supervisor).await {
                return;
            }
        }
    }

    async fn hand_off(&self, supervisor: &Supervisor) {
        while supervisor.controller.fence_native_run(self.target).is_err() {
            tokio::select! {
                biased;
                _ = supervisor.cancel.cancelled() => return,
                _ = tokio::time::sleep(Duration::from_secs(1)) => {}
            }
        }
    }

    async fn settle_proof(&self, supervisor: &Supervisor, proof: &ReleasedGeneration<'_>) {
        self.settle_with(
            supervisor,
            || {
                proof
                    .observe_test_release(self.target, Instant::now() + OBSERVE, &|| false)
                    .map_err(|_| anyhow::anyhow!("unconfirmed TEST release"))
            },
            |observed, stop| {
                observed.try_commit_stop(supervisor.controller.native_store(), stop, || {
                    supervisor.controller.native_clock()
                })
            },
        )
        .await;
    }

    /// The held proof stays with this owner across observation, gate or
    /// ledger failures. The release commit never depends on a stop receipt.
    pub(super) async fn settle_with<O>(
        &self,
        supervisor: &Supervisor,
        mut observe: impl FnMut() -> anyhow::Result<O>,
        mut commit: impl FnMut(&O, Option<&StopReceipt>) -> anyhow::Result<()>,
    ) {
        loop {
            let committed = (|| -> anyhow::Result<()> {
                let observed = observe()?;
                // Serialize only the bounded ledger decision with stop
                // admission; no handle reopen, hashing or waits under the gate.
                let _gate = supervisor
                    .gate
                    .try_lock()
                    .map_err(|_| anyhow::anyhow!("VM gate unavailable"))?;
                let stop = self
                    .state
                    .lock()
                    .map_err(|_| anyhow::anyhow!("TEST owner unavailable"))?
                    .stop
                    .clone();
                if stop.is_none() {
                    supervisor.controller.fence_native_run(self.target)?;
                }
                commit(&observed, stop.as_deref())
            })();
            if committed.is_ok() || !self.wait_retry(supervisor).await {
                break;
            }
        }
    }

    async fn wait_retry(&self, supervisor: &Supervisor) -> bool {
        // Keep the Job, lease, proof and retry ownership on the same worker.
        // On drain the durable unresolved epoch hands off to orphan recovery.
        let _ = supervisor.controller.fence_native_run(self.target);
        tokio::select! {
            biased;
            _ = supervisor.cancel.cancelled() => false,
            _ = self.retry.notified() => true,
            _ = tokio::time::sleep(Duration::from_secs(1)) => true,
        }
    }
}

impl Supervisor {
    /// One ordinary reaper turn for a TEST VM. A live run owner keeps its
    /// generation, lease and held proof; the reaper only wakes its bounded
    /// retry. Orphan recovery runs only when no worker owns the VM.
    #[allow(
        dead_code,
        reason = "native TEST reaper installation is deferred to 7c 16"
    )]
    pub(super) fn reap_native_run(&self, vm: Id, recover: &mut dyn FnMut() -> bool) -> bool {
        match self.observe_test_worker(&self.native_runs, vm, |owner| owner.cloned()) {
            Ok(Some(owner)) => {
                owner.retry.notify_one();
                false
            }
            Ok(None) => self.with_test_orphan_fence(&self.native_runs, vm, recover),
            Err(_) => false,
        }
    }
}

fn binding(scope: Scope) -> WorkspaceBinding {
    WorkspaceBinding {
        generation: scope.generation,
        production: scope.production,
        workspace: scope.workspace,
        lease: scope.lease,
    }
}
