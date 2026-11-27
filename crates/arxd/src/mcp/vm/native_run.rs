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
        OwnedStop,
        qualification::{
            BootReservation, ForegroundQualification, LaunchRequest, LaunchTarget, NativeLaunch,
            ProductionWorkspace, ReleasedGeneration, StartCreation, StartPair, StartRefusal,
            WorkspaceBinding,
        },
        runtime::ScriptAuthority,
        test_issuer::{
            CONTENTION_BUDGET, Installation, NativeTarget, SelectedSource, TestIssuer,
            retry_contended_async,
        },
    },
};
use arx_guest::ScriptStatus;
use tokio::{runtime::Runtime, sync::Notify};

const PATH: NativeRefusal = NativeRefusal::PathCustodyUnverified;
const REFUSED: NativeRefusal = NativeRefusal::QualificationMissing;
const SETUP_AND_READY: Duration = Duration::from_secs(60).saturating_add(guest::GUEST_READY_BUDGET);
const OBSERVE: Duration = Duration::from_secs(20);
/// Short backoff while a concurrent dispatch, such as a `vm_get` poll, holds
/// the foreground gate at the moment a validated boot is ready to commit.
const GATE_RETRY: Duration = Duration::from_millis(10);
/// Prompt bounded retry for establishing a recovery fence after a post-commit
/// pre-entry failure when the immediate store write fails transiently.
const FENCE_RETRY_DELAY: Duration = Duration::from_millis(20);
const FENCE_RETRY_WINDOW: Duration = Duration::from_secs(3);
/// After a stop, drain or loss signal, an in-flight script exchange gets at
/// most this long, well inside any script's own timeout, before the owner
/// abandons it as channel loss and tears down.
const SCRIPT_INTERRUPT: Duration = Duration::from_secs(2);
/// Poll interval while a cleanup turn waits for an ended owner to leave.
const CLEANUP_POLL: Duration = Duration::from_millis(10);

#[cfg(test)]
pub(super) struct BlockingBarrier {
    entered: AtomicBool,
    entered_signal: Notify,
    released: Mutex<bool>,
    release_signal: std::sync::Condvar,
}

#[cfg(test)]
impl BlockingBarrier {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            entered: AtomicBool::new(false),
            entered_signal: Notify::new(),
            released: Mutex::new(false),
            release_signal: std::sync::Condvar::new(),
        })
    }

    fn pause(&self) {
        self.entered.store(true, Ordering::SeqCst);
        self.entered_signal.notify_waiters();
        let mut released = self.released.lock().unwrap_or_else(|e| e.into_inner());
        let deadline = Instant::now() + Duration::from_secs(10);
        while !*released {
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                break;
            };
            let (next, _) = self
                .release_signal
                .wait_timeout(released, remaining)
                .unwrap_or_else(|e| e.into_inner());
            released = next;
        }
    }

    pub(super) async fn wait_entered(&self) {
        let notified = self.entered_signal.notified();
        tokio::pin!(notified);
        let _ = notified.as_mut().enable();
        if !self.entered.load(Ordering::SeqCst) {
            notified.await;
        }
    }

    pub(super) fn release(&self) {
        let mut released = self.released.lock().unwrap_or_else(|e| e.into_inner());
        *released = true;
        self.release_signal.notify_all();
    }
}

pub(super) struct Run {
    principal: String,
    target: Target,
    cancel: Shutdown,
    state: Mutex<State>,
    retry: Notify,
    scripts: Notify,
    #[cfg(test)]
    settled: std::sync::atomic::AtomicUsize,
    #[cfg(test)]
    stop_handoff_barrier: Mutex<Option<Arc<BlockingBarrier>>>,
    #[cfg(test)]
    after_retry_fence: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
}

#[derive(Default)]
struct State {
    witness: Option<Id>,
    ended: bool,
    stop_requested: bool,
    stop: Option<Arc<StopReceipt>>,
    /// One admitted script waiting for the owned channel. The ledger admits
    /// at most one pending script per VM.
    queued: Option<ScriptJob>,
    /// Admitted receipts whose settlement failed; retried, never dropped.
    unsettled: Vec<(PendingOperation, Settlement)>,
}

/// One durably admitted guest script for the owned channel.
pub(super) struct ScriptJob {
    pub(super) worker: PendingOperation,
    pub(super) script: String,
    pub(super) timeout_s: u32,
}

/// How an admitted script receipt settles.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Settlement {
    /// A typed guest result that passed the terminal cutoff.
    Guest(ScriptResult),
    /// The channel was lost with the script sent: execution is unknown and
    /// the VM enters recovery.
    Lost,
    /// An admitted owner stop interrupted the sent script: execution is
    /// unknown, and the stop's own release settles the VM.
    Stopped,
    /// Never dispatched: refused authorization, queued at stop or loss.
    Cancelled,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum ReceiptDisposition {
    Verified,
    Withdrawn,
}

/// Authorizes one admitted script at channel dispatch, consuming a fresh
/// single-use permit for its exact bytes, timeout and live writer epoch.
pub(super) trait ScriptAuthorizer {
    fn authorize(
        &self,
        worker: &PendingOperation,
        target: NativeTarget,
        script: &str,
        timeout_s: u32,
        cancel: &Shutdown,
    ) -> Result<ScriptAuthority, NativeRefusal>;
}

impl<S: SelectedSource, C: Clock> ScriptAuthorizer for TestIssuer<'_, S, C> {
    fn authorize(
        &self,
        worker: &PendingOperation,
        target: NativeTarget,
        script: &str,
        timeout_s: u32,
        cancel: &Shutdown,
    ) -> Result<ScriptAuthority, NativeRefusal> {
        let epoch = target.writer_epoch.ok_or(REFUSED)?;
        self.intent(worker, target, cancel)?
            .script(script, timeout_s)?
            .consume(target.generation, epoch, script, timeout_s)
    }
}

/// The selected admission inputs for scripts on an owned TEST channel.
#[allow(dead_code, reason = "TEST installation is closed until 7c 16")]
pub(super) struct NativeScripts {
    pub(super) installation: Installation,
    pub(super) host: Arc<dyn Fn() -> HostObservation + Send + Sync>,
    pub(super) resources: Resources,
}

impl Run {
    pub(super) fn new(principal: &str, target: Target) -> Self {
        Self {
            principal: principal.to_owned(),
            target,
            cancel: Shutdown::default(),
            state: Mutex::default(),
            retry: Notify::new(),
            scripts: Notify::new(),
            #[cfg(test)]
            settled: Default::default(),
            #[cfg(test)]
            stop_handoff_barrier: Mutex::default(),
            #[cfg(test)]
            after_retry_fence: Mutex::default(),
        }
    }

    #[cfg(test)]
    pub(super) fn arm_stop_handoff_barrier(&self, barrier: Arc<BlockingBarrier>) {
        *self.stop_handoff_barrier.lock().unwrap() = Some(barrier);
    }

    #[cfg(test)]
    pub(super) fn arm_after_retry_fence(&self) -> tokio::sync::oneshot::Receiver<()> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        *self.after_retry_fence.lock().unwrap() = Some(tx);
        rx
    }

    #[cfg(test)]
    fn pause_stop_handoff(&self) {
        let barrier = self
            .stop_handoff_barrier
            .lock()
            .ok()
            .and_then(|mut slot| slot.take());
        if let Some(barrier) = barrier {
            barrier.pause();
        }
    }

    /// Script receipt settlement attempts so far.
    #[cfg(test)]
    pub(super) fn scripts_settled(&self) -> usize {
        self.settled.load(Ordering::SeqCst)
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
    pub(super) fn target(&self) -> Target {
        self.target
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

    /// Ends this run for expiry or recovery cleanup. The witness goes first,
    /// then the live loop, a boot and any in-flight script are interrupted.
    /// No owner stop receipt exists, so an interrupted script settles as
    /// lost and teardown fences the writer before it commits the release.
    fn end_for_cleanup(&self) {
        self.withdraw();
        self.cancel.cancel();
        self.retry.notify_one();
    }

    /// Whether a new script could still be queued for this live run.
    fn accepts_script(&self) -> bool {
        !self.cancel.is_cancelled()
            && self
                .state
                .lock()
                .is_ok_and(|state| !state.ended && state.queued.is_none())
    }

    /// Queues an admitted script, or returns it when the run has ended so
    /// the caller settles its receipt. Ending and the final drain take the
    /// same lock, so no job is left behind.
    pub(super) fn enqueue(&self, job: ScriptJob) -> Result<(), ScriptJob> {
        let Ok(mut state) = self.state.lock() else {
            return Err(job);
        };
        if state.ended || self.cancel.is_cancelled() || state.queued.is_some() {
            return Err(job);
        }
        state.queued = Some(job);
        self.scripts.notify_one();
        Ok(())
    }

    fn take_script(&self) -> Option<ScriptJob> {
        self.state.lock().ok()?.queued.take()
    }

    /// Settles one receipt, keeping it for a later retry if the ledger
    /// refuses. A receipt is never dropped by a settlement failure.
    pub(super) fn settle_script(
        &self,
        supervisor: &Supervisor,
        worker: PendingOperation,
        settlement: Settlement,
    ) -> ReceiptDisposition {
        let mut disposition = ReceiptDisposition::Verified;
        let settled = finish_script(supervisor, &worker, self.target, settlement, || {
            self.withdraw();
            disposition = ReceiptDisposition::Withdrawn;
        });
        if settled.is_err() {
            if supervisor.controller.native_clock().monotonic >= worker.deadline() {
                self.withdraw();
                disposition = ReceiptDisposition::Withdrawn;
            }
            if let Ok(mut state) = self.state.lock() {
                state.unsettled.push((worker, settlement));
            }
        }
        #[cfg(test)]
        self.settled.fetch_add(1, Ordering::SeqCst);
        disposition
    }

    fn retry_receipts(&self, supervisor: &Supervisor) -> ReceiptDisposition {
        let Ok(pending) = self
            .state
            .lock()
            .map(|mut state| std::mem::take(&mut state.unsettled))
        else {
            return ReceiptDisposition::Withdrawn;
        };
        let mut disposition = ReceiptDisposition::Verified;
        for (worker, settlement) in pending {
            if self.settle_script(supervisor, worker, settlement) == ReceiptDisposition::Withdrawn {
                disposition = ReceiptDisposition::Withdrawn;
            }
        }
        disposition
    }

    fn receipt_deadline(&self) -> Option<Instant> {
        self.state
            .lock()
            .ok()?
            .unsettled
            .iter()
            .map(|(worker, _)| worker.deadline())
            .min()
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

    fn stop_requested(&self) -> bool {
        self.state.lock().is_ok_and(|state| state.stop_requested)
    }

    fn stop(&self, receipt: Option<StopReceipt>) -> Outcome<()> {
        let mut state = self.state.lock().map_err(|_| STATE_UNAVAILABLE)?;
        state.ended = true;
        state.witness = None;
        state.stop_requested = true;
        state.stop = receipt.map(Arc::new);
        for (_, settlement) in &mut state.unsettled {
            if matches!(settlement, Settlement::Guest(_)) {
                *settlement = Settlement::Stopped;
            }
        }
        self.cancel.cancel();
        Ok(())
    }

    /// Wake settlement once the caller has released the stop admission gate.
    pub(super) fn notify_retry(&self) {
        self.retry.notify_one();
    }

    fn project(&self, supervisor: &Supervisor, mut row: Value) -> Value {
        row["ready"] = json!(false);
        if supervisor
            .selected_test
            .as_ref()
            .is_some_and(|selected| selected.refresh().is_err())
        {
            return row;
        }
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
    /// On success, the caller wakes the owner after releasing the gate.
    pub(super) fn stop_subject(
        &self,
        args: Option<&Map<String, Value>>,
    ) -> Option<Outcome<(Value, Arc<Run>)>> {
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
        // A script still queued never reached the guest; one the run already
        // took is in flight. Taking it here means it can no longer dispatch.
        let queued = owner.take_script();
        let script = if queued.is_some() {
            FailureReason::Cancelled
        } else {
            FailureReason::ExecutionUnknown
        };
        let admitted = self.supervisor.controller.admit_owned_native_stop(
            &self.principal,
            request.key,
            owner.target,
            script,
            owner.stop_requested(),
        );
        // The normal stop replaces the queued receipt in the ledger. A
        // refusal or replay returns the job to its owner.
        let mut queued = queued;
        if !matches!(
            admitted,
            Ok(Ok((_, OwnedStop::Pending(_) | OwnedStop::Recovery)))
        ) && let Some(job) = queued.take()
            && let Err(job) = owner.enqueue(job)
        {
            owner.settle_script(&self.supervisor, job.worker, Settlement::Cancelled);
        }
        Some((|| match admitted? {
            Ok((operation, stop)) => {
                match stop {
                    OwnedStop::Pending(stop) => {
                        #[cfg(test)]
                        owner.pause_stop_handoff();
                        owner.stop(Some(stop))?;
                    }
                    OwnedStop::Recovery => {
                        owner.stop(None)?;
                        if let Some(job) = queued {
                            owner.settle_script(
                                &self.supervisor,
                                job.worker,
                                Settlement::Cancelled,
                            );
                        }
                    }
                    OwnedStop::Replay => {}
                }
                Ok((receipt(operation, None), owner))
            }
            Err(reason) => self.refuse(action, request.vm(), reason.into()),
        })())
    }

    /// One script for the caller's current TEST run owner, under the
    /// foreground gate after the generic replay and state precheck. The
    /// queue slot is checked before the durable admission; a run that ended
    /// in between settles the admitted receipt as cancelled. Without the
    /// fixture-only installation this route never matches.
    pub(super) fn script_subject(
        &self,
        action: i64,
        request: &Mutation,
        script: &str,
        timeout_s: u32,
    ) -> Option<Outcome<Value>> {
        let scripts = self.supervisor.native_scripts.get()?;
        let owner = match self
            .supervisor
            .native_runs
            .with_entry(request.id, |owner| owner.cloned())
        {
            Ok(Some(owner)) if owner.principal == self.principal => owner,
            Ok(_) => return None,
            Err(reason) => return Some(Err(reason)),
        };
        if !owner.accepts_script() {
            return Some(self.refuse(action, request.vm(), Refusal::Busy.into()));
        }
        let supervisor = &self.supervisor;
        let admission = supervisor.controller.admit_native_script(
            &scripts.installation,
            &Request {
                principal: &self.principal,
                vm: request.id,
                request_id: request.key,
                operation: Operation::Script {
                    script,
                    timeout_seconds: timeout_s.into(),
                },
                disk: DiskClaim::default(),
            },
            owner.target,
            (scripts.host)(),
            scripts.resources,
        );
        Some((|| match admission? {
            Admission::Admitted(worker) => {
                let operation = worker.admitted().ok_or(STATE_UNAVAILABLE)?.operation;
                let job = ScriptJob {
                    worker,
                    script: script.to_owned(),
                    timeout_s,
                };
                if let Err(job) = owner.enqueue(job) {
                    owner.settle_script(supervisor, job.worker, Settlement::Cancelled);
                }
                Ok(receipt(operation, None))
            }
            // A concurrent replay of the same request returns its receipt.
            Admission::Existing(_) => supervisor
                .controller
                .foreground_replay(
                    &self.principal,
                    request.key,
                    Subject::Vm(request.id),
                    action,
                )??
                .map(|previous| receipt(previous.operation.id, previous.lease_end))
                .ok_or(STATE_UNAVAILABLE),
            Admission::Refused(reason) => self.refuse(action, request.vm(), reason.into()),
        })())
    }
}

pub(super) struct Start {
    pub(super) worker: PendingOperation,
    pub(super) active: store::snapshot::Active,
}

/// The owned launch surface a live run drives. Production implements it for
/// [`NativeLaunch`]; fixtures supply a known child with duplex peers.
pub(super) trait LiveLaunch {
    type Proof;

    async fn boot(
        &mut self,
        cancel: &Shutdown,
        boot_by: Instant,
        refresh: impl FnMut() -> Result<Instant, NativeRefusal>,
    ) -> Result<(), NativeRefusal>;

    fn check_live(
        &self,
        refresh: impl FnOnce() -> Result<Instant, NativeRefusal>,
    ) -> Result<(), NativeRefusal>;

    async fn ping(
        &mut self,
        cancel: &Shutdown,
        refresh: impl FnMut() -> Result<Instant, NativeRefusal>,
    ) -> Result<(), NativeRefusal>;

    /// One script under an authority consumed for exactly these bytes and
    /// this timeout, never outlasting the admitted worker `deadline`. A
    /// typed guest result is `Ok`; any transport failure returns before
    /// teardown.
    async fn run_script(
        &mut self,
        authority: ScriptAuthority,
        script: String,
        timeout_s: u32,
        deadline: Instant,
        cancel: &Shutdown,
        refresh: impl FnMut() -> Result<Instant, NativeRefusal>,
    ) -> Result<(ScriptStatus, Option<u32>), NativeRefusal>;

    /// One bounded teardown attempt that keeps every handle. `None` leaves
    /// the same launch for the next retry.
    async fn stop(&mut self) -> Option<Self::Proof>;
}

impl<'a> LiveLaunch for NativeLaunch<'a> {
    type Proof = ReleasedGeneration<'a>;

    async fn boot(
        &mut self,
        cancel: &Shutdown,
        boot_by: Instant,
        refresh: impl FnMut() -> Result<Instant, NativeRefusal>,
    ) -> Result<(), NativeRefusal> {
        NativeLaunch::boot(self, cancel, boot_by, refresh).await
    }

    fn check_live(
        &self,
        refresh: impl FnOnce() -> Result<Instant, NativeRefusal>,
    ) -> Result<(), NativeRefusal> {
        NativeLaunch::check_live(self, refresh)
    }

    async fn ping(
        &mut self,
        cancel: &Shutdown,
        refresh: impl FnMut() -> Result<Instant, NativeRefusal>,
    ) -> Result<(), NativeRefusal> {
        NativeLaunch::ping(self, cancel, refresh).await
    }

    async fn run_script(
        &mut self,
        authority: ScriptAuthority,
        script: String,
        timeout_s: u32,
        deadline: Instant,
        cancel: &Shutdown,
        refresh: impl FnMut() -> Result<Instant, NativeRefusal>,
    ) -> Result<(ScriptStatus, Option<u32>), NativeRefusal> {
        NativeLaunch::run_script(
            self, authority, script, timeout_s, deadline, cancel, refresh,
        )
        .await
    }

    async fn stop(&mut self) -> Option<Self::Proof> {
        self.stop_generation(&Shutdown::default(), || true).await.1
    }
}

// The selected source runs on the tracked worker.
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
        self.start_native_run_with(
            supervisor,
            installation,
            request,
            host,
            resources,
            || {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
            },
            #[cfg(test)]
            || {},
            work,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn start_native_run_with(
        &mut self,
        supervisor: &Arc<Supervisor>,
        installation: &Installation,
        request: Request<'_>,
        host: HostObservation,
        resources: Resources,
        initialize: fn() -> std::io::Result<Runtime>,
        #[cfg(test)] after_admit: impl FnOnce(),
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
        #[cfg(test)]
        after_admit();
        let task = owner.clone();
        let cleanup_owner = owner.clone();
        let enqueued = slot.install(owner.clone()).and_then(|()| {
            self.enqueue_test_worker_with_cleanup(
                supervisor,
                slot,
                initialize,
                move |supervisor| {
                    cleanup_owner.withdraw();
                    fence_blocking(supervisor, cleanup_owner.target);
                },
                move |supervisor, runtime| {
                    work(supervisor, runtime, &task, Start { worker, active })
                },
            )
        });
        if let Err(reason) = enqueued {
            // Rollback-safe handoff: no worker owns the committed epoch, so
            // fence it immediately or leave a prompt bounded retry to establish
            // the recovery claim without blocking the foreground gate.
            owner.withdraw();
            if supervisor.controller.fence_native_run(target).is_err() {
                let supervisor = supervisor.clone();
                self.spawn_test_recovery(async move { retry_fence(supervisor, target).await });
            }
            return Err(reason);
        }
        Ok(receipt(epoch, None))
    }
}

#[allow(dead_code, reason = "TEST installation is closed until 7c 16")]
fn fence_blocking(supervisor: &Supervisor, target: Target) {
    let deadline = Instant::now() + FENCE_RETRY_WINDOW;
    loop {
        if supervisor.controller.fence_native_run(target).is_ok() {
            return;
        }
        let now = Instant::now();
        if now >= deadline {
            return;
        }
        std::thread::sleep(FENCE_RETRY_DELAY.min(deadline - now));
    }
}

#[allow(dead_code, reason = "TEST installation is closed until 7c 16")]
async fn retry_fence(supervisor: Arc<Supervisor>, target: Target) -> anyhow::Result<()> {
    let deadline = tokio::time::Instant::now() + FENCE_RETRY_WINDOW;
    loop {
        if supervisor.controller.fence_native_run(target).is_ok() {
            return Ok(());
        }
        let now = tokio::time::Instant::now();
        if now >= deadline {
            anyhow::bail!("TEST pre-entry recovery fence unconfirmed");
        }
        tokio::time::sleep(FENCE_RETRY_DELAY.min(deadline - now)).await;
    }
}

#[allow(dead_code, reason = "TEST installation is closed until 7c 16")]
fn replay_start(supervisor: &Supervisor, request: &Request<'_>) -> Outcome<Option<Value>> {
    Ok(supervisor
        .controller
        .foreground_replay_readonly(
            request.principal,
            request.request_id,
            Subject::Vm(request.vm),
            Action::Start as i64,
        )??
        .map(|previous| receipt(previous.operation.id, previous.lease_end)))
}

#[allow(dead_code, reason = "TEST installation is closed until 7c 16")]
impl Run {
    pub(super) async fn run<S: SelectedSource>(
        &self,
        supervisor: &Supervisor,
        start: Start,
        issuer: &TestIssuer<'_, S, ForegroundClock>,
        qualification: &ForegroundQualification,
    ) -> anyhow::Result<()> {
        let controller = &supervisor.controller;
        let pair = StartPair {
            slots: start.active.pair.0,
            evidence: start.active.pair.1,
        };
        let stopped = || self.cancel.is_cancelled() || supervisor.cancel.is_cancelled();
        let revoked = || issuer.epoch().effects_revoked();
        let Ok(scope) = retry_contended_async(
            &stopped,
            &revoked,
            || anyhow::anyhow!("TEST qualification missing"),
            || controller.native_workspace_scope(self.target.generation),
        )
        .await
        else {
            // No workspace was touched. Hand the committed epoch to recovery
            // instead of failing the worker, which would fence all TEST
            // admission.
            self.hand_off(supervisor).await;
            return Ok(());
        };
        let expected = binding(scope);
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
                    retry_contended_async(
                        &stopped,
                        &revoked,
                        || anyhow::anyhow!("TEST qualification missing"),
                        || controller.native_workspace_scope(slots.disk.generation),
                    )
                    .await
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
                        vars_content: None,
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
                    || self.observe_run(supervisor, false, &|| issuer.epoch().effects_revoked()),
                ) {
                    Ok(mut launch) => {
                        self.live(
                            supervisor,
                            &start.worker,
                            &mut launch,
                            issuer,
                            || issuer.epoch().effects_revoked(),
                            async |proof| self.settle_proof(supervisor, &proof).await,
                        )
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
        revoked: &dyn Fn() -> bool,
    ) -> Result<Instant, NativeRefusal> {
        let captured = Instant::now();
        if revoked()
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

    /// Boots the owned launch, commits readiness, then serves queued scripts
    /// and pings on the one owned channel while live, and settles its
    /// teardown. `revoked` reports selection-epoch revocation.
    pub(super) async fn live<L: LiveLaunch>(
        &self,
        supervisor: &Supervisor,
        worker: &PendingOperation,
        launch: &mut L,
        authorize: &impl ScriptAuthorizer,
        revoked: impl Fn() -> bool,
        settle: impl AsyncFnOnce(L::Proof),
    ) {
        let refresh = || self.observe_run(supervisor, false, &revoked);
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
                let receipt_deadline = self.receipt_deadline();
                tokio::select! {
                    biased;
                    _ = supervisor.cancel.cancelled() => break,
                    _ = self.cancel.cancelled() => break,
                    _ = async {
                        match receipt_deadline {
                            Some(deadline) => tokio::time::sleep_until(deadline.into()).await,
                            None => std::future::pending().await,
                        }
                    } => {}
                    _ = self.scripts.notified() => {}
                    _ = ping.tick() => {}
                }
                if self.retry_receipts(supervisor) == ReceiptDisposition::Withdrawn {
                    break;
                }
                let verified = match self.take_script() {
                    Some(job) => {
                        self.dispatch(supervisor, launch, authorize, &revoked, job)
                            .await
                    }
                    None => launch
                        .ping(&self.cancel, || {
                            self.observe_run(supervisor, true, &revoked)
                        })
                        .await
                        .is_ok(),
                };
                if !verified {
                    break;
                }
            }
        }
        self.settle_live(supervisor, async || launch.stop().await, settle)
            .await;
    }

    /// One admitted script on the owned channel, authorized at dispatch.
    /// Returns whether the channel is still verified. A stop, drain or loss
    /// interrupts the exchange within [`SCRIPT_INTERRUPT`], the admitted
    /// worker deadline caps it, and a result counts only if it arrived
    /// before that deadline with the run still current.
    async fn dispatch<L: LiveLaunch>(
        &self,
        supervisor: &Supervisor,
        launch: &mut L,
        authorize: &impl ScriptAuthorizer,
        revoked: &impl Fn() -> bool,
        job: ScriptJob,
    ) -> bool {
        let ScriptJob {
            worker,
            script,
            timeout_s,
        } = job;
        let deadline = worker.deadline();
        let Target {
            generation, epoch, ..
        } = self.target;
        let target = NativeTarget {
            generation,
            writer_epoch: Some(epoch),
        };
        let authority = match authorize
            .authorize(&worker, target, &script, timeout_s, &self.cancel)
            .and_then(|authority| {
                if authority.names(generation, epoch, &script, timeout_s) {
                    Ok(authority)
                } else {
                    Err(REFUSED)
                }
            }) {
            Ok(authority) => authority,
            Err(_) => {
                // No exchange was authorized. An unavailable ledger read
                // cannot establish that this run lost its writer.
                let lost = revoked()
                    || self.cancel.is_cancelled()
                    || supervisor.cancel.is_cancelled()
                    || matches!(
                        supervisor.controller.native_run_current(
                            &self.principal,
                            self.target,
                            true
                        ),
                        Ok(false)
                    );
                if lost {
                    self.withdraw();
                }
                let disposition = self.settle_script(supervisor, worker, Settlement::Cancelled);
                return !lost && disposition == ReceiptDisposition::Verified;
            }
        };
        let result = {
            let exchange =
                launch.run_script(authority, script, timeout_s, deadline, &self.cancel, || {
                    self.observe_run(supervisor, true, revoked)
                });
            tokio::pin!(exchange);
            tokio::select! {
                biased;
                result = &mut exchange => result,
                () = self.interrupted(supervisor) => Err(NativeRefusal::GuestEndpointUnverified),
                () = tokio::time::sleep_until(deadline.into()) => Err(REFUSED),
            }
        };
        // The terminal cutoff: the worker deadline, or a stop, revocation or
        // lost run after the final sample, suppresses the result.
        let result = result
            .and_then(|result| {
                if Instant::now() >= deadline {
                    return Err(REFUSED);
                }
                self.observe_run(supervisor, true, revoked).map(|_| result)
            })
            .ok()
            .and_then(|(status, code)| guest_result(status, code));
        match result {
            Some(result) => {
                self.settle_script(supervisor, worker, Settlement::Guest(result))
                    == ReceiptDisposition::Verified
            }
            None => {
                // Withdraw at loss detection, before the receipt or teardown.
                self.withdraw();
                let settlement = if self.stop_requested() {
                    Settlement::Stopped
                } else {
                    Settlement::Lost
                };
                self.settle_script(supervisor, worker, settlement);
                false
            }
        }
    }

    async fn interrupted(&self, supervisor: &Supervisor) {
        tokio::select! {
            _ = self.cancel.cancelled() => {}
            _ = supervisor.cancel.cancelled() => {}
        }
        tokio::time::sleep(SCRIPT_INTERRUPT).await;
    }

    /// Withdraws the witness and settles any queued script before the first
    /// teardown attempt, then keeps the same launch, its handles and join
    /// evidence through bounded retries. Receipts whose settlement failed
    /// are retried after teardown until they settle or drain hands them to
    /// recovery.
    pub(super) async fn settle_live<P>(
        &self,
        supervisor: &Supervisor,
        mut stop: impl AsyncFnMut() -> Option<P>,
        settle: impl AsyncFnOnce(P),
    ) {
        self.withdraw();
        if let Some(job) = self.take_script() {
            self.settle_script(supervisor, job.worker, Settlement::Cancelled);
        }
        loop {
            if let Some(proof) = stop().await {
                settle(proof).await;
                break;
            }
            if !self.wait_retry(supervisor).await {
                break;
            }
        }
        self.retry_receipts(supervisor);
        while self.receipt_deadline().is_some() {
            tokio::select! {
                biased;
                _ = supervisor.cancel.cancelled() => return,
                _ = self.retry.notified() => {}
                _ = tokio::time::sleep(Duration::from_secs(1)) => {}
            }
            self.retry_receipts(supervisor);
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
        // A refused observation or ledger read may be transient store
        // contention: observe again, bounded. Loss stays loss once the
        // contention budget is spent; cancellation ends it at once.
        let contended_until = Instant::now() + CONTENTION_BUDGET;
        loop {
            if live() {
                if let Ok(_gate) = supervisor.gate.try_lock() {
                    match supervisor.controller.finish_native_start(
                        worker,
                        &self.principal,
                        self.target,
                        cancelled,
                    ) {
                        Ok(committed) => return committed && self.publish(id, supervisor),
                        Err(_) if Instant::now() >= contended_until => return false,
                        Err(_) => {}
                    }
                }
            } else if Instant::now() >= contended_until {
                return false;
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

    pub(super) async fn hand_off(&self, supervisor: &Supervisor) {
        while supervisor.controller.fence_native_run(self.target).is_err() {
            tokio::select! {
                biased;
                _ = supervisor.cancel.cancelled() => return,
                _ = tokio::time::sleep(Duration::from_secs(1)) => {}
            }
        }
    }

    pub(super) async fn settle_proof(
        &self,
        supervisor: &Supervisor,
        proof: &ReleasedGeneration<'_>,
    ) {
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
                    self.retry_receipts(supervisor);
                    supervisor.controller.fence_native_run(self.target)?;
                }
                commit(&observed, stop.as_deref())
            })();
            if committed.is_ok() || !self.wait_retry(supervisor).await {
                break;
            }
        }
    }

    /// Whether this owner holds an admitted stop that can still succeed.
    fn holds_live_stop(&self) -> bool {
        self.state.lock().is_ok_and(|state| {
            state
                .stop
                .as_ref()
                .is_some_and(|stop| Instant::now() < stop.deadline())
        })
    }

    async fn wait_retry(&self, supervisor: &Supervisor) -> bool {
        // Keep the Job, lease, proof and retry ownership on the same worker.
        // A retryable failure under a live exact stop keeps its receipt
        // pending and the VM stopping, so a later attempt can still settle
        // it on time. Without one, or once it expires, fence the epoch.
        // Serialize the stop check and conditional fence under the foreground
        // gate held by stop admission so a retry cannot race an on-time stop
        // handoff and interrupt its receipt.
        if let Ok(_gate) = supervisor.gate.try_lock()
            && !self.holds_live_stop()
        {
            let _ = supervisor.controller.fence_native_run(self.target);
        }
        #[cfg(test)]
        if let Ok(mut hook) = self.after_retry_fence.lock()
            && let Some(signal) = hook.take()
        {
            let _ = signal.send(());
        }
        tokio::select! {
            biased;
            _ = supervisor.cancel.cancelled() => {
                // Drain hands the durable unresolved epoch to orphan recovery.
                let _ = supervisor.controller.fence_native_run(self.target);
                false
            }
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

    /// Expiry or recovery of a live TEST run: end its owner, then wait up to
    /// `join_by` for the owner to settle its release and leave. No gate or
    /// registry lock is held while waiting. Returns whether no owner remains;
    /// an owner still tearing down keeps sole custody of its handles.
    pub(super) fn end_native_run(&self, vm: Id, join_by: Instant) -> bool {
        match self.observe_test_worker(&self.native_runs, vm, |owner| owner.cloned()) {
            Ok(None) => return true,
            Ok(Some(owner)) => owner.end_for_cleanup(),
            Err(_) => return false,
        }
        loop {
            if matches!(
                self.native_runs.with_entry(vm, |owner| owner.is_none()),
                Ok(true)
            ) {
                return true;
            }
            let now = Instant::now();
            if now >= join_by {
                return false;
            }
            std::thread::sleep(CLEANUP_POLL.min(join_by - now));
        }
    }
}

fn finish_script(
    supervisor: &Supervisor,
    worker: &PendingOperation,
    target: Target,
    settlement: Settlement,
    withdraw: impl FnOnce(),
) -> anyhow::Result<()> {
    let (completion, script, cause) = match settlement {
        Settlement::Guest(result) => (
            if result == ScriptResult::Exited(0) {
                Completion::Succeeded
            } else {
                Completion::Failed
            },
            Some(result),
            None,
        ),
        Settlement::Lost => (
            Completion::CleanupUnconfirmed,
            None,
            Some(FailureReason::ExecutionUnknown),
        ),
        Settlement::Stopped => (
            Completion::Failed,
            None,
            Some(FailureReason::ExecutionUnknown),
        ),
        Settlement::Cancelled => (Completion::Failed, None, Some(FailureReason::Cancelled)),
    };
    supervisor
        .controller
        .settle_native_script(worker, target, completion, script, cause, withdraw)
}

/// The typed guest outcome. An exit without its code is malformed.
fn guest_result(status: ScriptStatus, code: Option<u32>) -> Option<ScriptResult> {
    Some(match status {
        ScriptStatus::Exited => ScriptResult::Exited(code?),
        ScriptStatus::Unavailable => ScriptResult::Unavailable,
        ScriptStatus::TimedOut => ScriptResult::TimedOut,
        ScriptStatus::LaunchFailed => ScriptResult::LaunchFailed,
        ScriptStatus::Terminated => ScriptResult::Terminated,
    })
}

fn binding(scope: Scope) -> WorkspaceBinding {
    WorkspaceBinding {
        generation: scope.generation,
        production: scope.production,
        workspace: scope.workspace,
        lease: scope.lease,
    }
}
