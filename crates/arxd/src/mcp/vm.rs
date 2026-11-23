//! Foreground-only VM tools. This module is selected after local authentication.
use crate::{
    config::{TestVms, VmRuntimeProfile},
    service::Shutdown,
    store::ListActivity,
    vm::{
        runtime::{
            NativeRefusal,
            publication::{Origin, Profile, Publication},
        },
        static_web::{StaticName, StaticState},
        *,
    },
};
use ring::hmac;
use rmcp::{
    ErrorData, RoleServer, ServerHandler,
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ListToolsResult,
        PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
    },
    service::RequestContext,
};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Clone)]
struct ForegroundClock {
    #[cfg(test)]
    utc: Option<Arc<std::sync::atomic::AtomicI64>>,
}

impl Clock for ForegroundClock {
    fn now(&self) -> ClockReading {
        #[cfg(test)]
        if let Some(now) = &self.utc {
            return ClockReading {
                utc_ms: now.load(Ordering::Relaxed),
                monotonic: Instant::now(),
            };
        }
        ClockReading {
            utc_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .ok()
                .and_then(|elapsed| i64::try_from(elapsed.as_millis()).ok())
                .unwrap_or(-1),
            monotonic: Instant::now(),
        }
    }
}

type Controller = VmController<ForegroundClock>;
type Outcome<T> = std::result::Result<T, Fault>;

#[derive(Clone, Copy, Debug)]
struct Fault(&'static str);

const INVALID: Fault = Fault("invalid");
const INVALID_CURSOR: Fault = Fault("invalid_cursor");
const SHUTDOWN: Fault = Fault("shutdown");
const NOT_FOUND: Fault = Fault("not_found");
const STATE_UNAVAILABLE: Fault = Fault("state_unavailable");
const NOT_READY: Fault = Fault("not_ready");
const HOST_EXECUTION_DISABLED: Fault = Fault("host_execution_disabled");

impl From<anyhow::Error> for Fault {
    fn from(_: anyhow::Error) -> Self {
        STATE_UNAVAILABLE
    }
}

/// The single checked controller-to-wire refusal projection.
fn refusal_wire(reason: Refusal) -> &'static str {
    match reason {
        Refusal::Invalid => "invalid",
        Refusal::StaleKey => "stale_key",
        Refusal::KeyCollision => "key_collision",
        Refusal::ReplayFull => "replay_full",
        Refusal::PendingFull => "pending_full",
        Refusal::NotFound => "not_found",
        Refusal::Busy => "busy",
        Refusal::Expired => "expired",
        Refusal::TemplateLimit => "template_limit",
        Refusal::InstanceLimit => "instance_limit",
        Refusal::SnapshotLimit => "snapshot_limit",
        Refusal::Disk => "disk",
        Refusal::StateHeadroom => "state_headroom",
        Refusal::UnknownHost => "unknown_host",
        Refusal::OwnerActive => "owner_active",
        Refusal::Cpu => "cpu",
        Refusal::Ram => "ram",
        Refusal::Shutdown => "shutdown",
        Refusal::CleanupUnconfirmed => "cleanup_unconfirmed",
        Refusal::Disabled => "disabled",
        Refusal::UnsupportedPlatform => "unsupported_platform",
        Refusal::QualificationMissing => "qualification_missing",
        Refusal::IdentityUnverified => "identity_unverified",
        Refusal::DistributionUnverified => "distribution_unverified",
        Refusal::PathCustodyUnverified => "path_custody_unverified",
        Refusal::JobUnverified => "job_unverified",
        Refusal::WhpxUnverified => "whpx_unverified",
        Refusal::QmpUnverified => "qmp_unverified",
        Refusal::GuestEndpointUnverified => "guest_endpoint_unverified",
        Refusal::DiskCeilingUnverified => "disk_ceiling_unverified",
        Refusal::OwnerActivityUnverified => "owner_activity_unverified",
        Refusal::ReservesUnverified => "reserves_unverified",
        Refusal::NetworkFenceUnverified => "network_fence_unverified",
        Refusal::InstallerPhaseUnavailable => "installer_phase_unavailable",
        Refusal::InstallerPhaseMismatch => "installer_phase_mismatch",
        Refusal::InstallerDeadlineExpired => "installer_deadline_expired",
        Refusal::NotReady => "not_ready",
        Refusal::RecipeUnqualified => "recipe_unqualified",
        Refusal::HostExecutionDisabled => "host_execution_disabled",
        Refusal::InvalidCursor => "invalid_cursor",
        Refusal::StateUnavailable => "state_unavailable",
        Refusal::NameUnavailable => "name_unavailable",
        Refusal::StaticDeleteForbidden => "static_delete_forbidden",
        Refusal::UnknownTool => "unknown_tool",
        Refusal::PublicSitesDisabled => "public_sites_disabled",
        Refusal::PublishCredentialMissing => "publish_credential_missing",
        Refusal::PublishTargetMismatch => "publish_target_mismatch",
        Refusal::PublishCleanupPending => "publish_cleanup_pending",
    }
}

/// Stored audit reason codes; an unknown future code is never guessed.
fn refusal_code(code: i64) -> Option<Refusal> {
    REFUSAL_CODES
        .into_iter()
        .find(|reason| *reason as i64 == code)
}

/// The stored code for a wire reason, only where the codes match exactly.
fn refusal_from_wire(reason: &str) -> Option<Refusal> {
    REFUSAL_CODES
        .into_iter()
        .find(|code| refusal_wire(*code) == reason)
}

const REFUSAL_CODES: [Refusal; 48] = {
    use Refusal::*;
    [
        Invalid,
        StaleKey,
        KeyCollision,
        ReplayFull,
        PendingFull,
        NotFound,
        Busy,
        Expired,
        TemplateLimit,
        InstanceLimit,
        SnapshotLimit,
        Disk,
        StateHeadroom,
        UnknownHost,
        OwnerActive,
        Cpu,
        Ram,
        Shutdown,
        CleanupUnconfirmed,
        Disabled,
        UnsupportedPlatform,
        QualificationMissing,
        IdentityUnverified,
        DistributionUnverified,
        PathCustodyUnverified,
        JobUnverified,
        WhpxUnverified,
        QmpUnverified,
        GuestEndpointUnverified,
        DiskCeilingUnverified,
        OwnerActivityUnverified,
        ReservesUnverified,
        NetworkFenceUnverified,
        InstallerPhaseUnavailable,
        InstallerPhaseMismatch,
        InstallerDeadlineExpired,
        NotReady,
        RecipeUnqualified,
        HostExecutionDisabled,
        InvalidCursor,
        StateUnavailable,
        NameUnavailable,
        StaticDeleteForbidden,
        UnknownTool,
        PublicSitesDisabled,
        PublishCredentialMissing,
        PublishTargetMismatch,
        PublishCleanupPending,
    ]
};

impl From<Refusal> for Fault {
    fn from(reason: Refusal) -> Self {
        Self(refusal_wire(reason))
    }
}

fn native_reason(reason: NativeRefusal) -> &'static str {
    use NativeRefusal::*;
    match reason {
        Disabled => "disabled",
        UnsupportedPlatform => "unsupported_platform",
        QualificationMissing => "qualification_missing",
        IdentityUnverified => "identity_unverified",
        DistributionUnverified => "distribution_unverified",
        PathCustodyUnverified => "path_custody_unverified",
        JobUnverified => "job_unverified",
        WhpxUnverified => "whpx_unverified",
        QmpUnverified => "qmp_unverified",
        GuestEndpointUnverified => "guest_endpoint_unverified",
        DiskCeilingUnverified => "disk_ceiling_unverified",
        OwnerActivityUnverified => "owner_activity_unverified",
        ReservesUnverified => "reserves_unverified",
        NetworkFenceUnverified => "network_fence_unverified",
        InstallerPhaseUnavailable => "installer_phase_unavailable",
        InstallerPhaseMismatch => "installer_phase_mismatch",
        InstallerDeadlineExpired => "installer_deadline_expired",
    }
}

// Stable catalog identity while native qualification is unavailable.
const WEB_RECIPE_ID: Id = [0xff; 16];
type Recipe = (Id, Option<[u8; 32]>);

struct Gate {
    closed: bool,
    allow_agent_public_sites: bool,
    publish_profile: Option<Arc<Profile>>,
    publish_jobs: tokio::task::JoinSet<anyhow::Result<()>>,
    #[cfg(test)]
    fixture: Option<tests::Fixture>,
    #[cfg(test)]
    drained: Option<tests::FixtureDrain>,
}

pub(crate) struct Supervisor {
    controller: Arc<Controller>,
    gate: Mutex<Gate>,
    /// Lock-free foreground admission fence, sampled inside the gate.
    fenced: AtomicBool,
    cancel: Shutdown,
    cursor_key: hmac::Key,
    host_id: Id,
    // Two TEST recipes and the independent A2 web recipe share one page.
    recipes: [Option<Recipe>; 3],
    native_refusal: NativeRefusal,
    /// The validated native runtime profile, if configured. It only selects
    /// the closed candidate runtime; it never authorizes a launch here.
    runtime: Option<VmRuntimeProfile>,
    publisher: Publication,
    /// Deferred local stops queued by owner revocation, at most one in flight
    /// per activation generation. Kept outside the dispatch gate: revocation
    /// runs before the gate, the replay ledger and every store write.
    stops: Mutex<StopJobs>,
    /// Deferred stops ever spawned, for witnesses.
    #[cfg(test)]
    stops_spawned: std::sync::atomic::AtomicUsize,
    /// Signalled after the drain's first stop pass, before it joins workers.
    #[cfg(test)]
    drain_first_pass: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    #[cfg(test)]
    ticks: Mutex<Option<tests::TestTicks>>,
    /// Parks the next tool call before admission.
    #[cfg(test)]
    hold: Mutex<Option<tests::Hold>>,
    #[cfg(test)]
    closed_connections: std::sync::atomic::AtomicUsize,
    /// The validated state root, owned by every store user: dispatch, the
    /// reaper and any detached close or recovery hold this supervisor, so a
    /// late store write after an unconfirmed result still runs under the
    /// same namespace guarantee. `None` only for in-module fixtures.
    root_pin: Option<Box<crate::config::RootPin>>,
    /// Blocks final recovery until released; see [`tests::Stall`].
    #[cfg(test)]
    stall_recovery: Mutex<Option<tests::StallHook>>,
    /// Blocks the next dispatch while it holds the gate.
    #[cfg(test)]
    stall_dispatch: Mutex<Option<tests::StallHook>>,
    /// Blocks the next publish after its activation is registered and before
    /// its admission transaction.
    #[cfg(test)]
    stall_admission: Mutex<Option<tests::StallHook>>,
    /// Blocks the next sweep.
    #[cfg(test)]
    stall_sweep: Mutex<Option<tests::StallHook>>,
    /// Acknowledges a fatal accept after the fence, then holds the listener
    /// until its stop is issued.
    #[cfg(test)]
    fatal_accept: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
}

impl Supervisor {
    pub(crate) fn open(
        root: &Path,
        mut profile: TestVms,
        pin: Box<crate::config::RootPin>,
    ) -> anyhow::Result<Arc<Self>> {
        let reason = if !profile.enabled {
            NativeRefusal::Disabled
        } else if cfg!(all(windows, target_arch = "x86_64")) {
            NativeRefusal::QualificationMissing
        } else {
            NativeRefusal::UnsupportedPlatform
        };
        let policy = profile.policy();
        let recipe = profile.recipe()?;
        let mut supervisor = Self::new(
            root,
            policy,
            ForegroundClock {
                #[cfg(test)]
                utc: None,
            },
            recipe,
            reason,
            profile.runtime.take(),
        )?;
        let state = Arc::get_mut(&mut supervisor)
            .ok_or_else(|| anyhow::anyhow!("VM supervisor shared before open"))?;
        state.root_pin = Some(pin);
        let gate = state
            .gate
            .get_mut()
            .map_err(|_| anyhow::anyhow!("VM gate unavailable"))?;
        gate.allow_agent_public_sites = profile.allow_agent_public_sites;
        gate.publish_profile = profile.publication.map(Arc::from);
        state.recipes[1] = Some((WEB_RECIPE_ID, None));
        state.recipes[2] = Some((Controller::windows_recipe_id(), None));
        Ok(supervisor)
    }

    fn new(
        root: &Path,
        policy: VmPolicy,
        clock: ForegroundClock,
        recipe: Option<(Id, [u8; 32])>,
        native_refusal: NativeRefusal,
        runtime: Option<VmRuntimeProfile>,
    ) -> anyhow::Result<Arc<Self>> {
        anyhow::ensure!(
            !recipe.is_some_and(|(id, _)| id == Controller::windows_recipe_id() || id == WEB_RECIPE_ID),
            "VM recipe ID conflicts with a built-in entry"
        );
        let mut secret = [0; 32];
        getrandom::fill(&mut secret).map_err(|_| anyhow::anyhow!("cursor key unavailable"))?;
        let controller = Arc::new(VmController::open(root, policy, clock)?);
        let host_id = controller.foreground_host_id()?;
        Ok(Arc::new(Self {
            controller,
            gate: Mutex::new(Gate {
                closed: false,
                allow_agent_public_sites: false,
                publish_profile: None,
                publish_jobs: Default::default(),
                #[cfg(test)]
                fixture: None,
                #[cfg(test)]
                drained: None,
            }),
            fenced: AtomicBool::new(false),
            cancel: Shutdown::default(),
            cursor_key: hmac::Key::new(hmac::HMAC_SHA256, &secret),
            host_id,
            recipes: [recipe.map(|(id, digest)| (id, Some(digest))), None, None],
            native_refusal,
            runtime,
            publisher: Publication::default(),
            stops: Mutex::default(),
            #[cfg(test)]
            stops_spawned: Default::default(),
            #[cfg(test)]
            drain_first_pass: Mutex::new(None),
            #[cfg(test)]
            ticks: Mutex::new(None),
            #[cfg(test)]
            hold: Mutex::new(None),
            #[cfg(test)]
            closed_connections: Default::default(),
            root_pin: None,
            #[cfg(test)]
            stall_recovery: Mutex::new(None),
            #[cfg(test)]
            stall_dispatch: Mutex::new(None),
            #[cfg(test)]
            stall_admission: Mutex::new(None),
            #[cfg(test)]
            stall_sweep: Mutex::new(None),
            #[cfg(test)]
            fatal_accept: Mutex::new(None),
        }))
    }

    /// The canonical root still denotes the pinned directory.
    pub(crate) fn verify_root(&self, root: &Path) -> anyhow::Result<()> {
        match &self.root_pin {
            Some(pin) => pin.verify(root),
            None => anyhow::bail!("VM state root not pinned"),
        }
    }

    pub(crate) fn connection(self: &Arc<Self>, principal: String) -> Connection {
        Connection(VmInventory {
            supervisor: self.clone(),
            closed: Arc::new(AtomicBool::new(false)),
            principal,
        })
    }

    fn lock(&self) -> anyhow::Result<std::sync::MutexGuard<'_, Gate>> {
        self.gate
            .lock()
            .map_err(|_| anyhow::anyhow!("VM supervisor unavailable"))
    }

    /// Fence fresh foreground admission at once, without taking any lock, and
    /// signal cancellation. Dispatch samples the fence inside the gate, so an
    /// admission that has not reached its commit refuses as `shutdown`.
    pub(crate) fn fence(&self) {
        self.fenced.store(true, Ordering::SeqCst);
        self.cancel.cancel();
    }

    /// The close barrier: fence, then close foreground admission, then
    /// controller admission. The same gate covers request cancellation
    /// sampling and the complete SQLite commit and worker registration, so
    /// once this returns every admitted operation is tracked.
    pub(crate) fn close(&self) {
        self.fence();
        if let Ok(mut gate) = self.gate.lock() {
            gate.closed = true;
        }
        // A failed close leaves the controller gate poisoned, which already
        // refuses every admission; teardown continues either way.
        let _ = self.controller.close_admission();
    }

    /// [`Supervisor::close`] off the async executor within `bound`. On expiry
    /// the barrier is unresolved: the caller reports an unconfirmed drain and
    /// the blocking close may still finish later.
    pub(crate) async fn close_within(self: &Arc<Self>, bound: Duration) -> anyhow::Result<()> {
        self.fence();
        let supervisor = self.clone();
        let mut task = tokio::task::spawn_blocking(move || supervisor.close());
        match tokio::time::timeout(bound, &mut task).await {
            Ok(result) => Ok(result?),
            Err(_) => {
                // Cancels the close only if it has not started.
                task.abort();
                anyhow::bail!("VM admission close unconfirmed")
            }
        }
    }

    /// Test-only acknowledgement of a fenced fatal accept; see the field.
    #[cfg(test)]
    pub(crate) async fn fatal_accept_observed(&self, stop: &Shutdown) {
        let ack = self.fatal_accept.lock().ok().and_then(|mut ack| ack.take());
        if let Some(ack) = ack {
            let _ = ack.send(());
            stop.cancelled().await;
        }
    }

    fn candidate(&self) -> runtime::CandidateRuntime<'_> {
        runtime::CandidateRuntime::new(self.runtime.as_ref())
    }

    /// One bounded reaper turn before the listener is published.
    pub(crate) fn startup_sweep(&self) -> anyhow::Result<()> {
        self.sweep()
    }

    fn sweep(&self) -> anyhow::Result<()> {
        #[cfg(test)]
        let _completed = self
            .stall_sweep
            .lock()
            .ok()
            .and_then(|mut stall| stall.take())
            .map(tests::StallHook::enter);
        #[cfg(test)]
        if let Some(reaper) = self.lock()?.fixture.as_ref().map(tests::Fixture::reaper) {
            self.controller.sweep_with(&reaper, &*reaper.custody)?;
            return Ok(());
        }
        self.controller.sweep_once(&self.candidate())?;
        Ok(())
    }

    /// The supervised periodic reaper; the caller cancels and joins it.
    ///
    /// Each turn is the same bounded sweep as the startup sweep, run on the
    /// blocking pool so controller locks and SQLite never occupy the
    /// scheduler that drives the teardown deadlines. A turn in progress is
    /// awaited before this returns, so the caller's bounded join covers it;
    /// on expiry the drain is unconfirmed while the turn finishes detached.
    pub(crate) async fn run_reaper(self: &Arc<Self>, stop: &Shutdown) -> anyhow::Result<()> {
        tokio::select! {
            biased;
            _ = stop.cancelled() => return Ok(()),
            _ = self.publication_turn() => {}
        }
        #[cfg(test)]
        if let Some(mut ticks) = self.ticks.lock().ok().and_then(|mut ticks| ticks.take()) {
            return self.reaper_turns(&mut ticks, stop).await;
        }
        self.reaper_turns(&mut MonotonicReaperTicks::default(), stop)
            .await
    }

    async fn reaper_turns(
        self: &Arc<Self>,
        ticks: &mut impl reaper::ReaperTick,
        stop: &Shutdown,
    ) -> anyhow::Result<()> {
        loop {
            tokio::select! {
                biased;
                _ = stop.cancelled() => return Ok(()),
                more = ticks.next_tick() => {
                    if !more {
                        return Ok(());
                    }
                    tokio::select! {
                        biased;
                        _ = stop.cancelled() => return Ok(()),
                        _ = self.publication_turn() => {}
                    }
                    let supervisor = self.clone();
                    tokio::task::spawn_blocking(move || supervisor.sweep()).await??;
                }
            }
        }
    }

    /// One reaper publication turn. Local ingress closes before any store
    /// write, and the durable intent survives a failed write, so a store or
    /// audit failure here is retried on the next tick instead of ending the
    /// reaper.
    async fn publication_turn(&self) -> bool {
        self.reap_stops();
        self.publisher
            .turn(&self.controller, &self.cancel)
            .await
            .is_ok()
    }

    /// Queues the local stop of one revoked activation generation, unless a
    /// stop for it is already in flight. Completed jobs are dropped first, so
    /// the set holds at most one job per generation still being stopped. The
    /// outcome is recorded in the publisher's unconfirmed-stop state.
    fn stop_revoked(self: &Arc<Self>, generation: u64) {
        let Ok(mut stops) = self.stops.lock() else {
            return;
        };
        stops.reap();
        if stops.running.values().any(|running| *running == generation) {
            return;
        }
        let supervisor = self.clone();
        let job = stops.set.spawn(async move {
            supervisor.publisher.stop_activation(generation).await;
        });
        stops.running.insert(job.id(), generation);
        #[cfg(test)]
        self.stops_spawned.fetch_add(1, Ordering::SeqCst);
    }

    /// Drops completed deferred stops.
    fn reap_stops(&self) {
        if let Ok(mut stops) = self.stops.lock() {
            stops.reap();
        }
    }

    /// Cancel and join every admitted worker and its terminal audit write.
    /// The caller has passed the close barrier, so no new worker can be
    /// registered.
    pub(crate) async fn drain_workers(&self, bound: Duration) -> anyhow::Result<()> {
        let deadline = tokio::time::Instant::now() + bound;
        let mut publish_jobs = std::mem::take(&mut self.lock()?.publish_jobs);
        let mut stops = self
            .stops
            .lock()
            .map(|mut jobs| std::mem::take(&mut *jobs))
            .map_err(|_| anyhow::anyhow!("publication settlement unconfirmed"))?;
        tokio::time::timeout_at(deadline, async {
            // Local stop only: registrations without a connector belong to
            // workers that have not joined yet. The outcome is decided below.
            self.publisher.stop_local(None).await;
            #[cfg(test)]
            if let Some(passed) = self
                .drain_first_pass
                .lock()
                .ok()
                .and_then(|mut slot| slot.take())
            {
                let _ = passed.send(());
            }
            let mut failed = false;
            while let Some(job) = publish_jobs.join_next().await {
                failed |= !matches!(job, Ok(Ok(())));
            }
            while let Some(job) = stops.set.join_next().await {
                failed |= job.is_err();
            }
            // A deferred stop may have confirmed what the first attempt could
            // not, so the final local state decides.
            // Every worker and deferred stop has joined: confirm the final local
            // state and clear what remains of the registry.
            let local = self.publisher.stop_all().await;
            failed |= !local || self.publisher.stop_unconfirmed();
            anyhow::ensure!(!failed, "publication settlement unconfirmed");
            Ok::<_, anyhow::Error>(())
        })
        .await??;
        #[cfg(test)]
        {
            let jobs = self.lock()?.fixture.as_mut().map(tests::Fixture::take_jobs);
            if let Some(jobs) = jobs {
                let drained = tokio::time::timeout_at(deadline, jobs.join()).await??;
                self.lock()?.drained = Some(drained);
            }
        }
        // Local containment and worker settlement have their own proof.
        // A remote timeout leaves the intent intact for startup reconciliation.
        if let Ok(result) = tokio::time::timeout_at(
            deadline,
            self.publisher.turn(&self.controller, &self.cancel),
        )
        .await
        {
            result?;
        }
        Ok(())
    }

    /// Final controller recovery, after the reaper and workers are joined,
    /// then one more custody turn. Unresolved custody, including build or
    /// staging quarantine and legacy uncertain rows, is an unconfirmed drain.
    ///
    /// The whole stage, including any fixture release, has one hard deadline.
    /// Running blocking work cannot be cancelled: on expiry this returns
    /// "VM recovery unconfirmed" while the detached task keeps the store and
    /// its lock until it finishes. The foreground executable then exits
    /// nonzero; an in-process caller must not treat the store as quiescent.
    pub(crate) async fn recover(self: &Arc<Self>, bound: Duration) -> anyhow::Result<()> {
        let deadline = tokio::time::Instant::now() + bound;
        let unconfirmed = || anyhow::anyhow!("VM recovery unconfirmed");
        #[cfg(test)]
        {
            let drained = self.lock()?.drained.take();
            if let Some(drained) = drained {
                tokio::time::timeout_at(deadline, drained.release())
                    .await
                    .map_err(|_| unconfirmed())??;
            }
        }
        #[cfg(test)]
        let stall = self
            .stall_recovery
            .lock()
            .ok()
            .and_then(|mut stall| stall.take());
        let supervisor = self.clone();
        let mut task = tokio::task::spawn_blocking(move || {
            #[cfg(test)]
            let _completed = stall.map(tests::StallHook::enter);
            supervisor.controller.recover_after_drain()?;
            supervisor.sweep()?;
            anyhow::ensure!(
                !supervisor.controller.foreground_custody_uncertain()?,
                "VM release unconfirmed"
            );
            Ok(())
        });
        match tokio::time::timeout_at(deadline, &mut task).await {
            Ok(result) => result?,
            Err(_) => {
                // Cancels the recovery only if it has not started.
                task.abort();
                Err(unconfirmed())
            }
        }
    }

    fn capability(&self, gate: &Gate) -> Value {
        #[cfg(test)]
        if gate.fixture.is_some() {
            return json!({"available":true});
        }
        let _ = gate;
        json!({"available":false,"reason":native_reason(self.native_refusal)})
    }
}

impl Drop for Supervisor {
    /// Production store users reach the store only through this supervisor,
    /// so the pinned root outlives them all. Native workers that clone the
    /// controller would break that; activation must keep this assertion.
    fn drop(&mut self) {
        debug_assert!(
            self.root_pin.is_none()
                || std::thread::panicking()
                || Arc::strong_count(&self.controller) == 1,
            "a VM store user outlived the pinned state root"
        );
    }
}

pub(crate) struct Connection(VmInventory);

impl Connection {
    pub(crate) fn inventory(&self) -> VmInventory {
        self.0.clone()
    }
}

impl Drop for Connection {
    /// Lock-free, so an aborted connection task never waits on a stuck gate.
    /// Dispatch samples the flag inside the gate: a call that already passed
    /// the sample commits and registers its worker before the gate is
    /// released, and so is tracked like any admission before the close.
    fn drop(&mut self) {
        self.0.closed.store(true, Ordering::SeqCst);
        #[cfg(test)]
        self.0
            .supervisor
            .closed_connections
            .fetch_add(1, Ordering::Relaxed);
    }
}

/// Deferred revocation stops and the generation each one stops.
#[derive(Default)]
struct StopJobs {
    set: tokio::task::JoinSet<()>,
    running: std::collections::HashMap<tokio::task::Id, u64>,
}

impl StopJobs {
    fn reap(&mut self) {
        while let Some(done) = self.set.try_join_next_with_id() {
            let id = match done {
                Ok((id, ())) => id,
                Err(error) => error.id(),
            };
            self.running.remove(&id);
        }
    }
}

/// Keeps a publication receipt in the live-worker set until its worker
/// exits, however it exits.
struct Tracked(Arc<Supervisor>, i64);

impl Drop for Tracked {
    fn drop(&mut self) {
        self.0.publisher.untrack(self.1);
    }
}

#[derive(Clone)]
pub(crate) struct VmInventory {
    supervisor: Arc<Supervisor>,
    closed: Arc<AtomicBool>,
    principal: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ToolName {
    Hosts,
    Activity,
    Catalog,
    List,
    Build,
    Create,
    Get,
    Operation,
    Start,
    Stop,
    Script,
    Extend,
    StaticCreate,
    StaticGet,
    StaticList,
    Publish,
    Unpublish,
    PublishStatus,
}

impl ToolName {
    const ALL: [Self; 18] = [
        Self::Hosts,
        Self::Activity,
        Self::Catalog,
        Self::List,
        Self::Build,
        Self::Create,
        Self::Get,
        Self::Operation,
        Self::Start,
        Self::Stop,
        Self::Script,
        Self::Extend,
        Self::StaticCreate,
        Self::StaticGet,
        Self::StaticList,
        Self::Publish,
        Self::Unpublish,
        Self::PublishStatus,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::Hosts => "list_hosts",
            Self::Activity => "list_activity",
            Self::Catalog => "vm_catalog",
            Self::List => "vm_list",
            Self::Build => "vm_template_build",
            Self::Create => "vm_create",
            Self::Get => "vm_get",
            Self::Operation => "vm_operation",
            Self::Start => "vm_start",
            Self::Stop => "vm_stop",
            Self::Script => "vm_run_script",
            Self::Extend => "vm_extend",
            Self::StaticCreate => "static_vm_create",
            Self::StaticGet => "static_vm_get",
            Self::StaticList => "static_vm_list",
            Self::Publish => "publish_site",
            Self::Unpublish => "unpublish_site",
            Self::PublishStatus => "publish_status",
        }
    }

    fn action(self) -> Option<i64> {
        Some(match self {
            Self::Build => BUILD_ACTION,
            Self::StaticCreate => STATIC_CREATE_ACTION,
            Self::Create => Action::Clone as i64,
            Self::Start => Action::Start as i64,
            Self::Stop => Action::Stop as i64,
            Self::Script => Action::Script as i64,
            Self::Extend => Action::Extend as i64,
            Self::Publish => PUBLISH_ACTION,
            Self::Unpublish => UNPUBLISH_ACTION,
            _ => return None,
        })
    }

    fn schema(self) -> Tool {
        let id = json!({"type":"string","pattern":"^[0-9a-f]{32}$"});
        let ttl = json!({"type":"integer","minimum":60,"maximum":2592000});
        let (description, properties, required): (_, _, &[&str]) = match self {
            Self::Hosts => ("List safe host facts", json!({}), &[]),
            Self::Activity => (
                "List caller-scoped VM receipts",
                json!({
                    "limit":{"type":"integer","minimum":1,"maximum":50,"default":20},
                    "before_id":{"type":"integer","minimum":1,"maximum":i64::MAX},
                    "activity_id":{"type":"integer","minimum":1,"maximum":i64::MAX},
                    "include_reads":{"type":"boolean","default":false}
                }),
                &[],
            ),
            Self::Catalog | Self::List | Self::StaticList => (
                if self == Self::Catalog {
                    "List VM recipes, templates and caller builds"
                } else if self == Self::StaticList {
                    "List caller-owned static web VMs"
                } else {
                    "List caller-owned VMs"
                },
                json!({"limit":{"type":"integer","minimum":1,"maximum":20,"default":20},
                    "cursor":{"type":"string","maxLength":96,"pattern":"^[A-Za-z0-9_-]+$"}}),
                &[],
            ),
            Self::Build => (
                "Build a pinned template",
                json!({"recipe_id":id,"request_id":id}),
                &["recipe_id", "request_id"],
            ),
            Self::Create => (
                "Create a stopped clone",
                json!({"template_id":id,"request_id":id,"ttl_s":ttl}),
                &["template_id", "request_id"],
            ),
            Self::StaticCreate => (
                "Create a stopped static Ubuntu web VM",
                json!({"template_id":id,"request_id":id,
                    "name":{"type":"string","minLength":1,"maxLength":63,
                    "pattern":"^[A-Za-z0-9](?:[A-Za-z0-9-]*[A-Za-z0-9])?$"}}),
                &["template_id", "request_id", "name"],
            ),
            Self::Publish | Self::Unpublish => (
                if self == Self::Publish {
                    "Publish an enrolled static site"
                } else {
                    "Unpublish a static site"
                },
                json!({"vm_id":id,"request_id":id}),
                &["vm_id", "request_id"],
            ),
            Self::PublishStatus => (
                "Read a caller-owned static site status",
                json!({"vm_id":id}),
                &["vm_id"],
            ),
            Self::Get | Self::StaticGet => {
                ("Read a caller-owned VM", json!({"vm_id":id}), &["vm_id"])
            }
            Self::Operation => (
                "Read a caller-owned operation",
                json!({"operation_id":id}),
                &["operation_id"],
            ),
            Self::Start => (
                "Start a caller-owned VM",
                json!({"vm_id":id,"request_id":id}),
                &["vm_id", "request_id"],
            ),
            Self::Stop => (
                "Stop a caller-owned VM",
                json!({"vm_id":id,"request_id":id}),
                &["vm_id", "request_id"],
            ),
            Self::Script => (
                "Run a bounded guest script",
                json!({"vm_id":id,"request_id":id,
                "script":{"type":"string","minLength":1,"maxLength":4096},
                "timeout_s":{"type":"integer","minimum":1,"maximum":120,"default":30}}),
                &["vm_id", "request_id", "script"],
            ),
            Self::Extend => (
                "Extend a caller-owned VM lease",
                json!({"vm_id":id,"request_id":id,"ttl_s":ttl}),
                &["vm_id", "request_id", "ttl_s"],
            ),
        };
        let mut schema = json!({"type":"object","properties":properties,"required":required,"additionalProperties":false});
        if self == Self::Activity {
            schema["not"] = json!({"required":["activity_id","before_id"]});
        }
        Tool::new(
            self.name(),
            description,
            Arc::new(schema.as_object().expect("tool schema").clone()),
        )
    }
}

impl ServerHandler for VmInventory {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
    }

    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult {
            tools: ToolName::ALL.map(ToolName::schema).into(),
            ..Default::default()
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        // Dropped after dispatch, or when the SDK drops this handler.
        #[cfg(test)]
        let hold = self
            .supervisor
            .hold
            .lock()
            .ok()
            .and_then(|mut hold| hold.take());
        #[cfg(test)]
        if let Some(hold) = &hold {
            hold.entered.notify_one();
            hold.release.notified().await;
        }
        // Dispatch takes the blocking supervisor gate and SQLite, so it runs
        // on the blocking pool: it can never occupy the scheduler that drives
        // the teardown deadlines. A dispatch the SDK stops awaiting still
        // finishes under the gate, which the close barrier waits for.
        let inventory = self.clone();
        let cancelled = context.ct.clone();
        let result = tokio::task::spawn_blocking(move || {
            inventory.dispatch(request, || cancelled.is_cancelled())
        })
        .await
        .unwrap_or(Err(STATE_UNAVAILABLE));
        let (value, error) = match result {
            Ok(mut object) => {
                object["ok"] = Value::Bool(true);
                (object, false)
            }
            Err(reason) => (json!({"ok":false,"reason":reason.0}), true),
        };
        let mut result = CallToolResult::structured(value);
        result.content.clear();
        result.is_error = Some(error);
        Ok(result.into())
    }
}

struct Args(Map<String, Value>);

impl Args {
    /// The authenticated frame pump replaces a present non-object argument
    /// value with this reserved key, so it fails as `invalid` here instead of
    /// the SDK erasing `null` into an empty object.
    fn new(arguments: Map<String, Value>) -> Self {
        Self(arguments)
    }

    fn malformed(&self) -> bool {
        self.0.contains_key(crate::local::INVALID_TOOL_ARGUMENTS)
    }

    fn optional<T: DeserializeOwned>(&mut self, name: &str) -> Outcome<Option<T>> {
        self.0
            .remove(name)
            .map(|value| {
                if value.is_null() {
                    return Err(INVALID);
                }
                serde_json::from_value(value).map_err(|_| INVALID)
            })
            .transpose()
    }

    fn get<T: DeserializeOwned>(&mut self, name: &str) -> Outcome<T> {
        self.optional(name)?.ok_or(INVALID)
    }

    fn id(&mut self, name: &str) -> Outcome<Id> {
        parse_id(&self.get::<String>(name)?)
    }

    fn end(&self) -> Outcome<()> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(INVALID)
        }
    }
}

fn parse_id(value: &str) -> Outcome<Id> {
    if value.len() != 32
        || !value
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(INVALID);
    }
    let mut id = [0; 16];
    hex::decode_to_slice(value, &mut id).map_err(|_| INVALID)?;
    Ok(id)
}

enum Work {
    Build,
    Create(u64),
    StaticCreate(StaticName),
    Publish,
    Unpublish,
    Start,
    Stop,
    Script(
        #[cfg_attr(
            not(test),
            allow(dead_code, reason = "native channel issuance remains closed")
        )]
        String,
        #[cfg_attr(
            not(test),
            allow(dead_code, reason = "native channel issuance remains closed")
        )]
        u32,
    ),
    Extend(
        #[cfg_attr(
            not(test),
            allow(dead_code, reason = "unqualified fresh extensions remain closed")
        )]
        u64,
    ),
}

struct Mutation {
    key: RequestId,
    id: Id,
    work: Work,
}

impl Mutation {
    fn parse(tool: ToolName, args: &mut Args) -> Outcome<Self> {
        let id = args.id(match tool {
            ToolName::Build => "recipe_id",
            ToolName::Create | ToolName::StaticCreate => "template_id",
            _ => "vm_id",
        })?;
        let key = RequestId::parse(&args.get::<String>("request_id")?)?;
        let work = match tool {
            ToolName::Build => Work::Build,
            ToolName::StaticCreate => Work::StaticCreate(
                StaticName::parse(&args.get::<String>("name")?).map_err(|_| INVALID)?,
            ),
            ToolName::Create | ToolName::Extend => {
                let ttl = args
                    .optional("ttl_s")?
                    .or((tool == ToolName::Create).then_some(DEFAULT_LEASE_SECONDS))
                    .filter(|ttl| (60..=2_592_000).contains(ttl))
                    .ok_or(INVALID)?;
                if tool == ToolName::Create {
                    Work::Create(ttl)
                } else {
                    Work::Extend(ttl)
                }
            }
            ToolName::Start => Work::Start,
            ToolName::Publish => Work::Publish,
            ToolName::Unpublish => Work::Unpublish,
            ToolName::Stop => Work::Stop,
            ToolName::Script => {
                let script: String = args.get("script")?;
                let timeout = args.optional("timeout_s")?.unwrap_or(30);
                if script.is_empty()
                    || script.len() > 4096
                    || script.contains('\0')
                    || !(1..=120).contains(&timeout)
                {
                    return Err(INVALID);
                }
                Work::Script(script, timeout)
            }
            _ => return Err(INVALID),
        };
        args.end()?;
        Ok(Self { key, id, work })
    }

    fn vm(&self) -> Option<Id> {
        (!matches!(
            self.work,
            Work::Build | Work::Create(_) | Work::StaticCreate(_)
        ))
        .then_some(self.id)
    }
}

impl VmInventory {
    fn dispatch(
        &self,
        request: CallToolRequestParams,
        cancelled: impl FnOnce() -> bool,
    ) -> Outcome<Value> {
        // Before the dispatch gate, the replay ledger, headroom, the pending
        // limit, audit and the effects slot: another writer holding any of
        // them cannot delay local revocation.
        self.revoke_early(&request);
        let mut gate = self.supervisor.lock()?;
        #[cfg(test)]
        let _completed = self
            .supervisor
            .stall_dispatch
            .lock()
            .ok()
            .and_then(|mut stall| stall.take())
            .map(tests::StallHook::enter);
        if gate.closed
            || self.supervisor.fenced.load(Ordering::SeqCst)
            || self.closed.load(Ordering::SeqCst)
            || cancelled()
        {
            return Err(SHUTDOWN);
        }
        let name = request.name.as_ref();
        if matches!(name, "run_command" | "discover_tools") {
            return self.refuse(HOST_REFUSAL_ACTION, None, HOST_EXECUTION_DISABLED);
        }
        if matches!(
            name,
            "static_vm_delete" | "vm_delete" | "backup_delete" | "vm_backup_delete"
        ) {
            return self.refuse(
                HOST_REFUSAL_ACTION,
                None,
                Refusal::StaticDeleteForbidden.into(),
            );
        }
        let Some(tool) = ToolName::ALL.into_iter().find(|tool| tool.name() == name) else {
            // One uncorrelated refusal under the fixed non-VM action. The
            // caller-supplied name is never stored.
            return self.refuse(HOST_REFUSAL_ACTION, None, Refusal::UnknownTool.into());
        };
        let mut args = Args::new(request.arguments.unwrap_or_default());
        if args.malformed() {
            return self.refuse(tool.action().unwrap_or(READ_REFUSAL_ACTION), None, INVALID);
        }
        if let Some(action) = tool.action() {
            // The attempted VM ID is only a candidate correlation. The audit
            // transaction keeps it only when it is owned by this caller.
            let vm = args
                .0
                .get("vm_id")
                .and_then(Value::as_str)
                .and_then(|id| parse_id(id).ok());
            let request = match Mutation::parse(tool, &mut args) {
                Ok(request) => request,
                Err(reason) => return self.refuse(action, vm, reason),
            };
            return self.mutate(&mut gate, action, request);
        }
        let vm = if matches!(
            tool,
            ToolName::Get | ToolName::StaticGet | ToolName::PublishStatus
        ) {
            args.0
                .get("vm_id")
                .and_then(Value::as_str)
                .and_then(|id| parse_id(id).ok())
        } else {
            None
        };
        match self.read(&gate, tool, args) {
            Ok(value) => Ok(value),
            Err(reason) => self.refuse(READ_REFUSAL_ACTION, vm, reason),
        }
    }

    /// File an exact, closed reason under the authenticated principal.
    fn refuse(&self, action: i64, vm: Option<Id>, reason: Fault) -> Outcome<Value> {
        if let Some(code) = refusal_from_wire(reason.0) {
            self.supervisor
                .controller
                .foreground_refusal(&self.principal, action, vm, code)??;
        }
        Err(reason)
    }

    fn mutate(&self, gate: &mut Gate, action: i64, request: Mutation) -> Outcome<Value> {
        let controller = &self.supervisor.controller;
        let recipe = hex::encode(request.id);
        let configured = self
            .supervisor
            .recipes
            .iter()
            .flatten()
            .find(|(id, _)| *id == request.id);
        let digest = configured.and_then(|(_, digest)| *digest);
        let web_digest = self
            .supervisor
            .recipes
            .iter()
            .flatten()
            .find(|(id, _)| *id == WEB_RECIPE_ID)
            .and_then(|(_, digest)| *digest);
        let subject = match &request.work {
            Work::Build => Subject::Recipe(&recipe, digest),
            Work::StaticCreate(name) => static_subject(request.id, name),
            Work::Create(ttl) => Subject::Template(
                request.id,
                Sha256::new()
                    .chain_update(b"arxcovo.clone.ttl.v1\0")
                    .chain_update(ttl.to_be_bytes())
                    .finalize()
                    .into(),
            ),
            _ => Subject::Vm(request.id),
        };
        let replay = controller.foreground_replay(&self.principal, request.key, subject, action);
        match replay {
            Ok(Ok(Some(replay))) => return Ok(receipt(replay.operation.id, replay.lease_end)),
            Ok(Ok(None)) => {}
            Ok(Err(reason)) => return self.refuse(action, request.vm(), reason.into()),
            Err(error) => return Err(error.into()),
        }
        // Scope and subject state precede native or fixture eligibility.
        let subject = match &request.work {
            Work::Build if digest.is_none() => {
                return self.refuse(
                    action,
                    None,
                    if configured.is_some() {
                        Refusal::RecipeUnqualified.into()
                    } else {
                        NOT_FOUND
                    },
                );
            }
            Work::StaticCreate(_) => {
                if let Err(reason) =
                    controller.foreground_static_template(request.id, web_digest)?
                {
                    return self.refuse(action, None, reason.into());
                }
                None
            }
            Work::Build => None,
            Work::Create(_) => Some(ForegroundSubject::Template(request.id)),
            Work::Start => Some(ForegroundSubject::Start(request.id)),
            Work::Stop => Some(ForegroundSubject::Stop(request.id)),
            Work::Script(..) => Some(ForegroundSubject::Script(request.id)),
            Work::Extend(_) => Some(ForegroundSubject::Extend(request.id)),
            Work::Publish | Work::Unpublish => return self.publication(gate, action, request),
        };
        if let Some(subject) = subject
            && let Err(reason) = controller.foreground_precheck(&self.principal, subject)?
        {
            let reason = match reason {
                Precheck::Refused(reason) => reason.into(),
                Precheck::NotReady => NOT_READY,
            };
            return self.refuse(action, request.vm(), reason);
        }
        #[cfg(test)]
        if let Some(fixture) = &mut gate.fixture {
            return fixture.admit(self, action, request, digest);
        }
        let _ = gate;
        self.refuse(
            action,
            request.vm(),
            Fault(native_reason(self.supervisor.native_refusal)),
        )
    }

    /// Owner revocation for an authenticated `unpublish_site`, with no lock
    /// beyond the activation registry's. Any such call from the principal
    /// that started an active or registered publication of the named VM
    /// cancels it and queues its connector stop at once, whatever its request
    /// key, timestamp or later ledger, pressure or audit outcome. Those decide
    /// only the response and the durable bookkeeping. Another principal never
    /// matches. A failed stop stays retryable: every later owner unpublish and
    /// reaper turn retries it.
    fn revoke_early(&self, request: &CallToolRequestParams) {
        if request.name.as_ref() != ToolName::Unpublish.name() {
            return;
        }
        let Some(vm) = request
            .arguments
            .as_ref()
            .and_then(|arguments| arguments.get("vm_id"))
            .and_then(Value::as_str)
            .and_then(|id| parse_id(id).ok())
        else {
            return;
        };
        let supervisor = &self.supervisor;
        for generation in supervisor.publisher.revoke_owned(&self.principal, vm) {
            supervisor.stop_revoked(generation);
        }
    }

    fn publication(&self, gate: &mut Gate, action: i64, request: Mutation) -> Outcome<Value> {
        let vm = request.id;
        let refuse = |reason: Refusal| self.refuse(action, Some(vm), reason.into());
        let publish = action == PUBLISH_ACTION;
        let supervisor = &self.supervisor;
        let controller = &supervisor.controller;
        if publish && !gate.allow_agent_public_sites {
            return refuse(Refusal::PublicSitesDisabled);
        }
        let mut owned = controller.foreground_static_rows(&self.principal, Some(vm), None, 1)?;
        let Some(row) = owned.pop() else {
            return refuse(Refusal::NotFound);
        };
        // A failed worker leaves its durable intent and receipt to the reaper,
        // which reconciles both. Only a live connector whose stop is still
        // unconfirmed closes publish admission, and a later confirmed stop
        // reopens it.
        while gate.publish_jobs.try_join_next().is_some() {}
        supervisor.reap_stops();
        if publish && supervisor.publisher.stop_unconfirmed() {
            return refuse(Refusal::StateUnavailable);
        }
        // Publish needs the free effects slot. Unpublish never waits here: its
        // revocation already ran before the gate, and its worker queues for
        // the slot.
        let slot = if publish {
            let Some(slot) = supervisor.publisher.claim() else {
                return refuse(Refusal::Busy);
            };
            Some(slot)
        } else {
            None
        };
        let prepared = if publish {
            let Some(profile) = gate.publish_profile.as_ref() else {
                return refuse(Refusal::PublishCredentialMissing);
            };
            if !profile.target(supervisor.host_id, &row.name) {
                return refuse(Refusal::PublishTargetMismatch);
            }
            if row.state != StaticState::Running {
                return refuse(Refusal::NotReady);
            }
            let origin: Option<Origin> = supervisor.candidate().publication_origin(vm);
            #[cfg(test)]
            let origin = gate
                .fixture
                .as_ref()
                .and_then(|fixture| fixture.publish_origin(vm))
                .or(origin);
            let Some(origin) = origin else {
                return refuse(Refusal::NotReady);
            };
            let credentials = supervisor.publisher.prepare(profile);
            let credentials = match credentials {
                Ok(credentials) => credentials,
                Err(reason) => return refuse(reason),
            };
            Some((profile.intent(vm, &origin)?, credentials, origin))
        } else {
            None
        };
        let publisher = &supervisor.publisher;
        // Registered before the admission transaction, so an owner unpublish
        // that races it still revokes this activation.
        let activation = prepared
            .is_some()
            .then(|| publisher.register(vm, &self.principal));
        #[cfg(test)]
        let _registered = supervisor
            .stall_admission
            .lock()
            .ok()
            .and_then(|mut stall| stall.take())
            .map(tests::StallHook::enter);
        let mut workers = publisher.workers();
        let admitted = controller.admit_publication(
            &self.principal,
            request.key,
            vm,
            prepared.as_ref().map(|(intent, _, _)| (intent, &row.name)),
        );
        let operation = match admitted {
            Ok(Ok(operation)) => operation,
            refused => {
                drop(workers);
                if let Some((generation, _)) = activation {
                    publisher.abandon(generation);
                }
                return match refused {
                    Ok(Err(reason)) => refuse(reason),
                    Err(error) => Err(error.into()),
                    Ok(Ok(_)) => unreachable!("admitted above"),
                };
            }
        };
        workers.insert(operation.receipt.activity_id);
        drop(workers);
        let tracked = Tracked(supervisor.clone(), operation.receipt.activity_id);
        if let Some((generation, _)) = activation {
            publisher.admitted(generation);
        } else {
            // The admitted unpublish committed `revoking`: revoke again for an
            // activation admitted between the early check and this point.
            for generation in publisher.revoke(vm) {
                supervisor.stop_revoked(generation);
            }
        }
        let supervisor = supervisor.clone();
        gate.publish_jobs.spawn(async move {
            let _tracked = tracked;
            match (prepared, activation) {
                (Some((intent, credentials, origin)), Some((generation, revoked))) => {
                    let _slot = slot;
                    supervisor
                        .publisher
                        .publish(
                            &supervisor.controller,
                            operation,
                            intent,
                            credentials,
                            origin,
                            (generation, &revoked),
                            &supervisor.cancel,
                        )
                        .await
                }
                _ => {
                    supervisor
                        .publisher
                        .unpublish(&supervisor.controller, operation, vm, &supervisor.cancel)
                        .await
                }
            }
        });
        Ok(receipt(operation.id, None))
    }

    fn read(&self, gate: &Gate, tool: ToolName, mut args: Args) -> Outcome<Value> {
        let controller = &self.supervisor.controller;
        match tool {
            ToolName::Hosts => {
                args.end()?;
                let mut host = sysinfo::System::new();
                host.refresh_cpu_all();
                host.refresh_memory();
                Ok(
                    json!({"hosts":[{"host_id":hex::encode(self.supervisor.host_id),
                    "os":std::env::consts::OS,"arch":sysinfo::System::cpu_arch(),
                    "logical_cpus":host.cpus().len(),"ram_bytes":host.total_memory(),
                    "execution_available":false,"execution_unavailable_reason":"host_execution_disabled",
                    "execution":{"available":false,"reason":"host_execution_disabled"},
                    "tool_discovery":"not_configured"}]}),
                )
            }
            ToolName::Activity => self.activity(args),
            ToolName::Catalog | ToolName::List | ToolName::StaticList => {
                self.page(gate, tool, args)
            }
            ToolName::PublishStatus => {
                let id = args.id("vm_id")?;
                args.end()?;
                let status = controller.publication_status(&self.principal, id)??;
                let mut value = json!({
                    "vm_id":hex::encode(id),
                    "phase":status.phase.map_or("unpublished", |phase| phase.wire()),
                });
                if let Some(reason) = status.reason {
                    value["reason"] = json!(reason.wire());
                }
                Ok(value)
            }
            ToolName::StaticGet => {
                let id = args.id("vm_id")?;
                args.end()?;
                let row = controller
                    .foreground_static_rows(&self.principal, Some(id), None, 1)?
                    .pop()
                    .ok_or(NOT_FOUND)?;
                Ok(static_row(row, gate))
            }
            ToolName::Get => {
                let id = args.id("vm_id")?;
                args.end()?;
                let status = controller.get_status(&self.principal, id)??;
                Ok(vm_row(status, gate, true))
            }
            ToolName::Operation => {
                let id = args.id("operation_id")?;
                args.end()?;
                Ok(operation_row(
                    controller.operation_status(&self.principal, id)??,
                ))
            }
            _ => Err(INVALID),
        }
    }

    fn activity(&self, mut args: Args) -> Outcome<Value> {
        let exact: Option<i64> = args.optional("activity_id")?;
        if args.0.values().any(Value::is_null) {
            return Err(INVALID);
        }
        let query: ListActivity =
            serde_json::from_value(Value::Object(args.0)).map_err(|_| INVALID)?;
        if query.validate().is_err() || exact.is_some_and(|id| id <= 0 || query.before_id.is_some())
        {
            return Err(INVALID);
        }
        let mut rows = self.supervisor.controller.foreground_activity(
            &self.principal,
            query.before_id,
            exact,
            query.limit + 1,
        )?;
        let mut value = json!({"activities":[]});
        if rows.len() > usize::from(query.limit) {
            rows.pop();
            value["next_before_id"] = json!(rows.last().expect("positive limit").id);
        }
        value["activities"] =
            Value::Array(rows.into_iter().map(activity_row).collect::<Outcome<_>>()?);
        Ok(value)
    }

    fn page(&self, gate: &Gate, tool: ToolName, mut args: Args) -> Outcome<Value> {
        let limit: u32 = args.optional("limit")?.unwrap_or(20);
        if !(1..=20).contains(&limit) {
            return Err(INVALID);
        }
        let cursor: Option<String> = args.optional("cursor")?;
        args.end()?;
        let after = cursor
            .as_deref()
            .map(|value| self.decode_cursor(tool, value))
            .transpose()?;
        let capability = self.supervisor.capability(gate);
        let controller = &self.supervisor.controller;
        let available = capability["available"] == true;
        let recipes = self.supervisor.recipes.map(|entry| entry.map(|(id, _)| id));
        let mut rows: Vec<((u8, Id), Value)> = if tool == ToolName::Catalog {
            controller
                .foreground_catalog(
                    &self.principal,
                    recipes,
                    after.unwrap_or((0, [0; 16])),
                    limit + 1,
                )?
                .into_iter()
                .map(|entry| {
                    let buildable = available
                        && self
                            .supervisor
                            .recipes
                            .iter()
                            .flatten()
                            .any(|(id, digest)| *id == entry.id && digest.is_some());
                    Ok(((entry.kind, entry.id), catalog_row(&entry, buildable)?))
                })
                .collect::<Outcome<_>>()?
        } else if tool == ToolName::StaticList {
            controller
                .foreground_static_rows(&self.principal, None, after.map(|(_, id)| id), limit + 1)?
                .into_iter()
                .map(|row| ((0, row.id), static_row(row, gate)))
                .collect()
        } else {
            controller
                .foreground_list(&self.principal, after.map(|(_, id)| id), limit + 1)??
                .into_iter()
                .map(|item| ((0, item.id), vm_row(item, gate, false)))
                .collect()
        };
        let mut value = json!({"arch":"x86_64","capability":capability});
        if rows.len() > limit as usize {
            rows.pop();
            value["next_cursor"] = json!(self.cursor(tool, rows.last().expect("positive limit").0));
        }
        value["entries"] = Value::Array(rows.into_iter().map(|(_, value)| value).collect());
        Ok(value)
    }

    fn cursor(&self, tool: ToolName, (kind, id): (u8, Id)) -> String {
        let mut bytes = [0; 51];
        bytes[0] = 1;
        bytes[1] = kind;
        bytes[2..18].copy_from_slice(&id);
        let tag = self.cursor_mac(tool, &bytes[..19]);
        bytes[19..].copy_from_slice(tag.as_ref());
        encode_cursor(bytes)
    }

    fn cursor_mac(&self, tool: ToolName, bytes: &[u8]) -> hmac::Tag {
        let mut mac = hmac::Context::with_key(&self.supervisor.cursor_key);
        mac.update(b"arxcovo.vm.cursor.v1\0");
        mac.update(tool.name().as_bytes());
        mac.update(&[0]);
        mac.update(self.principal.as_bytes());
        mac.update(&[0]);
        mac.update(bytes);
        mac.sign()
    }

    fn decode_cursor(&self, tool: ToolName, value: &str) -> Outcome<(u8, Id)> {
        use subtle::ConstantTimeEq;
        let bytes = decode_cursor(value)?;
        let valid_kind = if tool == ToolName::Catalog {
            (1..=3).contains(&bytes[1])
        } else {
            bytes[1] == 0
        };
        let expected = self.cursor_mac(tool, &bytes[..19]);
        if bytes[0] != 1
            || !valid_kind
            || bytes[18] != 0
            || !bool::from(expected.as_ref().ct_eq(&bytes[19..]))
        {
            return Err(INVALID_CURSOR);
        }
        Ok((
            bytes[1],
            bytes[2..18].try_into().expect("fixed cursor key width"),
        ))
    }
}

const BASE64URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

fn encode_cursor(bytes: [u8; 51]) -> String {
    let mut output = String::with_capacity(68);
    for chunk in bytes.chunks_exact(3) {
        let [a, b, c] = [chunk[0], chunk[1], chunk[2]];
        for index in [
            a >> 2,
            (a << 4 | b >> 4) & 63,
            (b << 2 | c >> 6) & 63,
            c & 63,
        ] {
            output.push(char::from(BASE64URL[index as usize]));
        }
    }
    output
}

fn decode_cursor(value: &str) -> Outcome<[u8; 51]> {
    if value.len() != 68 {
        return Err(INVALID_CURSOR);
    }
    let mut output = [0; 51];
    for (packed, chunk) in output
        .chunks_exact_mut(3)
        .zip(value.as_bytes().chunks_exact(4))
    {
        let mut digits = [0u8; 4];
        for (digit, byte) in digits.iter_mut().zip(chunk) {
            *digit = BASE64URL
                .iter()
                .position(|candidate| candidate == byte)
                .ok_or(INVALID_CURSOR)? as u8;
        }
        let [a, b, c, d] = digits;
        packed.copy_from_slice(&[a << 2 | b >> 4, b << 4 | c >> 2, c << 6 | d]);
    }
    Ok(output)
}

/// Catalog kinds are the keyset's first ordering column. Optional recipe
/// correlations are omitted; a sealed or building row always has one in v6,
/// so an absent value is a store invariant failure, not a wire `null`.
fn static_subject(template: Id, name: &StaticName) -> Subject<'static> {
    Subject::Template(
        template,
        Sha256::new()
            .chain_update(b"arxcovo.static.name.v1\0")
            .chain_update(name.as_str().as_bytes())
            .finalize()
            .into(),
    )
}

fn static_row(row: crate::store::vm::static_vm::StaticRow, gate: &Gate) -> Value {
    let state = match row.state {
        StaticState::Creating => "creating",
        StaticState::Stopped => "stopped",
        StaticState::Running => "running",
        StaticState::Recovering => "recovering",
        StaticState::Failed => "failed",
    };
    let ready = false;
    #[cfg(test)]
    let ready = ready
        || (state == "running"
            && gate
                .fixture
                .as_ref()
                .is_some_and(|fixture| fixture.web_ready(row.id)));
    let _ = gate;
    json!({"vm_id":hex::encode(row.id),"name":row.name.as_str(),"kind":"static",
        "guest":"ubuntu","arch":"x86_64","state":state,"ready":ready,
        "web_health":if ready { "healthy" } else { "unavailable" },
        "provenance":{"template_id":hex::encode(row.template),
            "source_generation":hex::encode(row.provenance.source_generation),
            "recipe_digest":hex::encode(row.provenance.recipe_digest),
            "disk_digest":hex::encode(row.provenance.disk_digest),
            "vars_digest":hex::encode(row.provenance.vars_digest)}})
}

fn catalog_row(entry: &crate::vm::CatalogEntry, buildable: bool) -> Outcome<Value> {
    let id = hex::encode(entry.id);
    Ok(
        match (
            entry.kind,
            entry.recipe.as_deref(),
            entry.guest.map(guest_wire),
        ) {
            (1, None, Some(guest)) if buildable => {
                json!({"recipe_id":id,"guest":guest,"state":"buildable"})
            }
            (1, None, Some(guest)) => {
                json!({"recipe_id":id,"guest":guest,"state":"unqualified","reason":"recipe_unqualified"})
            }
            (2, Some(recipe), Some(guest)) => {
                json!({"template_id":id,"recipe_id":recipe,"guest":guest,"state":"sealed"})
            }
            (3, Some(recipe), None) => {
                json!({"operation_id":id,"recipe_id":recipe,"state":"building"})
            }
            _ => return Err(STATE_UNAVAILABLE),
        },
    )
}

fn guest_wire(guest: Guest) -> &'static str {
    match guest {
        Guest::UbuntuX86_64 => "ubuntu",
        Guest::WindowsX86_64 => "windows",
    }
}

fn receipt(id: Id, expires: Option<i64>) -> Value {
    let mut value = json!({"operation_id":hex::encode(id)});
    if let Some(expires) = expires {
        value["expires_at_ms"] = json!(expires);
    }
    value
}

fn vm_row(status: VmStatus, gate: &Gate, get: bool) -> Value {
    let ready = status.ready;
    #[cfg(test)]
    let ready = ready
        || (status.state == "running" && gate.fixture.as_ref().is_some_and(|f| f.ready(status.id)));
    let _ = gate;
    let mut value = json!({"vm_id":hex::encode(status.id),"state":status.state,"ready":ready});
    if let Some(template) = status.template {
        value["template_id"] = json!(hex::encode(template));
    }
    if let Some(guest) = status.guest {
        value["guest"] = json!(guest_wire(guest));
    }
    if let Some(expires) = status.expires_at_ms {
        value["expires_at_ms"] = json!(expires);
    }
    if let Some(expired) = status.expired_at_ms {
        value["expired_at_ms"] = json!(expired);
    }
    if get {
        value["snapshots"] = json!([]);
        if let Some(activity) = status.cleanup_activity {
            value["cleanup_activity_id"] = json!(activity);
        }
        if let Some(cleanup) = status.cleanup.and_then(cleanup_wire) {
            value["cleanup"] = json!(cleanup);
        }
    }
    value
}

fn status_label(status: Status) -> &'static str {
    match status {
        Status::Pending => "pending",
        Status::Succeeded => "succeeded",
        Status::Failed => "failed",
        Status::Interrupted => "interrupted",
    }
}

fn operation_row(operation: OperationStatus) -> Value {
    let mut value = receipt(operation.id, operation.expires_at_ms);
    value["action"] = json!(operation.action);
    value["state"] = json!(status_label(operation.state));
    if let Some(stage) = operation.stage {
        value["stage"] = json!(stage);
    }
    if let Some(cleanup) = operation.cleanup.and_then(cleanup_wire) {
        value["cleanup"] = json!(cleanup);
    }
    if let Some(vm) = operation.vm {
        value["vm_id"] = json!(hex::encode(vm));
    }
    if let Some(recipe) = operation.recipe {
        value["recipe_id"] = json!(recipe);
    }
    if let Some(template) = operation.template {
        value["template_id"] = json!(hex::encode(template));
    }
    if let Some(result) = operation.script {
        value["script_status"] = json!(match result {
            ScriptResult::Exited(code) => {
                value["exit_code"] = json!(code);
                "exited"
            }
            ScriptResult::Unavailable => "unavailable",
            ScriptResult::TimedOut => "timed_out",
            ScriptResult::LaunchFailed => "launch_failed",
            ScriptResult::Terminated => "terminated",
        });
    }
    if let Some(reason) = operation.failure {
        value["reason"] = json!(reason.wire());
    }
    value
}

fn action_wire(code: i64) -> Option<&'static str> {
    Some(match code {
        1 => "create",
        2 => "run_script",
        3 => "start",
        4 => "stop",
        5 => "snapshot",
        6 => "restore",
        7 => "expire",
        8 => "extend",
        9 => "seal_template",
        10 => "recover",
        BUILD_ACTION => "template_build",
        STATIC_CREATE_ACTION => "static_create",
        READ_REFUSAL_ACTION => "read",
        HOST_REFUSAL_ACTION => "host_execution",
        PUBLISH_ACTION => "publish",
        UNPUBLISH_ACTION => "unpublish",
        _ => return None,
    })
}

fn activity_state_wire(code: i64) -> Option<&'static str> {
    Some(match code {
        0 => "pending",
        1 => "succeeded",
        2 => "failed",
        3 => "interrupted",
        4 => "refused",
        _ => return None,
    })
}

fn activity_row(row: crate::vm::ActivityReceipt) -> Outcome<Value> {
    let action = action_wire(row.action).ok_or(STATE_UNAVAILABLE)?;
    let state = activity_state_wire(row.state).ok_or(STATE_UNAVAILABLE)?;
    let mut value = json!({"activity_id":row.id,"action":action,"state":state,
        "identity_mode":"local_vm_principal","actor":"agent","started_at_ms":row.started_ms});
    if let Some(finished) = row.finished_ms {
        value["finished_at_ms"] = json!(finished);
    }
    if let Some(vm) = row.vm {
        value["vm_id"] = json!(hex::encode(vm));
    }
    if let Some(id) = row.operation {
        value["operation_id"] = json!(hex::encode(id));
    }
    if let Some(reason) = row.reason {
        value["reason"] = json!(refusal_wire(refusal_code(reason).ok_or(STATE_UNAVAILABLE)?));
    }
    if let Some(failure) = row.publication_failure {
        value["failure_reason"] = json!(failure.wire());
    }
    if let Some(cleanup) = row.cleanup {
        value["cleanup"] = json!(proof_wire(cleanup));
    }
    Ok(value)
}

/// Coordinator steering (2026-11-23): explicit proof maps `true` to
/// `confirmed` and `false` to `cleanup_unconfirmed`; absent proof is omitted.
fn proof_wire(confirmed: bool) -> &'static str {
    if confirmed {
        "confirmed"
    } else {
        "cleanup_unconfirmed"
    }
}

/// Closed projection of a store cleanup label. `pending` is the current VM
/// teardown state, never operation proof; any other label is omitted.
fn cleanup_wire(label: &str) -> Option<&'static str> {
    match label {
        "confirmed" => Some(proof_wire(true)),
        "unconfirmed" => Some(proof_wire(false)),
        "pending" => Some("pending"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::{Agent, Config},
        local::{self, Endpoint, Listener},
        vm::{
            guest::GuestChannel,
            qmp::Qmp,
            reaper::{CleanupWork, fixture::FixtureCustody},
        },
    };
    use anyhow::{Result, ensure};
    use rmcp::{
        RoleClient, ServiceExt, service::RunningService, transport::async_rw::AsyncRwTransport,
    };
    use std::{
        collections::{HashMap, HashSet},
        fs,
        sync::atomic::AtomicI64,
    };
    use tokio::{
        io::{
            AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, DuplexStream, ReadHalf,
            WriteHalf, split,
        },
        sync::Notify,
        task::{JoinHandle, JoinSet},
    };

    const HOST: HostObservation = HostObservation {
        owner_idle: Some(true),
        logical_cpus: Some(4),
        available_ram: Some(u64::MAX),
        capacity: Some(Capacity {
            free_bytes: u64::MAX,
            managed_bytes: 0,
        }),
    };
    const RESOURCES: Resources = Resources {
        vcpus: 1,
        guest_ram: 1024,
        overhead_ram: 1024,
    };
    const RETAINED: DiskClaim = DiskClaim {
        staging: 0,
        images: 128,
        metadata: 0,
    };
    const TRANSIENT: DiskClaim = DiskClaim {
        staging: 128,
        images: 128,
        metadata: 0,
    };
    const UTC: i64 = 1_800_000_000_000;
    const RECIPE: (Id, [u8; 32]) = ([3; 16], [4; 32]);
    const WEB_DIGEST: [u8; 32] = [17; 32];
    const WEB_RETAINED: DiskClaim = DiskClaim {
        images: 131_072,
        ..RETAINED
    };
    const WEB_TRANSIENT: DiskClaim = DiskClaim {
        images: 131_072,
        ..TRANSIENT
    };
    const WINDOWS_TRANSIENT: DiskClaim = DiskClaim {
        staging: 131_072,
        ..TRANSIENT
    };

    /// Staged fixture files with real file identities, written through the
    /// reaper's own test custody so expiry and recovery proofs are the 4c2b
    /// reaper's, not this module's. Pathname custody never qualifies native.
    #[derive(Clone)]
    struct Artifacts(Arc<FixtureCustody>);

    impl Artifacts {
        fn write(&self, slot: ArtifactSlot) -> Result<ArtifactEvidence> {
            self.stage(slot, slot.role.as_bytes())
        }

        fn stage(&self, slot: ArtifactSlot, bytes: &[u8]) -> Result<ArtifactEvidence> {
            let evidence = self.0.stage(slot, bytes)?;
            protect_fixture(&self.0.path(slot.role, slot.key), false)?;
            Ok(evidence)
        }

        /// Open the slot and prove, through the held handles, that it is the
        /// recorded object in the recorded directory with the pinned content.
        /// A same-content replacement fails the identity check. On POSIX the
        /// leaf is opened relative to the verified parent descriptor.
        fn hold(
            &self,
            slot: ArtifactSlot,
            proof: ArtifactEvidence,
            delete: bool,
        ) -> Result<custody::Held> {
            let path = self.0.path(slot.role, slot.key);
            let (parent, name) = path
                .parent()
                .zip(path.file_name())
                .ok_or_else(|| anyhow::anyhow!("fixture slot parent"))?;
            let directory = custody::open(parent, true, false)?;
            ensure!(
                custody::identity(&directory, true)?
                    == crate::vm::paths::FileIdentity {
                        volume: proof.identity.volume,
                        file: proof.directory,
                    },
                "fixture directory changed"
            );
            let mut file = custody::open_leaf(&directory, &path, name, delete)?;
            ensure!(
                custody::identity(&file, false)? == proof.identity,
                "fixture artifact identity changed"
            );
            ensure!(
                proof.pin.verify(&mut file, 131_072)?,
                "fixture artifact changed"
            );
            Ok(custody::Held {
                directory,
                file,
                name: name.to_owned(),
            })
        }

        fn verify(&self, slot: ArtifactSlot, proof: ArtifactEvidence) -> Result<()> {
            self.hold(slot, proof, false).map(drop)
        }

        /// Dispose of exactly the recorded object. Ok only when that object
        /// is proven gone; anything else leaves custody unproven and never
        /// removes a different entry.
        fn remove(&self, slot: ArtifactSlot, proof: ArtifactEvidence) -> Result<()> {
            self.remove_with(slot, proof, || {})
        }

        /// As [`Artifacts::remove`], running `between` after verification and
        /// before disposal, to witness a concurrent slot change.
        fn remove_with(
            &self,
            slot: ArtifactSlot,
            proof: ArtifactEvidence,
            between: impl FnOnce(),
        ) -> Result<()> {
            let held = self.hold(slot, proof, true)?;
            between();
            custody::dispose(&held)
        }
    }

    /// Handle-anchored identity for fixture objects, computed exactly as the
    /// fixture custody records it. Pathname checks are evidence only.
    mod custody {
        use super::*;
        use crate::vm::paths::FileIdentity;

        /// A verified leaf with its verified parent and slot name.
        pub(super) struct Held {
            #[cfg_attr(windows, allow(dead_code))]
            pub(super) directory: fs::File,
            pub(super) file: fs::File,
            #[cfg_attr(windows, allow(dead_code))]
            pub(super) name: std::ffi::OsString,
        }

        pub(super) fn open(
            path: &Path,
            directory: bool,
            exclusive: bool,
        ) -> std::io::Result<fs::File> {
            let mut options = fs::OpenOptions::new();
            options.read(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                let _ = exclusive;
                let flags = libc::O_NOFOLLOW | libc::O_NONBLOCK;
                options.custom_flags(if directory {
                    flags | libc::O_DIRECTORY
                } else {
                    flags
                });
            }
            #[cfg(windows)]
            {
                use std::os::windows::fs::OpenOptionsExt;
                use windows_sys::Win32::Storage::FileSystem::*;
                let directory_flag = if directory {
                    FILE_FLAG_BACKUP_SEMANTICS
                } else {
                    0
                };
                options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | directory_flag);
                options.share_mode(if exclusive {
                    0
                } else {
                    FILE_SHARE_READ | FILE_SHARE_WRITE
                });
            }
            options.open(path)
        }

        /// The slot's file, held exclusively. On Windows a disposing handle
        /// also carries DELETE so removal targets this exact object.
        #[cfg(windows)]
        pub(super) fn open_leaf(
            _: &fs::File,
            path: &Path,
            _: &std::ffi::OsStr,
            delete: bool,
        ) -> std::io::Result<fs::File> {
            if delete {
                use std::os::windows::fs::OpenOptionsExt;
                use windows_sys::Win32::Storage::FileSystem::*;
                return fs::OpenOptions::new()
                    .access_mode(FILE_GENERIC_READ | DELETE)
                    .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
                    .share_mode(0)
                    .open(path);
            }
            open(path, false, true)
        }

        /// The leaf, opened relative to the verified parent descriptor.
        #[cfg(unix)]
        pub(super) fn open_leaf(
            directory: &fs::File,
            _: &Path,
            name: &std::ffi::OsStr,
            _: bool,
        ) -> std::io::Result<fs::File> {
            use std::os::unix::ffi::OsStrExt;
            let name = std::ffi::CString::new(name.as_bytes())?;
            openat(directory, &name)
        }

        #[cfg(unix)]
        fn openat(directory: &fs::File, name: &std::ffi::CStr) -> std::io::Result<fs::File> {
            use std::os::unix::io::{AsRawFd, FromRawFd};
            // The directory descriptor stays open for the synchronous call.
            let fd = unsafe {
                libc::openat(
                    directory.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
                )
            };
            if fd < 0 {
                return Err(std::io::Error::last_os_error());
            }
            // The new descriptor is owned only by the returned file.
            Ok(unsafe { fs::File::from_raw_fd(fd) })
        }

        fn id(value: u64) -> Id {
            let mut id = [0; 16];
            id[8..].copy_from_slice(&value.to_be_bytes());
            id
        }

        #[cfg(unix)]
        pub(super) fn identity(file: &fs::File, directory: bool) -> Result<FileIdentity> {
            use std::os::unix::fs::MetadataExt;
            let metadata = file.metadata()?;
            ensure!(
                if directory {
                    metadata.is_dir()
                } else {
                    metadata.is_file() && metadata.nlink() == 1
                },
                "invalid fixture object"
            );
            Ok(FileIdentity {
                volume: metadata.dev(),
                file: id(metadata.ino()),
            })
        }

        #[cfg(windows)]
        pub(super) fn identity(file: &fs::File, directory: bool) -> Result<FileIdentity> {
            use std::os::windows::io::AsRawHandle;
            use windows_sys::Win32::Storage::FileSystem::*;
            let handle = file.as_raw_handle();
            let mut info = BY_HANDLE_FILE_INFORMATION::default();
            // The borrowed handle remains open for the synchronous query.
            ensure!(
                unsafe {
                    GetFileType(handle) == FILE_TYPE_DISK
                        && GetFileInformationByHandle(handle, &mut info) != 0
                },
                "invalid fixture handle"
            );
            ensure!(
                info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT == 0
                    && (info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0) == directory
                    && (directory || info.nNumberOfLinks == 1),
                "invalid fixture object"
            );
            let index = (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow);
            Ok(FileIdentity {
                volume: u64::from(info.dwVolumeSerialNumber),
                file: id(index),
            })
        }

        /// Mark the verified handle's own object for deletion. The handle is
        /// exclusive, so no other open can intervene before close.
        #[cfg(windows)]
        pub(super) fn dispose(held: &Held) -> Result<()> {
            let file = &held.file;
            use std::os::windows::io::AsRawHandle;
            use windows_sys::Win32::Storage::FileSystem::*;
            let info = FILE_DISPOSITION_INFO { DeleteFile: true };
            // The borrowed handle remains open for the synchronous call.
            ensure!(
                unsafe {
                    SetFileInformationByHandle(
                        file.as_raw_handle(),
                        FileDispositionInfo,
                        (&info as *const FILE_DISPOSITION_INFO).cast(),
                        std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32,
                    )
                } != 0,
                "fixture disposal refused"
            );
            Ok(())
        }

        /// POSIX names are not handles, so never unlink an unproven name. Move
        /// the slot entry to a fresh private name in the verified parent,
        /// prove that name is the held object, and only then unlink it. A
        /// different object is moved back if its slot is still free, and is
        /// never deleted.
        #[cfg(unix)]
        pub(super) fn dispose(held: &Held) -> Result<()> {
            use std::ffi::CString;
            use std::os::unix::{ffi::OsStrExt, fs::MetadataExt, io::AsRawFd};
            let dir = held.directory.as_raw_fd();
            let name = CString::new(held.name.as_bytes())?;
            let mut nonce = [0; 16];
            getrandom::fill(&mut nonce)
                .map_err(|error| anyhow::anyhow!("fixture disposal nonce: {error}"))?;
            let quarantine = CString::new(format!(".arx-dispose-{}", hex::encode(nonce)))?;
            // Both descriptors and names stay alive for each synchronous call.
            ensure!(
                unsafe { libc::renameat(dir, name.as_ptr(), dir, quarantine.as_ptr()) } == 0,
                "fixture disposal move: {}",
                std::io::Error::last_os_error()
            );
            let expected = held.file.metadata()?;
            let same = openat(&held.directory, &quarantine)
                .and_then(|moved| moved.metadata())
                .is_ok_and(|moved| {
                    moved.is_file()
                        && moved.nlink() == 1
                        && (moved.dev(), moved.ino()) == (expected.dev(), expected.ino())
                });
            if !same {
                let mut probe = std::mem::MaybeUninit::<libc::stat>::uninit();
                let slot_free = unsafe {
                    libc::fstatat(
                        dir,
                        name.as_ptr(),
                        probe.as_mut_ptr(),
                        libc::AT_SYMLINK_NOFOLLOW,
                    )
                } != 0;
                if slot_free {
                    unsafe { libc::renameat(dir, quarantine.as_ptr(), dir, name.as_ptr()) };
                }
                anyhow::bail!("fixture disposal target changed");
            }
            ensure!(
                unsafe { libc::unlinkat(dir, quarantine.as_ptr(), 0) } == 0,
                "fixture disposal unlink: {}",
                std::io::Error::last_os_error()
            );
            ensure!(
                held.file.metadata()?.nlink() == 0,
                "fixture disposal unproven"
            );
            Ok(())
        }
    }

    #[cfg(unix)]
    fn protect_fixture(path: &Path, directory: bool) -> Result<()> {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let mode = if directory { 0o700 } else { 0o600 };
        fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
        let metadata = fs::symlink_metadata(path)?;
        ensure!(
            metadata.is_dir() == directory
                && metadata.mode() & 0o7777 == mode
                && metadata.uid() == unsafe { libc::geteuid() },
            "static fixture permissions"
        );
        Ok(())
    }

    #[cfg(windows)]
    fn protect_fixture(path: &Path, directory: bool) -> Result<()> {
        use std::{os::windows::ffi::OsStrExt, ptr};
        use windows_sys::Win32::{
            Foundation::LocalFree,
            Security::{Authorization::*, *},
        };
        let identity = crate::local::vm_supervisor_identity()?;
        let inheritance = if directory { "OICI" } else { "" };
        let sddl: Vec<u16> = format!("O:{identity}D:P(A;{inheritance};FA;;;{identity})")
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let path_wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        // The converted descriptor owns its SID and ACL pointers through SetNamedSecurityInfoW.
        unsafe {
            let mut sd = ptr::null_mut();
            ensure!(
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    sddl.as_ptr(),
                    1,
                    &mut sd,
                    ptr::null_mut()
                ) != 0,
                "static fixture descriptor"
            );
            let (mut present, mut defaulted) = (0, 0);
            let (mut owner, mut dacl) = (ptr::null_mut(), ptr::null_mut());
            let read = GetSecurityDescriptorOwner(sd, &mut owner, &mut defaulted) != 0
                && GetSecurityDescriptorDacl(sd, &mut present, &mut dacl, &mut defaulted) != 0;
            let set = read
                && SetNamedSecurityInfoW(
                    path_wide.as_ptr(),
                    SE_FILE_OBJECT,
                    OWNER_SECURITY_INFORMATION
                        | DACL_SECURITY_INFORMATION
                        | PROTECTED_DACL_SECURITY_INFORMATION,
                    owner,
                    ptr::null_mut(),
                    dacl,
                    ptr::null_mut(),
                ) == 0;
            let mut checked = ptr::null_mut();
            let (mut checked_owner, mut checked_dacl) = (ptr::null_mut(), ptr::null_mut());
            let fetched = set
                && GetNamedSecurityInfoW(
                    path_wide.as_ptr(),
                    SE_FILE_OBJECT,
                    OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                    &mut checked_owner,
                    ptr::null_mut(),
                    &mut checked_dacl,
                    ptr::null_mut(),
                    &mut checked,
                ) == 0;
            let (mut control, mut revision) = (0, 0);
            let exact = fetched
                && !checked_owner.is_null()
                && !checked_dacl.is_null()
                && EqualSid(owner, checked_owner) != 0
                && GetSecurityDescriptorControl(checked, &mut control, &mut revision) != 0
                && control & SE_DACL_PROTECTED != 0
                && (*checked_dacl).AceCount == 1;
            if !checked.is_null() {
                LocalFree(checked);
            }
            LocalFree(sd);
            ensure!(exact, "static fixture owner/DACL");
        }
        crate::security::private(path)?;
        Ok(())
    }

    fn fixture_qcow2() -> Vec<u8> {
        let mut header = vec![0; 65_536];
        header[..4].copy_from_slice(b"QFI\xfb");
        header[4..8].copy_from_slice(&3u32.to_be_bytes());
        header[20..24].copy_from_slice(&16u32.to_be_bytes());
        header[24..32].copy_from_slice(&(1u64 << 20).to_be_bytes());
        header[36..40].copy_from_slice(&1u32.to_be_bytes());
        header[40..48].copy_from_slice(&0x3_0000u64.to_be_bytes());
        header[48..56].copy_from_slice(&0x1_0000u64.to_be_bytes());
        header[56..60].copy_from_slice(&1u32.to_be_bytes());
        header[96..100].copy_from_slice(&4u32.to_be_bytes());
        header[100..104].copy_from_slice(&112u32.to_be_bytes());
        header
    }

    /// The fixture's teardown authority for the 4c2b reaper. Release is
    /// offline only when no fixture boot or worker holds the subject.
    #[derive(Clone)]
    pub(super) struct FixtureReaper {
        live: Arc<Mutex<Live>>,
        pub(super) custody: Arc<FixtureCustody>,
    }

    impl runtime::ReaperRuntime for FixtureReaper {
        fn quiesce(&self, work: &CleanupWork, _: Instant) -> runtime::Quiesced {
            let live = self.live.lock().unwrap();
            match work.kind.vm() {
                Some(vm) if live.boots.contains_key(&vm) || live.working.contains(&vm) => {
                    runtime::Quiesced::Unconfirmed
                }
                _ => runtime::Quiesced::Offline,
            }
        }

        fn graceful_stop(&self, _: &CleanupWork, _: Instant) -> runtime::StopDisposition {
            runtime::StopDisposition::ChargedQuarantine
        }

        fn force_reap(&self, _: &CleanupWork, _: Instant) -> runtime::StopDisposition {
            runtime::StopDisposition::ChargedQuarantine
        }

        fn static_released(&self, vm: Id, _: Id, _: Instant) -> bool {
            let live = self.live.lock().unwrap();
            !live.boots.contains_key(&vm)
                && !live.working.contains(&vm)
                && !live.stuck.contains(&vm)
        }
    }

    /// Injected reaper turns for the supervised runner.
    pub(super) struct TestTicks(tokio::sync::mpsc::UnboundedReceiver<()>);

    impl ReaperTick for TestTicks {
        async fn next_tick(&mut self) -> bool {
            self.0.recv().await.is_some()
        }
    }

    type Pair = (PairSlots, [ArtifactEvidence; 2]);

    /// A published healthy boot. Channel health (the slot exists) is distinct
    /// from its one exchange slot (idle, or held by a script worker).
    enum Slot {
        Idle(Box<Boot>),
        Exchanging,
    }

    #[derive(Default)]
    struct Live {
        templates: HashMap<Id, Pair>,
        instances: HashMap<Id, Pair>,
        static_instances: HashMap<Id, (Pair, Arc<FixtureOrigin>)>,
        boots: HashMap<Id, Slot>,
        /// VMs with a registered, not yet joined fixture worker.
        working: HashSet<Id>,
        /// Static VMs whose boot release this fixture refuses to prove.
        stuck: HashSet<Id>,
    }

    /// Injected static copy faults, consumed by the next static create.
    #[derive(Clone, Copy)]
    pub(super) enum StaticFault {
        /// The disk leaf is written, then the VARS copy fails. The worker
        /// disposes of its own identified disk only when `dispose` is set.
        AfterDisk { dispose: bool },
        /// The disk leaf is written and the VARS write is interrupted after
        /// creating a partial file whose identity is not captured.
        Interrupted,
        /// Both leaves are written, then the disk moves aside and an
        /// identical replacement takes its slot before the first boot.
        ReplaceBeforeBoot,
    }

    pub(super) struct Fixture {
        jobs: JoinSet<Result<()>>,
        live: Arc<Mutex<Live>>,
        artifacts: Artifacts,
        build_hold: Option<Arc<Notify>>,
        script_hold: Option<Arc<Notify>>,
        static_hold: Option<Arc<Notify>>,
        static_fault: Option<StaticFault>,
        windows_recipe: bool,
        host: HostObservation,
        /// A cancelled build worker reports failure, to witness drain errors.
        fail_cancelled: bool,
        failed: bool,
    }

    pub(super) struct FixtureDrain {
        jobs: JoinSet<Result<()>>,
        live: Arc<Mutex<Live>>,
        failed: bool,
    }

    impl FixtureDrain {
        pub(super) async fn join(mut self) -> Result<Self> {
            while let Some(result) = self.jobs.join_next().await {
                self.failed |= !matches!(result, Ok(Ok(())));
            }
            ensure!(!self.failed, "fixture job failed");
            Ok(self)
        }

        /// Release idle boots once every worker holding a slot is joined.
        pub(super) async fn release(self) -> Result<()> {
            let boots = std::mem::take(&mut self.live.lock().unwrap().boots);
            for slot in boots.into_values() {
                if let Slot::Idle(boot) = slot {
                    (*boot).release().await?;
                }
            }
            Ok(())
        }
    }

    impl Fixture {
        fn new(root: &Path) -> Self {
            Self {
                jobs: JoinSet::new(),
                live: Arc::new(Mutex::new(Live::default())),
                artifacts: Artifacts(Arc::new(FixtureCustody::open(root).unwrap())),
                build_hold: None,
                script_hold: None,
                static_hold: None,
                static_fault: None,
                windows_recipe: false,
                host: HOST,
                fail_cancelled: false,
                failed: false,
            }
        }

        pub(super) fn ready(&self, vm: Id) -> bool {
            self.live.lock().unwrap().boots.contains_key(&vm)
        }

        pub(super) fn publish_origin(&self, vm: Id) -> Option<Origin> {
            let live = self.live.lock().unwrap();
            match live.boots.get(&vm) {
                Some(Slot::Idle(boot)) => boot.publish_origin.clone(),
                _ => None,
            }
        }

        pub(super) fn web_ready(&self, vm: Id) -> bool {
            let live = self.live.lock().unwrap();
            match live.boots.get(&vm) {
                Some(Slot::Idle(boot)) => boot.origin.as_ref().is_some_and(|claim| claim.verify()),
                Some(Slot::Exchanging) => live.static_instances.contains_key(&vm),
                None => false,
            }
        }

        pub(super) fn reaper(&self) -> FixtureReaper {
            FixtureReaper {
                live: self.live.clone(),
                custody: self.artifacts.0.clone(),
            }
        }

        fn reap(&mut self) {
            while let Some(result) = self.jobs.try_join_next() {
                self.failed |= !matches!(result, Ok(Ok(())));
            }
        }

        pub(super) fn take_jobs(&mut self) -> FixtureDrain {
            FixtureDrain {
                jobs: std::mem::take(&mut self.jobs),
                live: self.live.clone(),
                failed: self.failed,
            }
        }

        pub(super) fn admit(
            &mut self,
            inventory: &VmInventory,
            action: i64,
            mutation: Mutation,
            digest: Option<[u8; 32]>,
        ) -> Outcome<Value> {
            self.reap();
            if self.failed {
                return Err(STATE_UNAVAILABLE);
            }
            if self.jobs.len() >= 8 {
                return inventory.refuse(action, mutation.vm(), Refusal::PendingFull.into());
            }
            if let Work::Script(..) = mutation.work {
                // The controller already proved a running owned generation
                // with no pending operation; this checks the live channel.
                match self.live.lock().unwrap().boots.get(&mutation.id) {
                    Some(Slot::Idle(_)) => {}
                    Some(Slot::Exchanging) => {
                        return inventory.refuse(action, mutation.vm(), Refusal::Busy.into());
                    }
                    None => return inventory.refuse(action, mutation.vm(), NOT_READY),
                }
            }
            let controller = inventory.supervisor.controller.clone();
            let cancel = inventory.supervisor.cancel.clone();
            let principal = &inventory.principal;
            let Mutation { key, id, work } = mutation;
            let live = self.live.clone();
            let artifacts = self.artifacts.clone();
            let mut registry = self.live.lock().unwrap();
            let (operation, vm) = match work {
                Work::Build => {
                    let recipe = hex::encode(id);
                    let web = id == WEB_RECIPE_ID;
                    let windows = id == Controller::windows_recipe_id();
                    let (retained, transient) = if web {
                        (WEB_RETAINED, WEB_TRANSIENT)
                    } else if windows {
                        (RETAINED, WINDOWS_TRANSIENT)
                    } else {
                        (RETAINED, TRANSIENT)
                    };
                    let spec = BuildRequest {
                        principal,
                        key,
                        recipe: &recipe,
                        digest: digest.ok_or(NOT_FOUND)?,
                        retained,
                        transient,
                    };
                    let (slots, worker, reference) = match controller.admit_build(
                        spec,
                        HOST,
                        RESOURCES,
                        |template, slots| {
                            let (saved, [disk, vars]) = *registry.templates.get(&template)?;
                            if saved != slots {
                                return None;
                            }
                            artifacts.verify(slots.disk, disk).ok()?;
                            artifacts.verify(slots.vars, vars).ok()?;
                            Some([disk, vars])
                        },
                    )?? {
                        BuildAdmission::Fresh(slots, worker, reference) => {
                            (*slots, worker, reference)
                        }
                        BuildAdmission::Existing(reference) => {
                            return Ok(receipt(reference.id, None));
                        }
                    };
                    let principal = principal.clone();
                    let digest = spec.digest;
                    let hold = self.build_hold.take();
                    let fail_cancelled = self.fail_cancelled;
                    self.jobs.spawn(async move {
                        if !released(hold, &cancel).await {
                            controller.fail_build(&worker, slots, true)?;
                            ensure!(!fail_cancelled, "fixture worker failed");
                            return Ok(());
                        }
                        let spec = BuildRequest {
                            principal: &principal,
                            key,
                            recipe: &recipe,
                            digest,
                            retained,
                            transient,
                        };
                        if windows {
                            let mut registry = live.lock().unwrap();
                            let sealed = controller.finish_windows_fixture_build(
                                spec,
                                (slots, worker, reference),
                                &|slot, bytes| artifacts.stage(slot, bytes),
                                &|slot, proof| artifacts.verify(slot, proof),
                                &|slot, proof| artifacts.remove(slot, proof),
                            )?;
                            registry.templates.insert(slots.id, sealed);
                            return Ok(());
                        }
                        let [disk, vars, seed, overlay, verify_vars] = [
                            slots.pair.disk,
                            slots.pair.vars,
                            slots.seed,
                            slots.verification.disk,
                            slots.verification.vars,
                        ];
                        let mut proof = Vec::new();
                        for slot in [disk, vars, seed, overlay, verify_vars] {
                            let file = if web && slot == disk {
                                artifacts.stage(slot, &fixture_qcow2())?
                            } else {
                                artifacts.write(slot)?
                            };
                            controller.build_evidence(
                                &worker,
                                slots.id,
                                &[(slot, None), (slot, Some(file))],
                            )?;
                            proof.push(file);
                        }
                        for (slot, file) in
                            [seed, overlay, verify_vars].into_iter().zip(&proof[2..])
                        {
                            artifacts.remove(slot, *file)?;
                        }
                        let pair = [proof[0], proof[1]];
                        let mut registry = live.lock().unwrap();
                        ensure!(
                            controller.finish_build(&worker, slots, spec, pair)?
                                == Status::Succeeded,
                            "fixture build interrupted"
                        );
                        registry.templates.insert(slots.id, (slots.pair, pair));
                        Ok(())
                    });
                    return Ok(receipt(reference.id, None));
                }
                Work::StaticCreate(ref name) => {
                    use crate::vm::static_web::FullCopyReservation;
                    let (golden, proof) = registry
                        .templates
                        .get(&id)
                        .copied()
                        .ok_or(STATE_UNAVAILABLE)?;
                    let disk = FullCopyReservation {
                        qcow2_bytes: proof[0].pin.size,
                        qcow2_growth_bytes: 1 << 20,
                        vars_bytes: proof[1].pin.size,
                        staging_bytes: 65_536,
                        metadata_bytes: 4096,
                    }
                    .checked_claim()
                    .ok_or(Fault("disk"))?;
                    let binding = crate::store::vm::Binding {
                        principal,
                        key,
                        subject: static_subject(id, name),
                        action,
                    };
                    let (vm, slots, worker, reference) = match controller.admit_static(
                        binding,
                        name,
                        id,
                        (golden, proof),
                        WEB_DIGEST,
                        disk,
                        self.host,
                        RESOURCES,
                    )? {
                        Ok(admitted) => admitted,
                        Err(reason) => return inventory.refuse(action, None, reason.into()),
                    };
                    let hold = self.static_hold.take();
                    let fault = self.static_fault.take();
                    self.jobs.spawn(async move {
                        let copied = if released(hold, &cancel).await {
                            copy_and_provision(&artifacts, (golden, proof), slots, fault, &cancel)
                                .await
                        } else {
                            // Cancelled before any effect on the candidate.
                            Err(CopyFailure::untouched(FailureReason::Cancelled))
                        };
                        let outcome = copied
                            .as_ref()
                            .map(|(pair, _)| *pair)
                            .map_err(|failure| *failure);
                        let mut registry = live.lock().unwrap();
                        let status = controller.finish_static(&worker, slots, outcome)?;
                        if status == Status::Succeeded
                            && let Ok((pair, origin)) = copied
                        {
                            registry
                                .static_instances
                                .insert(vm, ((slots, pair), Arc::new(origin)));
                        }
                        Ok(())
                    });
                    return Ok(receipt(reference.id, None));
                }
                Work::Create(ttl) => {
                    // The controller precheck proved the sealed template.
                    let (golden, proof) = registry
                        .templates
                        .get(&id)
                        .copied()
                        .ok_or(STATE_UNAVAILABLE)?;
                    artifacts.verify(golden.disk, proof[0])?;
                    artifacts.verify(golden.vars, proof[1])?;
                    let request = Request {
                        principal,
                        request_id: key,
                        vm: [0; 16],
                        disk: RETAINED,
                        operation: Operation::Clone {
                            template: id,
                            lease_seconds: Some(ttl),
                        },
                    };
                    let (vm, slots, worker, reference) = match controller
                        .admit_linked_clone(request, HOST, RESOURCES, golden, proof)??
                    {
                        CloneAdmission::Fresh(vm, slots, worker, reference) => {
                            (vm, slots, worker, reference)
                        }
                        CloneAdmission::Existing(reference) => {
                            return Ok(receipt(
                                reference.id,
                                controller
                                    .operation_status(principal, reference.id)??
                                    .expires_at_ms,
                            ));
                        }
                    };
                    let expires = controller
                        .operation_status(principal, reference.id)??
                        .expires_at_ms;
                    self.jobs.spawn(async move {
                        let pair = [artifacts.write(slots.disk)?, artifacts.write(slots.vars)?];
                        let mut registry = live.lock().unwrap();
                        ensure!(
                            controller.finish_linked_clone(&worker, slots, Some(pair))?
                                == Status::Succeeded,
                            "fixture clone interrupted"
                        );
                        registry.instances.insert(vm, (slots, pair));
                        Ok(())
                    });
                    return Ok(receipt(reference.id, expires));
                }
                Work::Extend(ttl) => {
                    let end = controller.extend_lease(principal, id, key, ttl)??;
                    let replay = controller
                        .foreground_replay(principal, key, Subject::Vm(id), Action::Extend as i64)??
                        .ok_or(STATE_UNAVAILABLE)?;
                    return Ok(receipt(replay.operation.id, Some(end)));
                }
                Work::Publish | Work::Unpublish => {
                    unreachable!("publication has separate admission")
                }
                Work::Start => (Operation::Start, id),
                Work::Stop => (Operation::Stop, id),
                Work::Script(ref script, seconds) => (
                    Operation::Script {
                        script,
                        timeout_seconds: u64::from(seconds),
                    },
                    id,
                ),
            };
            let request = Request {
                principal,
                request_id: key,
                vm,
                operation,
                disk: DiskClaim::default(),
            };
            if matches!(work, Work::Start)
                && !registry.static_instances.contains_key(&vm)
                && !controller
                    .foreground_static_rows(principal, Some(vm), None, 1)?
                    .is_empty()
            {
                // Rehydrate the committed pair from durable custody records;
                // the boot re-verifies both files before launch. The fixture
                // origin key is in memory only, so a fresh one is issued
                // for the same generation name.
                match controller.static_custody(principal, vm)? {
                    Some(pair) => {
                        let origin = FixtureOrigin::new(pair.0.disk.generation)
                            .map_err(|_| STATE_UNAVAILABLE)?;
                        registry
                            .static_instances
                            .insert(vm, (pair, Arc::new(origin)));
                    }
                    None => return inventory.refuse(action, Some(vm), Refusal::NotReady.into()),
                }
            }
            let mut worker = match controller.admit(request, self.host, RESOURCES)? {
                Admission::Admitted(worker) => worker,
                Admission::Existing(prior) => {
                    return Ok(receipt(
                        controller.foreground_operation_ref(principal, prior.activity_id)?,
                        None,
                    ));
                }
                Admission::Refused(reason) => return Err(reason.into()),
            };
            let activity = worker.activity_id();
            // Registered synchronously with the committed admission; the
            // guard leaves the reaper's offline proof closed until joined.
            registry.working.insert(vm);
            let working = Working(live.clone(), vm);
            match work {
                Work::Start => {
                    let controller = controller.clone();
                    let static_pair = registry.static_instances.get(&vm).cloned();
                    self.jobs.spawn(async move {
                        let _working = working;
                        let boot = match static_pair {
                            Some((pair, identity)) => {
                                Boot::start_static(&artifacts, pair, &identity, &cancel).await
                            }
                            None => Boot::start(&cancel).await,
                        };
                        match boot {
                            Ok(boot) => {
                                // Publish ready only if this worker's settlement
                                // won; a worker that lost to expiry or shutdown
                                // is reaped without a ready boot.
                                let settled = {
                                    let mut registry = live.lock().unwrap();
                                    let status =
                                        controller.finish(&worker, Completion::Succeeded)?;
                                    if status == Status::Succeeded {
                                        registry.boots.insert(vm, Slot::Idle(Box::new(boot)));
                                        None
                                    } else {
                                        Some(boot)
                                    }
                                };
                                if let Some(boot) = settled {
                                    boot.release().await?;
                                }
                            }
                            Err(_) => {
                                controller.finish_with_cause(
                                    &worker,
                                    Completion::CleanupUnconfirmed,
                                    Some(FailureReason::BootFailed),
                                )?;
                            }
                        }
                        Ok(())
                    });
                }
                Work::Stop => {
                    let boot = match registry.boots.remove(&vm) {
                        Some(Slot::Idle(boot)) => Some(*boot),
                        Some(Slot::Exchanging) | None => None,
                    };
                    if let Some(origin) =
                        boot.as_ref().and_then(|boot| boot.publish_origin.as_ref())
                    {
                        origin.retired.cancel();
                    }
                    let supervisor = inventory.supervisor.clone();
                    let controller = controller.clone();
                    self.jobs.spawn(async move {
                        let _working = working;
                        let local = supervisor.publisher.stop_local(Some(vm)).await;
                        // A failed powerdown is a known dirty stop; a missing
                        // boot leaves the cause unknown.
                        let (completion, cause) = match boot {
                            Some(boot) => match boot.stop().await {
                                Ok(()) => (Completion::Succeeded, None),
                                Err(_) => (
                                    Completion::CleanupUnconfirmed,
                                    Some(FailureReason::DirtyStop),
                                ),
                            },
                            None => (Completion::CleanupUnconfirmed, None),
                        };
                        controller.finish_with_cause(&worker, completion, cause)?;
                        if local {
                            Ok(())
                        } else {
                            anyhow::bail!("publication stop unconfirmed")
                        }
                    });
                }
                Work::Script(script, seconds) => {
                    let Some(Slot::Idle(boot)) = registry.boots.insert(vm, Slot::Exchanging) else {
                        unreachable!("idle channel checked under the fixture gate");
                    };
                    let mut boot = *boot;
                    let hold = self.script_hold.take();
                    let supervisor = inventory.supervisor.clone();
                    worker.deadline += Duration::from_secs(6);
                    let controller = controller.clone();
                    self.jobs.spawn(async move {
                        let _working = working;
                        let result = if released(hold, &cancel).await {
                            boot.channel.run_script(&cancel, script, seconds).await.ok()
                        } else {
                            None
                        };
                        let result = result.and_then(|(status, code)| guest_result(status, code));
                        // A trustworthy result that won settlement returns the
                        // exchange slot; anything else retires the boot.
                        let retired = match result {
                            Some(result) => {
                                let mut registry = live.lock().unwrap();
                                let status = controller.finish_script(&worker, result)?;
                                if matches!(status, Status::Succeeded | Status::Failed) {
                                    registry.boots.insert(vm, Slot::Idle(Box::new(boot)));
                                    None
                                } else {
                                    registry.boots.remove(&vm);
                                    Some(boot)
                                }
                            }
                            None => {
                                // The guest result was lost: execution unknown.
                                live.lock().unwrap().boots.remove(&vm);
                                controller.finish_with_cause(
                                    &worker,
                                    Completion::CleanupUnconfirmed,
                                    Some(FailureReason::ExecutionUnknown),
                                )?;
                                Some(boot)
                            }
                        };
                        if let Some(boot) = retired {
                            if let Some(origin) = &boot.publish_origin {
                                origin.retired.cancel();
                            }
                            supervisor.publisher.stop_local(Some(vm)).await;
                            boot.release().await?;
                        }
                        Ok(())
                    });
                }
                _ => unreachable!("handled before VM admission"),
            }
            Ok(receipt(
                controller.foreground_operation_ref(principal, activity)?,
                None,
            ))
        }
    }

    /// Copy the sealed fixture pair into the candidate's exclusive leaves and
    /// run its private first boot. A failure reports what each candidate
    /// slot is proven to hold; this worker may dispose only of an object it
    /// created and identified itself.
    async fn copy_and_provision(
        artifacts: &Artifacts,
        (golden, proofs): Pair,
        slots: PairSlots,
        fault: Option<StaticFault>,
        cancel: &Shutdown,
    ) -> Result<([ArtifactEvidence; 2], FixtureOrigin), CopyFailure> {
        use crate::vm::CandidateSlot::{Absent, Retained, Unknown};
        let failed = |slots| CopyFailure {
            reason: FailureReason::CloneFailed,
            slots,
        };
        let bytes = |slot: ArtifactSlot, proof: ArtifactEvidence| -> Result<Vec<u8>> {
            use std::io::{Read, Seek};
            let path = artifacts.0.path(slot.role, slot.key);
            ensure!(
                fs::symlink_metadata(&path)?.is_file(),
                "invalid static fixture source"
            );
            let mut file = fs::File::open(&path)?;
            ensure!(
                proof.pin.verify(&mut file, 131_072)?,
                "static fixture source changed"
            );
            file.rewind()?;
            let mut content = Vec::new();
            file.take(131_073).read_to_end(&mut content)?;
            ensure!(
                content.len() as u64 == proof.pin.size
                    && Sha256::digest(&content)[..] == proof.pin.sha256,
                "invalid static fixture length or digest"
            );
            Ok(content)
        };
        // Source reads and header checks precede every destination effect.
        let source = (|| {
            let disk = bytes(golden.disk, proofs[0])?;
            let vars = bytes(golden.vars, proofs[1])?;
            crate::vm::qemu_img::inspect_header(&disk, None)
                .map_err(|_| anyhow::anyhow!("invalid static fixture image"))?;
            Ok::<_, anyhow::Error>((disk, vars))
        })();
        let Ok((disk, vars)) = source else {
            return Err(CopyFailure::untouched(FailureReason::CloneFailed));
        };
        // An exclusive create that fails may still have left an object.
        let Ok(disk_file) = artifacts.stage(slots.disk, &disk) else {
            return Err(failed([Unknown, Absent]));
        };
        match fault {
            Some(StaticFault::AfterDisk { dispose }) => {
                let disk_slot = if dispose && artifacts.remove(slots.disk, disk_file).is_ok() {
                    Absent
                } else {
                    Retained(disk_file)
                };
                return Err(failed([disk_slot, Absent]));
            }
            Some(StaticFault::Interrupted) => {
                use std::io::Write;
                let path = artifacts.0.path(slots.vars.role, slots.vars.key);
                if let Ok(mut file) = fs::File::create_new(path) {
                    let _ = file.write_all(&vars[..vars.len() / 2]);
                }
                return Err(failed([Retained(disk_file), Unknown]));
            }
            Some(StaticFault::ReplaceBeforeBoot) | None => {}
        }
        let Ok(vars_file) = artifacts.stage(slots.vars, &vars) else {
            return Err(failed([Retained(disk_file), Unknown]));
        };
        let files = [disk_file, vars_file];
        let committed = [Retained(disk_file), Retained(vars_file)];
        if let Some(StaticFault::ReplaceBeforeBoot) = fault {
            let path = artifacts.0.path(slots.disk.role, slots.disk.key);
            let _ = fs::rename(&path, path.with_file_name("moved-original"));
            let _ = artifacts.stage(slots.disk, &disk);
        }
        // The first private boot uses the same recorded-pair check as a
        // later start: identity and content through held handles.
        for (slot, proof) in [(slots.disk, disk_file), (slots.vars, vars_file)] {
            artifacts
                .verify(slot, proof)
                .map_err(|_| failed(committed))?;
        }
        let boot = Boot::start(cancel).await.map_err(|_| CopyFailure {
            reason: FailureReason::BootFailed,
            slots: committed,
        })?;
        let origin = FixtureOrigin::new(slots.disk.generation);
        // Finish the private boot even when the synthetic provisioning fails.
        boot.stop().await.map_err(|_| CopyFailure {
            reason: FailureReason::DirtyStop,
            slots: committed,
        })?;
        let origin = origin.map_err(|_| CopyFailure {
            reason: FailureReason::HealthFailed,
            slots: committed,
        })?;
        Ok((files, origin))
    }

    /// Marks a VM as held by a fixture worker until the worker finishes.
    struct Working(Arc<Mutex<Live>>, Id);

    impl Drop for Working {
        fn drop(&mut self) {
            if let Ok(mut live) = self.0.lock() {
                live.working.remove(&self.1);
            }
        }
    }

    async fn released(hold: Option<Arc<Notify>>, cancel: &Shutdown) -> bool {
        if let Some(hold) = hold {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => return false,
                _ = hold.notified() => {}
            }
        }
        !cancel.is_cancelled()
    }

    fn guest_result(status: arx_guest::ScriptStatus, code: Option<u32>) -> Option<ScriptResult> {
        use arx_guest::ScriptStatus::*;
        Some(match status {
            Exited => ScriptResult::Exited(code?),
            Unavailable => ScriptResult::Unavailable,
            TimedOut => ScriptResult::TimedOut,
            LaunchFailed => ScriptResult::LaunchFailed,
            Terminated => ScriptResult::Terminated,
        })
    }

    type Monitor = Qmp<ReadHalf<DuplexStream>, WriteHalf<DuplexStream>>;
    struct Boot {
        channel: GuestChannel<DuplexStream>,
        qmp: Monitor,
        peer: JoinHandle<Result<()>>,
        guest: JoinHandle<Result<()>>,
        origin: Option<crate::vm::static_web::OriginPortLease>,
        publish_origin: Option<Origin>,
    }

    /// Synthetic guest endpoint and independently configured host trust.
    /// This fixture has no production serial encoder, and never logs key bytes.
    struct FixtureOrigin {
        name: String,
        guest: Arc<rustls::ServerConfig>,
        client: Arc<rustls::ClientConfig>,
    }

    impl FixtureOrigin {
        fn new(generation: Id) -> Result<Self> {
            use crate::vm::static_web::StaticOriginIdentity;
            use rcgen::{
                BasicConstraints, CertificateParams, ExtendedKeyUsagePurpose, IsCa, Issuer,
                KeyPair, KeyUsagePurpose,
            };
            use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
            let plan = StaticOriginIdentity::for_generation(generation).provisioning();
            let name = plan.server_name().to_owned();
            let now = time::OffsetDateTime::now_utc();
            let mut ca = CertificateParams::default();
            ca.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
            ca.key_usages = vec![KeyUsagePurpose::KeyCertSign];
            ca.not_before = now - time::Duration::minutes(1);
            ca.not_after = now + time::Duration::days(1);
            let ca_key = KeyPair::generate()?;
            let certificate = ca.self_signed(&ca_key)?;
            let issuer = Issuer::new(ca, ca_key);
            let mut leaf = CertificateParams::new(vec![name.clone()])?;
            leaf.not_before = now - time::Duration::minutes(1);
            leaf.not_after = now + time::Duration::days(1);
            leaf.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
            let key = KeyPair::generate()?;
            let cert = leaf.signed_by(&key, &issuer)?;
            let provider = Arc::new(rustls::crypto::ring::default_provider());
            let guest = rustls::ServerConfig::builder_with_provider(provider.clone())
                .with_safe_default_protocol_versions()?
                .with_no_client_auth()
                .with_single_cert(
                    vec![cert.der().clone()],
                    PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der())),
                )?;
            let mut roots = rustls::RootCertStore::empty();
            roots.add(certificate.der().clone())?;
            let client = rustls::ClientConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()?
                .with_root_certificates(roots)
                .with_no_client_auth();
            Ok(Self {
                name,
                guest: Arc::new(guest),
                client: Arc::new(client),
            })
        }

        async fn probe(
            &self,
            lease: &crate::vm::static_web::OriginPortLease,
            expected: String,
        ) -> Result<()> {
            let listener = tokio::net::TcpListener::from_std(lease.fixture_listener()?)?;
            let address = listener.local_addr()?;
            let tls = tokio_rustls::TlsAcceptor::from(self.guest.clone());
            let connector = tokio_rustls::TlsConnector::from(self.client.clone());
            const REQUEST: &[u8] =
                b"GET /arx-health HTTP/1.1\r\nHost: fixture\r\nConnection: close\r\n\r\n";
            const RESPONSE: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\nok\n";
            let guest = async {
                let (socket, _) = listener.accept().await?;
                let mut stream = tls.accept(socket).await?;
                let mut request = [0; REQUEST.len()];
                stream.read_exact(&mut request).await?;
                ensure!(request == REQUEST, "invalid fixture health request");
                stream.write_all(RESPONSE).await?;
                Ok::<_, anyhow::Error>(())
            };
            let host = async {
                let socket = tokio::net::TcpStream::connect(address).await?;
                let mut stream = connector.connect(expected.try_into()?, socket).await?;
                stream.write_all(REQUEST).await?;
                let mut response = [0; RESPONSE.len()];
                stream.read_exact(&mut response).await?;
                ensure!(response == RESPONSE, "invalid fixture health response");
                Ok::<_, anyhow::Error>(())
            };
            tokio::time::timeout(Duration::from_secs(1), async {
                tokio::try_join!(host, guest)
            })
            .await??;
            Ok(())
        }
    }

    impl Boot {
        async fn start_static(
            artifacts: &Artifacts,
            (slots, [disk, vars]): Pair,
            identity: &FixtureOrigin,
            cancel: &Shutdown,
        ) -> Result<Self> {
            for (slot, proof) in [(slots.disk, disk), (slots.vars, vars)] {
                artifacts.verify(slot, proof)?;
            }
            let origin = crate::vm::static_web::OriginPortLease::acquire()?;
            ensure!(origin.verify(), "static origin listener mismatch");
            let mut boot = Self::start(cancel).await?;
            if identity
                .probe(&origin, identity.name.clone())
                .await
                .is_err()
            {
                boot.release().await?;
                anyhow::bail!("static fixture TLS proof failed");
            }
            boot.publish_origin = Some(Origin {
                generation: slots.disk.generation,
                boot: crate::store::vm::new_id()?,
                port: origin.port().try_into()?,
                ca_path: artifacts.0.path("origin_ca", slots.disk.generation),
                ca_receipt: crate::vm::paths::ArtifactPin {
                    size: 1,
                    sha256: [1; 32],
                },
                retired: Shutdown::default(),
            });
            boot.origin = Some(origin);
            Ok(boot)
        }

        async fn start(cancel: &Shutdown) -> Result<Self> {
            let (host, mut peer) = tokio::io::duplex(4096);
            let (serial, guest) = tokio::io::duplex(8192);
            let peer = tokio::spawn(async move {
                let [major, minor, micro] = crate::vm::qemu_args::QEMU_VERSION
                    .split('.')
                    .map(|part| part.parse::<u32>().unwrap())
                    .collect::<Vec<_>>()[..]
                else {
                    unreachable!("pinned QEMU version is major.minor.micro");
                };
                let greeting = json!({"QMP":{"version":{"qemu":{"major":major,"minor":minor,"micro":micro},"package":""},"capabilities":[]}});
                peer.write_all(format!("{greeting}\n").as_bytes()).await?;
                let mut peer = BufReader::new(peer);
                for (id, command, result) in [
                    (1, "qmp_capabilities", json!({})),
                    (
                        2,
                        "query-status",
                        json!({"status":"prelaunch","running":false}),
                    ),
                    (3, "cont", json!({})),
                    (4, "system_powerdown", json!({})),
                ] {
                    let mut line = String::new();
                    if peer.read_line(&mut line).await? == 0 {
                        return Ok(());
                    }
                    ensure!(
                        serde_json::from_str::<Value>(&line)? == json!({"execute":command,"id":id}),
                        "unexpected fixture QMP command"
                    );
                    send(&mut peer, json!({"return":result,"id":id})).await?;
                    if id == 3 {
                        send(&mut peer, json!({"event":"VSERPORT_CHANGE","data":{"id":crate::vm::qemu_args::GUEST_PORT_ID,"open":true}})).await?;
                    }
                }
                send(
                    &mut peer,
                    json!({"event":"SHUTDOWN","data":{"guest":true,"reason":"guest-shutdown"}}),
                )
                .await
            });
            let guest = tokio::spawn(guest_peer(guest));
            let (read, write) = split(host);
            let started = async {
                let mut qmp = Qmp::handshake(read, write, cancel).await?;
                qmp.release_startup_hold(cancel, || true).await?;
                let channel = GuestChannel::ready(serial, &mut qmp, cancel).await?;
                Ok::<_, crate::vm::qmp::ChannelError>((qmp, channel))
            }
            .await;
            match started {
                Ok((qmp, channel)) => Ok(Self {
                    channel,
                    qmp,
                    peer,
                    guest,
                    origin: None,
                    publish_origin: None,
                }),
                Err(_) => {
                    peer.await??;
                    guest.await??;
                    anyhow::bail!("fixture boot unconfirmed")
                }
            }
        }

        async fn release(self) -> Result<()> {
            if let Some(origin) = &self.publish_origin {
                origin.retired.cancel();
            }
            drop(self.qmp);
            drop(self.channel);
            self.peer.await??;
            self.guest.await??;
            Ok(())
        }

        async fn stop(self) -> Result<()> {
            let stopped = self.qmp.powerdown(&Shutdown::default()).await;
            drop(self.channel);
            self.peer.await??;
            self.guest.await??;
            ensure!(stopped.is_ok(), "fixture stop unconfirmed");
            Ok(())
        }
    }

    async fn send(peer: &mut BufReader<DuplexStream>, value: Value) -> Result<()> {
        Ok(peer
            .get_mut()
            .write_all(format!("{value}\n").as_bytes())
            .await?)
    }

    async fn guest_peer(mut stream: DuplexStream) -> Result<()> {
        use arx_guest::{Request, Response, ScriptExecution, ScriptStatus};
        while let Ok(length) = stream.read_u32().await {
            ensure!(
                length as usize <= arx_guest::MAX_FRAME,
                "fixture frame length"
            );
            let mut payload = vec![0; length as usize + 4];
            payload[..4].copy_from_slice(&length.to_be_bytes());
            stream.read_exact(&mut payload[4..]).await?;
            let request = arx_guest::read_frame::<Request>(&mut payload.as_slice())?
                .expect("fixture request");
            let response = match request {
                Request::Hello { version, challenge } => Response::HelloAck {
                    version,
                    challenge,
                    script_execution: ScriptExecution::Available,
                },
                Request::RunScript { id, script, .. }
                    if script == "exit 7" || script == "exit 0" =>
                {
                    Response::ScriptResult {
                        id,
                        status: ScriptStatus::Exited,
                        exit_code: Some(if script == "exit 7" { 7 } else { 0 }),
                    }
                }
                _ => anyhow::bail!("unexpected fixture guest request"),
            };
            let mut frame = Vec::new();
            arx_guest::write_frame(&mut frame, &response)?;
            stream.write_all(&frame).await?;
        }
        Ok(())
    }

    fn key(number: u64) -> String {
        format!("{UTC:016x}{number:016x}")
    }

    fn request(name: &str, arguments: Value) -> CallToolRequestParams {
        CallToolRequestParams::new(name.to_owned())
            .with_arguments(arguments.as_object().unwrap().clone())
    }

    type Client = RunningService<RoleClient, ()>;

    async fn call(client: &Client, name: &str, args: Value) -> Value {
        let result = client.call_tool(request(name, args)).await.unwrap();
        let row = result.structured_content.expect("one structured result");
        assert!(result.content.is_empty());
        assert_eq!(result.is_error, Some(row["ok"] == false));
        row
    }

    async fn terminal(client: &Client, reference: &Value) -> Value {
        assert_eq!(reference["ok"], true, "{reference}");
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let result = call(
                    client,
                    "vm_operation",
                    json!({"operation_id":reference["operation_id"]}),
                )
                .await;
                assert_eq!(result["ok"], true, "{result}");
                if result["state"] != "pending" {
                    return result;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("operation did not settle")
    }

    /// Parks one tool call between SDK dispatch and admission. `done` fires
    /// when the parked handler finishes or the SDK drops it.
    pub(super) struct Hold {
        pub(super) entered: Arc<Notify>,
        pub(super) release: Arc<Notify>,
        done: Option<tokio::sync::oneshot::Sender<()>>,
    }

    impl Drop for Hold {
        fn drop(&mut self) {
            if let Some(done) = self.done.take() {
                let _ = done.send(());
            }
        }
    }

    type Raw = BufReader<Box<dyn Stream>>;
    trait Stream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send {}
    impl<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send> Stream for T {}

    async fn wire(raw: &mut Raw, value: Value) {
        raw.get_mut()
            .write_all(format!("{value}\n").as_bytes())
            .await
            .unwrap();
    }

    async fn reply(raw: &mut Raw, id: u64) -> Value {
        loop {
            let mut line = String::new();
            assert!(
                raw.read_line(&mut line).await.unwrap() > 0,
                "session closed"
            );
            let value: Value = serde_json::from_str(&line).unwrap();
            if value["id"] == id {
                return value;
            }
        }
    }

    struct Harness {
        temp: tempfile::TempDir,
        media: std::path::PathBuf,
        now: Arc<AtomicI64>,
        supervisor: Option<Arc<Supervisor>>,
        endpoint: Endpoint,
        tokens: [String; 2],
        ticks: tokio::sync::mpsc::UnboundedSender<()>,
        trip: Arc<Notify>,
        interrupt: Option<tokio::sync::oneshot::Sender<()>>,
        server: Option<JoinHandle<Result<()>>>,
        hold_build: Arc<Notify>,
        hold_script: Arc<Notify>,
    }

    impl Harness {
        async fn start(fail_cancelled: bool, interrupt_on_fatal_accept: bool) -> Self {
            Self::start_with_web(fail_cancelled, interrupt_on_fatal_accept, false).await
        }

        async fn start_with_web(
            fail_cancelled: bool,
            interrupt_on_fatal_accept: bool,
            web: bool,
        ) -> Self {
            Self::start_configured(fail_cancelled, interrupt_on_fatal_accept, web, |_| {}).await
        }

        async fn start_configured(
            fail_cancelled: bool,
            interrupt_on_fatal_accept: bool,
            web: bool,
            configure: impl FnOnce(&mut Fixture),
        ) -> Self {
            let temp = crate::service::application::tests::private_temp();
            let media = temp.path().join("fixture-media");
            fs::create_dir(&media).unwrap();
            protect_fixture(&media, true).unwrap();
            let now = Arc::new(AtomicI64::new(UTC));
            Self::launch(
                (temp, media, now),
                fail_cancelled,
                interrupt_on_fatal_accept,
                web,
                configure,
            )
            .await
        }

        /// Open a supervisor, fixture and server over existing state and
        /// fixture media, as a fresh foreground process would.
        async fn launch(
            (temp, media, now): (tempfile::TempDir, std::path::PathBuf, Arc<AtomicI64>),
            fail_cancelled: bool,
            interrupt_on_fatal_accept: bool,
            web: bool,
            configure: impl FnOnce(&mut Fixture),
        ) -> Self {
            let mut supervisor = Supervisor::new(
                temp.path(),
                VmPolicy::default(),
                ForegroundClock {
                    utc: Some(now.clone()),
                },
                Some(RECIPE),
                NativeRefusal::QualificationMissing,
                None,
            )
            .unwrap();
            if web {
                Arc::get_mut(&mut supervisor).unwrap().recipes[1] =
                    Some((WEB_RECIPE_ID, Some(WEB_DIGEST)));
            }
            let hold_build = Arc::new(Notify::new());
            let hold_script = Arc::new(Notify::new());
            let mut fixture = Fixture::new(&media);
            fixture.build_hold = Some(hold_build.clone());
            fixture.script_hold = Some(hold_script.clone());
            fixture.fail_cancelled = fail_cancelled;
            configure(&mut fixture);
            if fixture.windows_recipe {
                Arc::get_mut(&mut supervisor).unwrap().recipes[2] = Some((
                    Controller::windows_recipe_id(),
                    Some(Controller::windows_fixture_digest()),
                ));
            }
            supervisor.gate.lock().unwrap().fixture = Some(fixture);
            let (ticks, receiver) = tokio::sync::mpsc::unbounded_channel();
            *supervisor.ticks.lock().unwrap() = Some(TestTicks(receiver));
            supervisor.startup_sweep().unwrap();
            let endpoint = Endpoint {
                #[cfg(unix)]
                address: temp.path().join("vm.sock").to_str().unwrap().into(),
                #[cfg(windows)]
                address: format!(
                    r"\\.\pipe\arxcovo-mcp-{}",
                    hex::encode(crate::store::vm::new_id().unwrap())
                ),
                client_identity: local::vm_supervisor_identity().unwrap(),
            };
            let tokens = ["a".repeat(64), "b".repeat(64)];
            let config = Arc::new(Config {
                agents: ["alpha", "beta"]
                    .into_iter()
                    .zip(&tokens)
                    .map(|(principal, token)| Agent {
                        principal: principal.into(),
                        token_sha256: hex::encode(Sha256::digest(token.as_bytes())),
                    })
                    .collect(),
                ..Config::default()
            });
            let trip = Arc::new(Notify::new());
            let (interrupt, interrupted) = tokio::sync::oneshot::channel::<()>();
            // Optionally race an interrupt that fires once a failed listener
            // has fenced admission. The listener then completes only after
            // teardown stops it, so the interrupt deterministically wins.
            let fatal = interrupt_on_fatal_accept.then(|| {
                let (ack, observed) = tokio::sync::oneshot::channel();
                *supervisor.fatal_accept.lock().unwrap() = Some(ack);
                observed
            });
            let server = tokio::spawn(local::serve_vm_profile(
                local::TripAcceptor::new(Listener::bind(&endpoint).unwrap(), trip.clone()),
                config,
                supervisor.clone(),
                async move {
                    match fatal {
                        Some(observed) => observed.await.unwrap(),
                        None => {
                            let _ = interrupted.await;
                        }
                    }
                    Ok(())
                },
            ));
            Self {
                temp,
                media,
                now,
                supervisor: Some(supervisor),
                endpoint,
                tokens,
                ticks,
                trip,
                interrupt: Some(interrupt),
                server: Some(server),
                hold_build,
                hold_script,
            }
        }

        fn supervisor(&self) -> &Arc<Supervisor> {
            self.supervisor.as_ref().unwrap()
        }

        /// Interrupt the server and return its drain and recovery result.
        /// Callers drop their clients first.
        async fn shutdown(&mut self) -> Result<()> {
            self.interrupt.take().unwrap().send(()).unwrap();
            let result = self.server.take().unwrap().await.unwrap();
            self.supervisor = None;
            result
        }

        /// A new foreground process over the same state and fixture media.
        async fn reopen(self, configure: impl FnOnce(&mut Fixture)) -> Self {
            assert!(self.server.is_none(), "shut down before reopening");
            Self::launch(
                (self.temp, self.media, self.now),
                false,
                false,
                true,
                configure,
            )
            .await
        }

        fn fixture<T>(&self, run: impl FnOnce(&mut Fixture) -> T) -> T {
            run(self
                .supervisor()
                .gate
                .lock()
                .unwrap()
                .fixture
                .as_mut()
                .unwrap())
        }

        async fn connect(&self, token: &str) -> Result<Box<dyn Stream>> {
            let stream = local::connect_vm_fixture(
                &self.endpoint.address,
                &self.endpoint.client_identity,
                token,
            )
            .await?;
            Ok(Box::new(stream))
        }

        async fn client(&self, which: usize) -> Client {
            let (read, write) = split(self.connect(&self.tokens[which]).await.unwrap());
            ().serve(AsyncRwTransport::new_client(read, write))
                .await
                .unwrap()
        }

        async fn raw(&self) -> Raw {
            let mut stream = BufReader::new(self.connect(&self.tokens[0]).await.unwrap());
            wire(
                &mut stream,
                json!({"jsonrpc":"2.0","method":"initialize","id":0,"params":{
                "protocolVersion":"2025-06-18","capabilities":{},
                "clientInfo":{"name":"fixture","version":"1"}}}),
            )
            .await;
            assert!(reply(&mut stream, 0).await["result"]["capabilities"]["tools"].is_object());
            wire(
                &mut stream,
                json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            )
            .await;
            stream
        }

        fn hold(&self) -> (Arc<Notify>, Arc<Notify>, tokio::sync::oneshot::Receiver<()>) {
            let (entered, release) = (Arc::new(Notify::new()), Arc::new(Notify::new()));
            let (done, finished) = tokio::sync::oneshot::channel();
            *self.supervisor().hold.lock().unwrap() = Some(Hold {
                entered: entered.clone(),
                release: release.clone(),
                done: Some(done),
            });
            (entered, release, finished)
        }

        /// Alpha's audit and receipt rows, read through the controller.
        fn audit(&self) -> Vec<crate::vm::ActivityReceipt> {
            self.supervisor()
                .controller
                .foreground_activity("alpha", None, None, 100)
                .unwrap()
        }

        async fn tick(&self) {
            self.ticks.send(()).unwrap();
        }
    }

    /// Every file under the state root, including SQLite journal files.
    fn state_bytes(root: &std::path::Path) -> Vec<(std::path::PathBuf, Vec<u8>)> {
        let mut files = vec![];
        let mut directories = vec![root.to_path_buf()];
        while let Some(directory) = directories.pop() {
            for entry in fs::read_dir(directory).unwrap() {
                let entry = entry.unwrap();
                if entry.file_type().unwrap().is_dir() {
                    directories.push(entry.path());
                } else if let Ok(bytes) = fs::read(entry.path()) {
                    files.push((entry.path(), bytes));
                }
            }
        }
        files
    }

    #[tokio::test]
    async fn cloudflare_publication_follows_foreground_policy_origin_and_dns() {
        use crate::vm::runtime::publication::Fake;
        use std::sync::atomic::Ordering::SeqCst;

        let mut h = Harness::start_with_web(false, false, true).await;
        let a = h.client(0).await;
        let b = h.client(1).await;
        let build = call(
            &a,
            "vm_template_build",
            json!({
                "recipe_id":hex::encode(WEB_RECIPE_ID), "request_id":key(601)
            }),
        )
        .await;
        h.hold_build.notify_one();
        let template = terminal(&a, &build).await["template_id"].clone();
        let site = call(
            &a,
            "static_vm_create",
            json!({
                "name":"tunnel-site","template_id":template,"request_id":key(602)
            }),
        )
        .await;
        let vm = terminal(&a, &site).await["vm_id"].clone();
        let clone = call(
            &a,
            "vm_create",
            json!({
                "template_id":template,"request_id":key(603)
            }),
        )
        .await;
        let test = terminal(&a, &clone).await["vm_id"].clone();
        let args = |n| json!({"vm_id":vm,"request_id":key(n)});
        let status = json!({"vm_id":vm});
        assert_eq!(
            call(&a, "publish_site", args(604)).await["reason"],
            "public_sites_disabled"
        );
        let fake = Arc::new(Fake::default());
        {
            let supervisor = h.supervisor();
            *supervisor.publisher.fake.lock().unwrap() = Some(fake.clone());
            let mut gate = supervisor.gate.lock().unwrap();
            gate.allow_agent_public_sites = true;
            gate.publish_profile = Some(Arc::new(Profile::synthetic(
                supervisor.host_id,
                "wrong-site",
            )));
        }
        assert_eq!(
            call(&a, "publish_site", args(605)).await["reason"],
            "publish_target_mismatch"
        );
        assert_eq!(
            call(&b, "publish_status", status.clone()).await["reason"],
            "not_found"
        );
        assert_eq!(
            call(
                &a,
                "publish_site",
                json!({"vm_id":test,"request_id":key(606)})
            )
            .await["reason"],
            "not_found"
        );
        assert_eq!(
            call(
                &a,
                "publish_site",
                json!({
                    "vm_id":vm,"request_id":key(607),"hostname":"injection.invalid"
                })
            )
            .await["reason"],
            "invalid"
        );
        {
            let supervisor = h.supervisor();
            supervisor.gate.lock().unwrap().publish_profile = Some(Arc::new(Profile::synthetic(
                supervisor.host_id,
                "tunnel-site",
            )));
        }
        assert_eq!(
            call(&a, "publish_site", args(608)).await["reason"],
            "not_ready"
        );
        let start = call(&a, "vm_start", args(609)).await;
        assert_eq!(terminal(&a, &start).await["state"], "succeeded");
        assert_eq!(
            call(&a, "publish_site", args(610)).await["reason"],
            "publish_credential_missing"
        );
        fake.credential.store(true, SeqCst);
        fake.foreign.store(true, SeqCst);
        let conflict = call(&a, "publish_site", args(611)).await;
        let result = terminal(&a, &conflict).await;
        assert_eq!(result["reason"], "dns_conflict", "{result}");
        assert_eq!(fake.posts.load(SeqCst), 0);
        assert!(!fake.local.load(SeqCst));
        fake.foreign.store(false, SeqCst);

        let published = call(&a, "publish_site", args(612)).await;
        assert_eq!(terminal(&a, &published).await["state"], "succeeded");
        assert_eq!(
            call(&a, "publish_status", status.clone()).await["phase"],
            "published"
        );
        assert_eq!(call(&a, "publish_site", args(612)).await, published);
        assert_eq!(
            call(&a, "unpublish_site", args(612)).await["reason"],
            "key_collision"
        );
        // The ledger decides only the response: an owner unpublish, even with
        // a colliding key, closes ingress at once.
        assert_eq!(fake.posts.load(SeqCst), 1);
        until(|| !fake.local.load(SeqCst)).await;
        let ingress = fake.ingress.lock().unwrap().clone().unwrap();
        assert!(ingress.contains("hostname: 'publish.example.invalid'"));
        assert!(ingress.contains("noTLSVerify: false"));
        assert!(ingress.ends_with("  - service: http_status:404\n"));

        fake.credential.store(false, SeqCst);
        let revoke = call(&a, "unpublish_site", args(613)).await;
        let result = terminal(&a, &revoke).await;
        assert_eq!(result["state"], "failed");
        assert!(!fake.local.load(SeqCst));
        assert!(fake.record.load(SeqCst));
        assert_eq!(
            call(&a, "publish_status", status.clone()).await["phase"],
            "cleanup_pending"
        );

        fake.credential.store(true, SeqCst);
        let revoke = call(&a, "unpublish_site", args(614)).await;
        assert_eq!(terminal(&a, &revoke).await["state"], "succeeded");
        assert_eq!(fake.deletes.load(SeqCst), 1);
        assert_eq!(
            call(&a, "publish_status", status.clone()).await["phase"],
            "unpublished"
        );
        let repeated = call(&a, "unpublish_site", args(615)).await;
        assert_eq!(terminal(&a, &repeated).await["state"], "succeeded");
        assert_eq!(fake.deletes.load(SeqCst), 1);

        // Stop must terminate the local connector even if the Cloudflare
        // POST cannot return. An observed owned record is reconciled once.
        let (entered, in_post) = tokio::sync::oneshot::channel();
        let (_release, hold) = tokio::sync::oneshot::channel();
        *fake.post_gate.lock().unwrap() = Some((entered, hold));
        let pending = call(&a, "publish_site", args(616)).await;
        tokio::time::timeout(Duration::from_secs(2), in_post)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            call(&a, "publish_status", status.clone()).await["phase"],
            "publishing"
        );
        let stop = call(&a, "vm_stop", args(617)).await;
        assert_eq!(terminal(&a, &stop).await["state"], "succeeded");
        assert_eq!(terminal(&a, &pending).await["state"], "failed");
        assert!(!fake.local.load(SeqCst));
        assert_eq!(fake.deletes.load(SeqCst), 2);
        assert_eq!(
            call(&a, "publish_status", status.clone()).await["phase"],
            "unpublished"
        );

        let start = call(&a, "vm_start", args(618)).await;
        assert_eq!(terminal(&a, &start).await["state"], "succeeded");
        let published = call(&a, "publish_site", args(619)).await;
        assert_eq!(terminal(&a, &published).await["state"], "succeeded");
        fake.health.store(1, SeqCst);
        h.tick().await;
        let exited = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let row = call(&a, "publish_status", status.clone()).await;
                if row["phase"] == "unpublished" {
                    break row;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(exited["reason"], "connector_exited");
        assert_eq!(fake.deletes.load(SeqCst), 3);
        assert!(!fake.local.load(SeqCst));
        let audit = call(&a, "list_activity", json!({"limit":50}))
            .await
            .to_string();
        for secret in [
            "publish.example.invalid",
            "dns.dpapi",
            &"b".repeat(32),
            &"e".repeat(32),
        ] {
            assert!(!audit.contains(secret));
        }
        for tool in ["static_vm_delete", "backup_delete"] {
            assert_eq!(
                call(&a, tool, status.clone()).await["reason"],
                "static_delete_forbidden"
            );
        }
        h.shutdown().await.unwrap();
    }

    /// A running static VM enrolled for publication with a fake connector and
    /// DNS API holding valid synthetic credentials. Keys `base..base+3`.
    async fn enrolled(
        h: &Harness,
        a: &Client,
        base: u64,
    ) -> (Value, Arc<crate::vm::runtime::publication::Fake>) {
        use std::sync::atomic::Ordering::SeqCst;
        let build = call(
            a,
            "vm_template_build",
            json!({"recipe_id":hex::encode(WEB_RECIPE_ID), "request_id":key(base)}),
        )
        .await;
        h.hold_build.notify_one();
        let template = terminal(a, &build).await["template_id"].clone();
        let site = call(
            a,
            "static_vm_create",
            json!({"name":"tunnel-site","template_id":template,"request_id":key(base + 1)}),
        )
        .await;
        let vm = terminal(a, &site).await["vm_id"].clone();
        let start = call(
            a,
            "vm_start",
            json!({"vm_id":vm,"request_id":key(base + 2)}),
        )
        .await;
        assert_eq!(terminal(a, &start).await["state"], "succeeded");
        let fake = Arc::new(crate::vm::runtime::publication::Fake::default());
        fake.credential.store(true, SeqCst);
        enroll(
            h,
            &fake,
            true,
            Profile::synthetic(h.supervisor().host_id, "tunnel-site"),
        );
        (vm, fake)
    }

    fn enroll(
        h: &Harness,
        fake: &Arc<crate::vm::runtime::publication::Fake>,
        allow: bool,
        profile: Profile,
    ) {
        let supervisor = h.supervisor();
        *supervisor.publisher.fake.lock().unwrap() = Some(fake.clone());
        let mut gate = supervisor.gate.lock().unwrap();
        gate.allow_agent_public_sites = allow;
        gate.publish_profile = Some(Arc::new(profile));
    }

    async fn until(mut done: impl FnMut() -> bool) {
        tokio::time::timeout(Duration::from_secs(2), async {
            while !done() {
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .expect("condition not reached");
    }

    async fn phase(a: &Client, vm: &Value) -> Value {
        call(a, "publish_status", json!({"vm_id":vm})).await
    }

    async fn settle(h: &Harness, a: &Client, vm: &Value) -> Value {
        h.tick().await;
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let row = phase(a, vm).await;
                if row["phase"] == "unpublished" {
                    break row;
                }
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .expect("publication did not settle")
    }

    #[tokio::test]
    async fn owner_unpublish_stops_local_ingress_before_dns_slot_and_store_pressure() {
        use std::sync::atomic::Ordering::SeqCst;
        let mut h = Harness::start_with_web(false, false, true).await;
        let a = h.client(0).await;
        let b = h.client(1).await;
        let (vm, fake) = enrolled(&h, &a, 700).await;
        let args = |n| json!({"vm_id":vm,"request_id":key(n)});

        // A held DNS POST keeps the effects slot. Owner revocation still wins.
        let (entered, in_post) = tokio::sync::oneshot::channel();
        let (release, hold) = tokio::sync::oneshot::channel();
        *fake.post_gate.lock().unwrap() = Some((entered, hold));
        let pending = call(&a, "publish_site", args(710)).await;
        tokio::time::timeout(Duration::from_secs(2), in_post)
            .await
            .unwrap()
            .unwrap();
        assert!(fake.local.load(SeqCst));
        // A foreign caller can neither see nor revoke the publication.
        assert_eq!(
            call(&b, "unpublish_site", args(711)).await["reason"],
            "not_found"
        );
        assert!(fake.local.load(SeqCst));
        let revoke = call(&a, "unpublish_site", args(712)).await;
        assert_eq!(revoke["ok"], true, "{revoke}");
        until(|| !fake.local.load(SeqCst)).await;
        assert_eq!(fake.posts.load(SeqCst), 1);
        assert_eq!(phase(&a, &vm).await["phase"], "revoking");
        release.send(()).unwrap();
        let failed = terminal(&a, &pending).await;
        assert_eq!(failed["state"], "failed");
        assert_eq!(failed["reason"], "cancelled", "{failed}");
        assert_eq!(terminal(&a, &revoke).await["state"], "succeeded");
        assert_eq!(phase(&a, &vm).await["phase"], "unpublished");
        assert_eq!((fake.posts.load(SeqCst), fake.deletes.load(SeqCst)), (1, 1));
        assert!(!fake.record.load(SeqCst));

        // Exhausted store headroom refuses the unpublish after stopping ingress.
        let published = call(&a, "publish_site", args(713)).await;
        assert_eq!(terminal(&a, &published).await["state"], "succeeded");
        assert!(fake.local.load(SeqCst));
        let store = h.supervisor().controller.store_for_test();
        store
            .transact(|db| {
                db.execute_batch("CREATE TABLE pressure(filler BLOB) STRICT")?;
                while crate::store::vm::headroom(db)? {
                    db.execute("INSERT INTO pressure VALUES(zeroblob(2048))", [])?;
                }
                Ok(())
            })
            .unwrap();
        let refused = call(&a, "unpublish_site", args(714)).await;
        assert_eq!(refused["reason"], "state_headroom", "{refused}");
        until(|| !fake.local.load(SeqCst)).await;
        assert!(fake.record.load(SeqCst));
        store
            .transact(|db| Ok(db.execute_batch("DROP TABLE pressure")?))
            .unwrap();
        // The reaper completes the owner's revocation from the durable intent.
        assert_eq!(settle(&h, &a, &vm).await["reason"], "cancelled");
        assert_eq!((fake.posts.load(SeqCst), fake.deletes.load(SeqCst)), (2, 2));

        // A saturated pending limit refuses after stopping ingress.
        let published = call(&a, "publish_site", args(715)).await;
        assert_eq!(terminal(&a, &published).await["state"], "succeeded");
        assert!(fake.local.load(SeqCst));
        let store = h.supervisor().controller.store_for_test();
        store
            .transact(|db| {
                for _ in 0..crate::store::vm::MAX_CALLER_PENDING {
                    db.execute(
                        "INSERT INTO activity(principal,actor,vm,action,started_ms)
                         VALUES('alpha','agent',randomblob(16),2,0)",
                        [],
                    )?;
                }
                Ok(())
            })
            .unwrap();
        let refused = call(&a, "unpublish_site", args(716)).await;
        assert_eq!(refused["reason"], "pending_full", "{refused}");
        until(|| !fake.local.load(SeqCst)).await;
        store
            .transact(|db| {
                db.execute(
                    "DELETE FROM activity WHERE status=0 AND action=2 AND principal='alpha'",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        settle(&h, &a, &vm).await;
        assert_eq!((fake.posts.load(SeqCst), fake.deletes.load(SeqCst)), (3, 3));

        // An acknowledged record ID survives a failed verification lookup, so
        // cleanup settles even though the record vanished before reconciliation.
        fake.lose_after_post.store(true, SeqCst);
        let lost = call(&a, "publish_site", args(717)).await;
        let lost = terminal(&a, &lost).await;
        assert_eq!(lost["reason"], "dns_unavailable", "{lost}");
        assert_eq!(lost["cleanup"], "confirmed", "{lost}");
        assert_eq!(phase(&a, &vm).await["phase"], "unpublished");
        assert!(!fake.local.load(SeqCst));
        assert_eq!((fake.posts.load(SeqCst), fake.deletes.load(SeqCst)), (4, 3));
        drop((a, b));
        h.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn publication_ledger_survives_restart_drift_and_a_late_post() {
        use std::sync::atomic::Ordering::SeqCst;
        let mut h = Harness::start_with_web(false, false, true).await;
        let a = h.client(0).await;
        let (vm, fake) = enrolled(&h, &a, 800).await;
        let args = |n| json!({"vm_id":vm,"request_id":key(n)});
        let host = h.supervisor().host_id;
        let original = Profile::synthetic(host, "tunnel-site");
        let intent = |h: &Harness| h.supervisor().controller.publication_intent().unwrap();

        // Shutdown after durable admission and before any external effect.
        let (entered, in_preflight) = tokio::sync::oneshot::channel();
        let (_release, hold) = tokio::sync::oneshot::channel();
        *fake.preflight_gate.lock().unwrap() = Some((entered, hold));
        let pending = call(&a, "publish_site", args(810)).await;
        assert_eq!(pending["ok"], true, "{pending}");
        tokio::time::timeout(Duration::from_secs(2), in_preflight)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(phase(&a, &vm).await["phase"], "publishing");
        drop(a);
        h.shutdown().await.unwrap();
        let mut h = h.reopen(|_| {}).await;
        enroll(&h, &fake, true, Profile::synthetic(host, "tunnel-site"));
        let a = h.client(0).await;
        assert!(intent(&h).is_none());
        assert_eq!(phase(&a, &vm).await["phase"], "unpublished");
        assert_ne!(terminal(&a, &pending).await["state"], "succeeded");
        assert_eq!(fake.posts.load(SeqCst), 0);
        assert!(!fake.local.load(SeqCst));

        // Restart after DNS acknowledgement while cleanup credentials are gone.
        let start = call(&a, "vm_start", args(811)).await;
        assert_eq!(terminal(&a, &start).await["state"], "succeeded");
        let published = call(&a, "publish_site", args(812)).await;
        assert_eq!(terminal(&a, &published).await["state"], "succeeded");
        let before = intent(&h).unwrap();
        assert!(before.record.is_some());
        fake.credential.store(false, SeqCst);
        drop(a);
        h.shutdown().await.unwrap();
        assert!(!fake.local.load(SeqCst));
        assert!(fake.record.load(SeqCst));

        // A drifted owner binding with policy off: nothing republishes and
        // the original route stays pinned.
        let mut h = h.reopen(|_| {}).await;
        enroll(
            &h,
            &fake,
            false,
            Profile::drifted(
                host,
                "tunnel-site",
                "drift.example.invalid",
                &"d".repeat(32),
            ),
        );
        let a = h.client(0).await;
        let after = intent(&h).unwrap();
        assert_eq!(
            (
                &after.nonce,
                after.generation,
                after.boot,
                after.revision,
                &after.zone,
                after.hostname.as_str(),
                &after.credential_ref,
                &after.record,
            ),
            (
                &before.nonce,
                before.generation,
                before.boot,
                before.revision,
                &before.zone,
                before.hostname.as_str(),
                &before.credential_ref,
                &before.record,
            )
        );
        assert_eq!(phase(&a, &vm).await["phase"], "cleanup_pending");
        let revoke = call(&a, "unpublish_site", args(813)).await;
        assert_eq!(terminal(&a, &revoke).await["state"], "failed");
        assert_eq!(phase(&a, &vm).await["phase"], "cleanup_pending");
        let start = call(&a, "vm_start", args(814)).await;
        assert_eq!(terminal(&a, &start).await["state"], "succeeded");
        fake.credential.store(true, SeqCst);
        h.supervisor().gate.lock().unwrap().allow_agent_public_sites = true;
        assert_eq!(
            call(&a, "publish_site", args(815)).await["reason"],
            "publish_cleanup_pending"
        );
        fake.routes.lock().unwrap().clear();
        settle(&h, &a, &vm).await;
        let routes = fake.routes.lock().unwrap().clone();
        assert!(!routes.is_empty());
        let pinned = format!("{} publish.example.invalid ", "b".repeat(32));
        assert!(
            routes.iter().all(|route| route.starts_with(&pinned)),
            "{routes:?}"
        );
        assert_eq!((fake.posts.load(SeqCst), fake.deletes.load(SeqCst)), (1, 1));
        assert!(!fake.local.load(SeqCst));

        // A POST that commits only after the first absence observation.
        enroll(&h, &fake, true, original);
        let (late, committed) = tokio::sync::oneshot::channel();
        *fake.late_commit.lock().unwrap() = Some(committed);
        let pending = call(&a, "publish_site", args(816)).await;
        until(|| fake.posts.load(SeqCst) == 2).await;
        let before = intent(&h).unwrap();
        assert!(before.post_possible && before.record.is_none());
        let starts = fake.starts.load(SeqCst);
        drop(a);
        h.shutdown().await.unwrap();
        assert!(!fake.record.load(SeqCst));
        assert!(!fake.local.load(SeqCst));
        // Policy on and a drifted owner binding: the possible POST keeps the
        // whole original route pinned and blocks any fresh publication.
        let mut h = h.reopen(|_| {}).await;
        enroll(
            &h,
            &fake,
            true,
            Profile::drifted(
                host,
                "tunnel-site",
                "drift.example.invalid",
                &"d".repeat(32),
            ),
        );
        let a = h.client(0).await;
        assert_ne!(terminal(&a, &pending).await["state"], "succeeded");
        let pinned = |intent: &crate::store::vm::publish::Intent| {
            (
                intent.nonce.clone(),
                intent.generation,
                intent.boot,
                intent.revision,
                intent.zone.clone(),
                intent.hostname.as_str().to_owned(),
                intent.tunnel.clone(),
                intent.credential_ref.clone(),
                intent.record.clone(),
                intent.post_possible,
            )
        };
        assert_eq!(pinned(&intent(&h).unwrap()), pinned(&before));
        let start = call(&a, "vm_start", args(817)).await;
        assert_eq!(terminal(&a, &start).await["state"], "succeeded");
        assert_eq!(
            call(&a, "publish_site", args(818)).await["reason"],
            "publish_cleanup_pending"
        );
        // An observed absence cannot discharge the possible POST.
        fake.routes.lock().unwrap().clear();
        h.tick().await;
        until(|| !fake.routes.lock().unwrap().is_empty()).await;
        assert_eq!(pinned(&intent(&h).unwrap()), pinned(&before));
        assert_eq!(phase(&a, &vm).await["phase"], "cleanup_pending");
        late.send(()).unwrap();
        until(|| fake.record.load(SeqCst)).await;
        settle(&h, &a, &vm).await;
        let routes = fake.routes.lock().unwrap().clone();
        let original = format!("{} publish.example.invalid ", "b".repeat(32));
        assert!(
            routes.iter().all(|route| route.starts_with(&original)),
            "{routes:?}"
        );
        // Only the attributed record was deleted, with no second POST and no
        // connector start after the restart.
        assert_eq!((fake.posts.load(SeqCst), fake.deletes.load(SeqCst)), (2, 2));
        assert_eq!(fake.starts.load(SeqCst), starts);
        assert!(!fake.local.load(SeqCst));
        drop(a);
        h.shutdown().await.unwrap();
    }

    /// Joins every settled deferred revocation stop; true once none remain.
    fn revocations_settled(supervisor: &Supervisor) -> bool {
        let mut stops = supervisor.stops.lock().unwrap();
        stops.reap();
        stops.set.is_empty() && stops.running.is_empty()
    }

    fn stops_spawned(supervisor: &Supervisor) -> usize {
        supervisor.stops_spawned.load(Ordering::SeqCst)
    }

    fn gate() -> (
        crate::vm::runtime::publication::FakeGate,
        tokio::sync::oneshot::Receiver<()>,
        tokio::sync::oneshot::Sender<()>,
    ) {
        let (entered, in_gate) = tokio::sync::oneshot::channel();
        let (release, hold) = tokio::sync::oneshot::channel();
        ((entered, hold), in_gate, release)
    }

    async fn entered(in_gate: tokio::sync::oneshot::Receiver<()>) {
        tokio::time::timeout(Duration::from_secs(2), in_gate)
            .await
            .expect("gate not entered")
            .unwrap();
    }

    /// Calls a tool from its own connection, so the test can observe effects
    /// while the call is blocked.
    fn spawn_call(
        client: Client,
        name: &'static str,
        args: Value,
    ) -> tokio::task::JoinHandle<Value> {
        tokio::spawn(async move {
            let row = call(&client, name, args).await;
            drop(client);
            row
        })
    }

    #[tokio::test]
    async fn owner_revocation_does_not_wait_on_the_dispatch_gate_or_the_store() {
        use std::sync::atomic::Ordering::SeqCst;
        let mut h = Harness::start_with_web(false, false, true).await;
        let a = h.client(0).await;
        let (vm, fake) = enrolled(&h, &a, 900).await;
        let args = |n| json!({"vm_id":vm,"request_id":key(n)});

        // Another dispatch holds the foreground gate.
        let published = call(&a, "publish_site", args(910)).await;
        assert_eq!(terminal(&a, &published).await["state"], "succeeded");
        assert!(fake.local.load(SeqCst));
        let (reader, revoker) = (h.client(0).await, h.client(0).await);
        let mut stall = Stall::install(&h.supervisor().stall_dispatch);
        let blocked = spawn_call(reader, "publish_status", json!({"vm_id":vm}));
        stall.entered().await;
        let revoke = spawn_call(revoker, "unpublish_site", args(911));
        until(|| !fake.local.load(SeqCst)).await;
        assert!(!revoke.is_finished(), "unpublish passed a held gate");
        stall.release_and_join().await;
        assert_eq!(blocked.await.unwrap()["ok"], true);
        let revoke = revoke.await.unwrap();
        assert_eq!(terminal(&a, &revoke).await["state"], "succeeded");
        assert_eq!(phase(&a, &vm).await["phase"], "unpublished");
        assert_eq!((fake.posts.load(SeqCst), fake.deletes.load(SeqCst)), (1, 1));

        // Another writer holds the SQLite write transaction.
        let published = call(&a, "publish_site", args(912)).await;
        assert_eq!(terminal(&a, &published).await["state"], "succeeded");
        assert!(fake.local.load(SeqCst));
        let revoker = h.client(0).await;
        let (in_store, store_entered) = std::sync::mpsc::channel();
        let (release_store, held) = std::sync::mpsc::channel::<()>();
        let supervisor = h.supervisor().clone();
        let writer = std::thread::spawn(move || {
            supervisor
                .controller
                .store_for_test()
                .transact(|_| {
                    in_store.send(()).unwrap();
                    held.recv().unwrap();
                    Ok(())
                })
                .unwrap();
        });
        store_entered.recv().unwrap();
        let revoke = spawn_call(revoker, "unpublish_site", args(913));
        until(|| !fake.local.load(SeqCst)).await;
        until(|| revocations_settled(h.supervisor())).await;
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(!revoke.is_finished(), "unpublish passed a held store");
        release_store.send(()).unwrap();
        writer.join().unwrap();
        let revoke = revoke.await.unwrap();
        assert_eq!(terminal(&a, &revoke).await["state"], "succeeded");
        assert_eq!(phase(&a, &vm).await["phase"], "unpublished");
        assert_eq!((fake.posts.load(SeqCst), fake.deletes.load(SeqCst)), (2, 2));
        drop(a);
        h.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn publish_commit_never_overwrites_a_revocation() {
        use std::sync::atomic::Ordering::SeqCst;
        let mut h = Harness::start_with_web(false, false, true).await;
        let a = h.client(0).await;
        let (vm, fake) = enrolled(&h, &a, 920).await;
        let args = |n| json!({"vm_id":vm,"request_id":key(n)});
        let store = h.supervisor().controller.store_for_test();
        // Records every durable `published` transition.
        store
            .transact(|db| {
                Ok(db.execute_batch(
                    "CREATE TABLE published_witness(n INTEGER) STRICT;
                     CREATE TRIGGER published_witness AFTER UPDATE OF phase ON vm_publication
                     WHEN NEW.phase='published' BEGIN INSERT INTO published_witness VALUES(1); END;",
                )?)
            })
            .unwrap();
        let written = || {
            store
                .transact(|db| {
                    Ok(
                        db.query_row("SELECT count(*) FROM published_witness", [], |row| {
                            row.get::<_, i64>(0)
                        })?,
                    )
                })
                .unwrap()
        };
        let pause = |h: &Harness| {
            let (commit, in_gate, release) = gate();
            *h.supervisor().publisher.commit_gate.lock().unwrap() = Some(commit);
            (in_gate, release)
        };

        // The owner unpublish commits `revoking` while the worker is paused
        // after its token check.
        let (in_gate, release) = pause(&h);
        let pending = call(&a, "publish_site", args(930)).await;
        entered(in_gate).await;
        let revoke = call(&a, "unpublish_site", args(931)).await;
        assert_eq!(revoke["ok"], true, "{revoke}");
        assert_eq!(phase(&a, &vm).await["phase"], "revoking");
        release.send(()).unwrap();
        let failed = terminal(&a, &pending).await;
        assert_eq!(
            (&failed["state"], &failed["reason"]),
            (&json!("failed"), &json!("cancelled")),
            "{failed}"
        );
        assert_eq!(terminal(&a, &revoke).await["state"], "succeeded");
        assert_eq!(phase(&a, &vm).await["phase"], "unpublished");
        assert_eq!(written(), 0);
        assert!(!fake.local.load(SeqCst));

        // A durable revocation by another writer, with no in-memory token:
        // the compare-and-set on the intent phase alone refuses the commit.
        let (in_gate, release) = pause(&h);
        let pending = call(&a, "publish_site", args(932)).await;
        entered(in_gate).await;
        let nonce = h
            .supervisor()
            .controller
            .publication_intent()
            .unwrap()
            .unwrap()
            .nonce;
        h.supervisor()
            .controller
            .update_publication(
                &nonce,
                crate::store::vm::publish::Update::Phase(
                    crate::store::vm::publish::Phase::Revoking,
                    None,
                ),
            )
            .unwrap();
        release.send(()).unwrap();
        let failed = terminal(&a, &pending).await;
        assert_eq!(failed["state"], "failed", "{failed}");
        assert_eq!(phase(&a, &vm).await["phase"], "unpublished");
        assert_eq!(written(), 0);
        assert!(!fake.local.load(SeqCst));

        // An owner revocation whose durable unpublish is refused: the token
        // check inside the commit transaction alone refuses the commit.
        let (in_gate, release) = pause(&h);
        let pending = call(&a, "publish_site", args(933)).await;
        entered(in_gate).await;
        store
            .transact(|db| {
                db.execute_batch("CREATE TABLE pressure(filler BLOB) STRICT")?;
                while crate::store::vm::headroom(db)? {
                    db.execute("INSERT INTO pressure VALUES(zeroblob(2048))", [])?;
                }
                Ok(())
            })
            .unwrap();
        let refused = call(&a, "unpublish_site", args(934)).await;
        assert_eq!(refused["reason"], "state_headroom", "{refused}");
        assert_eq!(phase(&a, &vm).await["phase"], "publishing");
        store
            .transact(|db| Ok(db.execute_batch("DROP TABLE pressure")?))
            .unwrap();
        release.send(()).unwrap();
        let failed = terminal(&a, &pending).await;
        assert_eq!(
            (&failed["state"], &failed["reason"]),
            (&json!("failed"), &json!("cancelled")),
            "{failed}"
        );
        assert_eq!(phase(&a, &vm).await["phase"], "unpublished");
        assert_eq!(written(), 0);
        assert!(!fake.local.load(SeqCst));
        assert_eq!((fake.posts.load(SeqCst), fake.deletes.load(SeqCst)), (3, 3));

        // Unpaused, the same path does commit.
        let published = call(&a, "publish_site", args(935)).await;
        assert_eq!(terminal(&a, &published).await["state"], "succeeded");
        assert_eq!(written(), 1);
        drop(a);
        h.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn deferred_revocation_work_never_touches_a_later_activation() {
        use std::sync::atomic::Ordering::SeqCst;
        let mut h = Harness::start_with_web(false, false, true).await;
        let a = h.client(0).await;
        let (vm, fake) = enrolled(&h, &a, 940).await;
        let args = |n| json!({"vm_id":vm,"request_id":key(n)});

        // A revocation stop delayed across republication.
        let published = call(&a, "publish_site", args(950)).await;
        assert_eq!(terminal(&a, &published).await["state"], "succeeded");
        let (stop, in_gate, release) = gate();
        *h.supervisor().publisher.stop_gate.lock().unwrap() = Some(stop);
        let revoke = call(&a, "unpublish_site", args(951)).await;
        entered(in_gate).await;
        assert_eq!(terminal(&a, &revoke).await["state"], "succeeded");
        assert_eq!(phase(&a, &vm).await["phase"], "unpublished");
        assert!(!fake.local.load(SeqCst));
        let published = call(&a, "publish_site", args(952)).await;
        assert_eq!(terminal(&a, &published).await["state"], "succeeded");
        assert!(fake.local.load(SeqCst));
        assert!(!revocations_settled(h.supervisor()));
        release.send(()).unwrap();
        until(|| revocations_settled(h.supervisor())).await;
        assert!(
            fake.local.load(SeqCst),
            "a stale stop ended a later activation"
        );
        h.tick().await;
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(phase(&a, &vm).await["phase"], "published");
        assert!(fake.local.load(SeqCst));

        // An unpublish worker cannot outlive its receipt into a later
        // activation: its pending receipt keeps publish admission closed.
        let revoke = call(&a, "unpublish_site", args(953)).await;
        assert_eq!(terminal(&a, &revoke).await["state"], "succeeded");
        let deletes = fake.deletes.load(SeqCst);
        let (worker, in_gate, release) = gate();
        *h.supervisor().publisher.unpublish_gate.lock().unwrap() = Some(worker);
        let stale = call(&a, "unpublish_site", args(954)).await;
        entered(in_gate).await;
        assert_eq!(
            call(&a, "publish_site", args(955)).await["reason"],
            "not_ready"
        );
        release.send(()).unwrap();
        assert_eq!(terminal(&a, &stale).await["state"], "succeeded");
        until(|| revocations_settled(h.supervisor())).await;
        let published = call(&a, "publish_site", args(956)).await;
        assert_eq!(terminal(&a, &published).await["state"], "succeeded");
        assert!(fake.local.load(SeqCst));
        assert_eq!(fake.deletes.load(SeqCst), deletes);
        let revoke = call(&a, "unpublish_site", args(957)).await;
        assert_eq!(terminal(&a, &revoke).await["state"], "succeeded");
        assert!(!fake.local.load(SeqCst));
        drop(a);
        h.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn publication_audit_write_failure_closes_ingress_and_the_reaper_retries() {
        use std::sync::atomic::Ordering::SeqCst;
        let mut h = Harness::start_with_web(false, false, true).await;
        let a = h.client(0).await;
        let (vm, fake) = enrolled(&h, &a, 960).await;
        let args = |n| json!({"vm_id":vm,"request_id":key(n)});
        let store = h.supervisor().controller.store_for_test();
        let fail_audit = |when: &str| {
            store
                .transact(|db| {
                    Ok(db.execute_batch(&format!(
                        "CREATE TRIGGER audit_failure BEFORE INSERT ON activity WHEN {when}
                         BEGIN SELECT RAISE(ABORT, 'synthetic audit write failure'); END;"
                    ))?)
                })
                .unwrap();
        };
        let resume_audit = || {
            store
                .transact(|db| Ok(db.execute_batch("DROP TRIGGER audit_failure")?))
                .unwrap();
        };
        let releases = || {
            store
                .transact(|db| {
                    Ok(db.query_row(
                        "SELECT count(*) FROM activity WHERE actor='daemon' AND action=17",
                        [],
                        |row| row.get::<_, i64>(0),
                    )?)
                })
                .unwrap()
        };

        // Every audit write fails: the owner unpublish still closes ingress,
        // and its admission fails without a receipt.
        let published = call(&a, "publish_site", args(970)).await;
        assert_eq!(terminal(&a, &published).await["state"], "succeeded");
        fail_audit("1");
        let refused = call(&a, "unpublish_site", args(971)).await;
        assert_eq!(refused["ok"], false, "{refused}");
        until(|| !fake.local.load(SeqCst)).await;
        assert_eq!(phase(&a, &vm).await["phase"], "published");
        resume_audit();
        assert_eq!(settle(&h, &a, &vm).await["reason"], "cancelled");
        assert_eq!((fake.posts.load(SeqCst), fake.deletes.load(SeqCst)), (1, 1));
        assert_eq!(releases(), 1);

        // The reconciliation audit shares the intent release. Its failure
        // leaves the intent for the next reaper turn instead of ending the
        // reaper, and the retry neither deletes twice nor audits twice.
        let published = call(&a, "publish_site", args(972)).await;
        assert_eq!(terminal(&a, &published).await["state"], "succeeded");
        fail_audit("NEW.actor='daemon' AND NEW.action=17");
        fake.health.store(1, SeqCst);
        h.tick().await;
        until(|| fake.deletes.load(SeqCst) == 2 && !fake.local.load(SeqCst)).await;
        h.tick().await;
        h.tick().await;
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(phase(&a, &vm).await["phase"], "revoking");
        assert_eq!(releases(), 1);
        resume_audit();
        assert_eq!(settle(&h, &a, &vm).await["reason"], "connector_exited");
        assert_eq!((fake.posts.load(SeqCst), fake.deletes.load(SeqCst)), (2, 2));
        assert_eq!(releases(), 2);

        // The admitted unpublish's terminal receipt write fails after it
        // released the intent. The reaper settles the receipt once writes
        // resume, which reopens publish admission.
        fake.health.store(0, SeqCst);
        let published = call(&a, "publish_site", args(973)).await;
        assert_eq!(terminal(&a, &published).await["state"], "succeeded");
        store
            .transact(|db| {
                Ok(db.execute_batch(
                    "CREATE TRIGGER terminal_failure BEFORE UPDATE OF status ON activity
                     WHEN OLD.action=16 AND OLD.status=0 AND NEW.status<>0
                     BEGIN SELECT RAISE(ABORT, 'synthetic terminal write failure'); END;",
                )?)
            })
            .unwrap();
        let revoke = call(&a, "unpublish_site", args(974)).await;
        assert_eq!(revoke["ok"], true, "{revoke}");
        until(|| h.supervisor().publisher.workers().is_empty()).await;
        assert!(!fake.local.load(SeqCst));
        assert_eq!(phase(&a, &vm).await["phase"], "unpublished");
        assert_eq!(operation(&a, &revoke).await["state"], "pending");
        assert_eq!(
            call(&a, "publish_site", args(975)).await["reason"],
            "not_ready"
        );
        store
            .transact(|db| Ok(db.execute_batch("DROP TRIGGER terminal_failure")?))
            .unwrap();
        h.tick().await;
        assert_eq!(terminal(&a, &revoke).await["state"], "succeeded");
        let published = call(&a, "publish_site", args(976)).await;
        assert_eq!(terminal(&a, &published).await["state"], "succeeded");
        assert_eq!((fake.posts.load(SeqCst), fake.deletes.load(SeqCst)), (4, 3));
        drop(a);
        h.shutdown().await.unwrap();
    }

    async fn operation(a: &Client, reference: &Value) -> Value {
        call(
            a,
            "vm_operation",
            json!({"operation_id":reference["operation_id"]}),
        )
        .await
    }

    /// Waits for `unpublished` without driving a reaper turn.
    async fn converged(a: &Client, vm: &Value) -> Value {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let row = phase(a, vm).await;
                if row["phase"] == "unpublished" {
                    break row;
                }
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .expect("publication did not converge without the reaper")
    }

    fn trigger(h: &Harness, sql: &str) {
        h.supervisor()
            .controller
            .store_for_test()
            .transact(|db| Ok(db.execute_batch(sql)?))
            .unwrap();
    }

    #[tokio::test]
    async fn every_owner_unpublish_revokes_at_once_whatever_its_key() {
        use std::sync::atomic::Ordering::SeqCst;
        let mut h = Harness::start_with_web(false, false, true).await;
        let a = h.client(0).await;
        let b = h.client(1).await;
        let (vm, fake) = enrolled(&h, &a, 1000).await;
        let args = |n| json!({"vm_id":vm,"request_id":key(n)});

        // A foreign principal never revokes.
        let published = call(&a, "publish_site", args(1010)).await;
        assert_eq!(terminal(&a, &published).await["state"], "succeeded");
        assert_eq!(
            call(&b, "unpublish_site", args(1011)).await["reason"],
            "not_found"
        );
        assert!(revocations_settled(h.supervisor()));
        assert!(fake.local.load(SeqCst));

        // A distinct owner key minted before the publish key, with the
        // dispatch gate held by another call.
        let early = json!({"vm_id":vm,"request_id":format!("{:016x}{:016x}", UTC - 1_000, 1012)});
        let (reader, revoker) = (h.client(0).await, h.client(0).await);
        let mut stall = Stall::install(&h.supervisor().stall_dispatch);
        let blocked = spawn_call(reader, "publish_status", json!({"vm_id":vm}));
        stall.entered().await;
        let revoke = spawn_call(revoker, "unpublish_site", early);
        until(|| !fake.local.load(SeqCst)).await;
        assert!(!revoke.is_finished(), "unpublish passed a held gate");
        stall.release_and_join().await;
        assert_eq!(blocked.await.unwrap()["ok"], true);
        let revoke = revoke.await.unwrap();
        assert_eq!(terminal(&a, &revoke).await["state"], "succeeded");
        assert_eq!(phase(&a, &vm).await["phase"], "unpublished");
        until(|| revocations_settled(h.supervisor())).await;

        // An owner unpublish racing the publish admission transaction, whose
        // own durable admission then fails: the registered activation is
        // revoked before its worker can start the connector.
        let starts = fake.starts.load(SeqCst);
        let spawned = stops_spawned(h.supervisor());
        let (caller, revoker) = (h.client(0).await, h.client(0).await);
        let mut stall = Stall::install(&h.supervisor().stall_admission);
        let pending = spawn_call(caller, "publish_site", args(1013));
        stall.entered().await;
        let revoke = spawn_call(revoker, "unpublish_site", args(1014));
        until(|| stops_spawned(h.supervisor()) > spawned).await;
        trigger(
            &h,
            "CREATE TRIGGER refuse_unpublish BEFORE INSERT ON activity WHEN NEW.action=16
             BEGIN SELECT RAISE(ABORT, 'synthetic unpublish admission failure'); END;",
        );
        stall.release_and_join().await;
        let pending = pending.await.unwrap();
        assert_eq!(revoke.await.unwrap()["ok"], false);
        let failed = terminal(&a, &pending).await;
        assert_eq!(
            (&failed["state"], &failed["reason"]),
            (&json!("failed"), &json!("cancelled")),
            "{failed}"
        );
        trigger(&h, "DROP TRIGGER refuse_unpublish");
        assert_eq!(phase(&a, &vm).await["phase"], "unpublished");
        assert_eq!(fake.starts.load(SeqCst), starts);
        assert!(!fake.local.load(SeqCst));
        assert_eq!((fake.posts.load(SeqCst), fake.deletes.load(SeqCst)), (1, 1));
        until(|| revocations_settled(h.supervisor())).await;
        drop((a, b));
        h.shutdown().await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn owner_cancel_inside_the_publish_commit_converges_to_unpublished() {
        use std::sync::atomic::Ordering::SeqCst;
        let mut h = Harness::start_with_web(false, false, true).await;
        let a = h.client(0).await;
        let (vm, fake) = enrolled(&h, &a, 1020).await;
        let args = |n| json!({"vm_id":vm,"request_id":key(n)});
        // The owner's durable unpublish is refused, so only the publish
        // worker itself can converge, with no reaper turn.
        trigger(
            &h,
            "CREATE TRIGGER refuse_unpublish BEFORE INSERT ON activity WHEN NEW.action=16
             BEGIN SELECT RAISE(ABORT, 'synthetic unpublish admission failure'); END;",
        );
        let (entered, in_commit) = tokio::sync::oneshot::channel();
        let (release, held) = std::sync::mpsc::channel();
        *h.supervisor().publisher.commit_hook.lock().unwrap() = Some((entered, held));
        let revoker = h.client(0).await;
        let pending = call(&a, "publish_site", args(1030)).await;
        // Inside the commit transaction, after its authorization.
        tokio::time::timeout(Duration::from_secs(2), in_commit)
            .await
            .unwrap()
            .unwrap();
        assert!(fake.local.load(SeqCst));
        let revoke = spawn_call(revoker, "unpublish_site", args(1031));
        until(|| !fake.local.load(SeqCst)).await;
        release.send(()).unwrap();
        assert_eq!(revoke.await.unwrap()["ok"], false);
        // The commit stands, and the worker drives the activation back.
        assert_eq!(terminal(&a, &pending).await["state"], "succeeded");
        assert_eq!(converged(&a, &vm).await["reason"], "cancelled");
        assert!(!fake.local.load(SeqCst));
        assert_eq!((fake.posts.load(SeqCst), fake.deletes.load(SeqCst)), (1, 1));
        trigger(&h, "DROP TRIGGER refuse_unpublish");
        drop(a);
        h.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn a_cancelled_publish_never_pins_a_post_it_did_not_submit() {
        use std::sync::atomic::Ordering::SeqCst;
        let mut h = Harness::start_with_web(false, false, true).await;
        let a = h.client(0).await;
        let (vm, fake) = enrolled(&h, &a, 1040).await;
        let args = |n| json!({"vm_id":vm,"request_id":key(n)});
        let pause = |h: &Harness| {
            let (marker, in_gate, release) = gate();
            *h.supervisor().publisher.marker_gate.lock().unwrap() = Some(marker);
            (in_gate, release)
        };

        // An owner unpublish commits `revoking` between the durable marker
        // and the pre-submission check.
        let (in_gate, release) = pause(&h);
        let pending = call(&a, "publish_site", args(1050)).await;
        entered(in_gate).await;
        let revoke = call(&a, "unpublish_site", args(1051)).await;
        assert_eq!(revoke["ok"], true, "{revoke}");
        assert_eq!(phase(&a, &vm).await["phase"], "revoking");
        release.send(()).unwrap();
        let failed = terminal(&a, &pending).await;
        assert_eq!(
            (&failed["state"], &failed["reason"], &failed["cleanup"]),
            (&json!("failed"), &json!("cancelled"), &json!("confirmed")),
            "{failed}"
        );
        assert_eq!(terminal(&a, &revoke).await["state"], "succeeded");
        assert_eq!(phase(&a, &vm).await["phase"], "unpublished");
        assert_eq!((fake.posts.load(SeqCst), fake.deletes.load(SeqCst)), (0, 0));
        let published = call(&a, "publish_site", args(1052)).await;
        assert_eq!(terminal(&a, &published).await["state"], "succeeded");
        let revoke = call(&a, "unpublish_site", args(1053)).await;
        assert_eq!(terminal(&a, &revoke).await["state"], "succeeded");

        // The withdrawal write itself fails: the worker's no-submission
        // evidence still releases the intent without consulting DNS.
        trigger(
            &h,
            "CREATE TRIGGER withdrawal_failure BEFORE UPDATE OF post_possible ON vm_publication
             WHEN OLD.post_possible=1 AND NEW.post_possible=0
             BEGIN SELECT RAISE(ABORT, 'synthetic withdrawal failure'); END;",
        );
        let (in_gate, release) = pause(&h);
        let pending = call(&a, "publish_site", args(1054)).await;
        entered(in_gate).await;
        let revoke = call(&a, "unpublish_site", args(1055)).await;
        release.send(()).unwrap();
        let failed = terminal(&a, &pending).await;
        assert_eq!(
            (&failed["state"], &failed["reason"], &failed["cleanup"]),
            (
                &json!("failed"),
                &json!("cleanup_unconfirmed"),
                &json!("confirmed")
            ),
            "{failed}"
        );
        assert_eq!(terminal(&a, &revoke).await["state"], "succeeded");
        assert_eq!(phase(&a, &vm).await["phase"], "unpublished");
        trigger(&h, "DROP TRIGGER withdrawal_failure");

        // The withdrawal commits in `revoking`, then every release fails
        // until writes resume. Both workers exit with pending receipts; the
        // reaper releases the intent from its withdrawn marker and settles
        // both receipts, and publication reopens.
        trigger(
            &h,
            "CREATE TRIGGER release_failure BEFORE DELETE ON vm_publication
             BEGIN SELECT RAISE(ABORT, 'synthetic release failure'); END;",
        );
        let (in_gate, release) = pause(&h);
        let pending = call(&a, "publish_site", args(1056)).await;
        entered(in_gate).await;
        let revoke = call(&a, "unpublish_site", args(1057)).await;
        release.send(()).unwrap();
        until(|| h.supervisor().publisher.workers().is_empty()).await;
        assert_eq!(operation(&a, &pending).await["state"], "pending");
        assert_eq!(operation(&a, &revoke).await["state"], "pending");
        assert_eq!(phase(&a, &vm).await["phase"], "revoking");
        assert_eq!(
            call(&a, "publish_site", args(1058)).await["reason"],
            "publish_cleanup_pending"
        );
        trigger(&h, "DROP TRIGGER release_failure");
        settle(&h, &a, &vm).await;
        let failed = terminal(&a, &pending).await;
        assert_eq!(
            (&failed["state"], &failed["reason"]),
            (&json!("failed"), &json!("cancelled")),
            "{failed}"
        );
        assert_eq!(terminal(&a, &revoke).await["state"], "succeeded");
        let published = call(&a, "publish_site", args(1059)).await;
        assert_eq!(terminal(&a, &published).await["state"], "succeeded");
        assert!(fake.local.load(SeqCst));
        // Only the two completed publications ever posted.
        assert_eq!((fake.posts.load(SeqCst), fake.deletes.load(SeqCst)), (2, 1));
        let revoke = call(&a, "unpublish_site", args(1060)).await;
        assert_eq!(terminal(&a, &revoke).await["state"], "succeeded");

        // The withdrawal and every release fail in one outage. The proof
        // stays in memory for the reaper, which releases the intent without
        // DNS once writes resume.
        trigger(
            &h,
            "CREATE TRIGGER withdrawal_failure BEFORE UPDATE OF post_possible ON vm_publication
             WHEN OLD.post_possible=1 AND NEW.post_possible=0
             BEGIN SELECT RAISE(ABORT, 'synthetic withdrawal failure'); END;
             CREATE TRIGGER release_failure BEFORE DELETE ON vm_publication
             BEGIN SELECT RAISE(ABORT, 'synthetic release failure'); END;",
        );
        let (in_gate, release) = pause(&h);
        let pending = call(&a, "publish_site", args(1061)).await;
        entered(in_gate).await;
        let revoke = call(&a, "unpublish_site", args(1062)).await;
        release.send(()).unwrap();
        until(|| h.supervisor().publisher.workers().is_empty()).await;
        assert_eq!(operation(&a, &pending).await["state"], "pending");
        assert_eq!(phase(&a, &vm).await["phase"], "revoking");
        trigger(
            &h,
            "DROP TRIGGER withdrawal_failure; DROP TRIGGER release_failure;",
        );
        fake.routes.lock().unwrap().clear();
        settle(&h, &a, &vm).await;
        assert!(fake.routes.lock().unwrap().is_empty(), "DNS consulted");
        assert_eq!(terminal(&a, &pending).await["state"], "failed");
        assert_eq!(terminal(&a, &revoke).await["state"], "succeeded");
        let published = call(&a, "publish_site", args(1063)).await;
        assert_eq!(terminal(&a, &published).await["state"], "succeeded");
        assert_eq!((fake.posts.load(SeqCst), fake.deletes.load(SeqCst)), (3, 2));
        let revoke = call(&a, "unpublish_site", args(1064)).await;
        assert_eq!(terminal(&a, &revoke).await["state"], "succeeded");
        drop(a);
        h.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn publication_registry_and_stop_jobs_drain_on_every_terminal_path() {
        use std::sync::atomic::Ordering::SeqCst;
        let mut h = Harness::start_with_web(false, false, true).await;
        let a = h.client(0).await;
        let (vm, fake) = enrolled(&h, &a, 1100).await;
        let args = |n| json!({"vm_id":vm,"request_id":key(n)});
        let empty = |h: &Harness| {
            h.supervisor().publisher.registry_len() == 0 && revocations_settled(h.supervisor())
        };
        assert!(empty(&h));

        // Refused in the admission transaction after registration.
        trigger(
            &h,
            "CREATE TRIGGER refuse_publish BEFORE INSERT ON activity WHEN NEW.action=15
             BEGIN SELECT RAISE(ABORT, 'synthetic publish admission failure'); END;",
        );
        assert_eq!(call(&a, "publish_site", args(1110)).await["ok"], false);
        trigger(&h, "DROP TRIGGER refuse_publish");
        assert!(empty(&h));

        // A failed publish releases its publication.
        fake.foreign.store(true, SeqCst);
        let failed = call(&a, "publish_site", args(1111)).await;
        assert_eq!(terminal(&a, &failed).await["reason"], "dns_conflict");
        fake.foreign.store(false, SeqCst);
        assert!(empty(&h));

        // Confirmed cleanup by an owner unpublish. The ended publication no
        // longer matches a later owner unpublish.
        let published = call(&a, "publish_site", args(1112)).await;
        assert_eq!(terminal(&a, &published).await["state"], "succeeded");
        assert_eq!(h.supervisor().publisher.registry_len(), 1);
        let revoke = call(&a, "unpublish_site", args(1113)).await;
        assert_eq!(terminal(&a, &revoke).await["state"], "succeeded");
        until(|| empty(&h)).await;
        let spawned = stops_spawned(h.supervisor());
        let repeated = call(&a, "unpublish_site", args(1114)).await;
        assert_eq!(terminal(&a, &repeated).await["state"], "succeeded");
        assert_eq!(stops_spawned(h.supervisor()), spawned);

        // DNS cleanup stays pending, but ingress is confirmed down.
        let published = call(&a, "publish_site", args(1115)).await;
        assert_eq!(terminal(&a, &published).await["state"], "succeeded");
        fake.credential.store(false, SeqCst);
        let revoke = call(&a, "unpublish_site", args(1116)).await;
        assert_eq!(terminal(&a, &revoke).await["state"], "failed");
        assert_eq!(phase(&a, &vm).await["phase"], "cleanup_pending");
        until(|| empty(&h)).await;
        fake.credential.store(true, SeqCst);
        settle(&h, &a, &vm).await;

        // Reconciliation of a lost connector.
        let published = call(&a, "publish_site", args(1117)).await;
        assert_eq!(terminal(&a, &published).await["state"], "succeeded");
        fake.health.store(1, SeqCst);
        settle(&h, &a, &vm).await;
        fake.health.store(0, SeqCst);
        until(|| empty(&h)).await;

        // Bound: twenty owner unpublishes while the first stop is held queue
        // one job, and the job set drains once it completes.
        let published = call(&a, "publish_site", args(1118)).await;
        assert_eq!(terminal(&a, &published).await["state"], "succeeded");
        let (stop, in_gate, release) = gate();
        *h.supervisor().publisher.stop_gate.lock().unwrap() = Some(stop);
        let spawned = stops_spawned(h.supervisor());
        for _ in 0..20 {
            // The publish key collides, so no durable unpublish is admitted.
            assert_eq!(
                call(&a, "unpublish_site", args(1118)).await["reason"],
                "key_collision"
            );
        }
        entered(in_gate).await;
        assert_eq!(stops_spawned(h.supervisor()), spawned + 1);
        assert_eq!(h.supervisor().stops.lock().unwrap().running.len(), 1);
        release.send(()).unwrap();
        until(|| revocations_settled(h.supervisor())).await;
        assert!(!fake.local.load(SeqCst));
        // The confirmed deferred stop retired the activation, with no reaper
        // tick and no cleanup worker.
        assert_eq!(h.supervisor().publisher.registry_len(), 0);
        settle(&h, &a, &vm).await;
        until(|| empty(&h)).await;

        // Daemon shutdown with a live publication.
        let published = call(&a, "publish_site", args(1119)).await;
        assert_eq!(terminal(&a, &published).await["state"], "succeeded");
        assert_eq!(h.supervisor().publisher.registry_len(), 1);
        let supervisor = h.supervisor().clone();
        drop(a);
        h.shutdown().await.unwrap();
        assert_eq!(supervisor.publisher.registry_len(), 0);
        assert!(revocations_settled(&supervisor));
        assert!(!fake.local.load(SeqCst));
    }

    #[tokio::test]
    async fn every_confirmed_stop_retires_its_activation_at_once() {
        use std::sync::atomic::Ordering::SeqCst;
        let mut h = Harness::start_with_web(false, false, true).await;
        let a = h.client(0).await;
        let (vm, fake) = enrolled(&h, &a, 1200).await;
        let args = |n| json!({"vm_id":vm,"request_id":key(n)});

        // The publish success transaction fails: the worker's own confirmed
        // stop retires the activation before any reaper turn.
        trigger(
            &h,
            "CREATE TRIGGER refuse_commit BEFORE UPDATE OF phase ON vm_publication
             WHEN NEW.phase='published'
             BEGIN SELECT RAISE(ABORT, 'synthetic publish commit failure'); END;",
        );
        let pending = call(&a, "publish_site", args(1210)).await;
        assert_eq!(pending["ok"], true, "{pending}");
        until(|| fake.posts.load(SeqCst) == 1).await;
        until(|| h.supervisor().publisher.workers().is_empty()).await;
        assert!(!fake.local.load(SeqCst));
        assert_eq!(h.supervisor().publisher.registry_len(), 0);
        trigger(&h, "DROP TRIGGER refuse_commit");
        settle(&h, &a, &vm).await;
        assert_eq!(terminal(&a, &pending).await["state"], "failed");

        // A colliding owner unpublish never reaches a cleanup worker. Its
        // confirmed deferred stop retires the activation, with no reaper tick.
        let published = call(&a, "publish_site", args(1211)).await;
        assert_eq!(terminal(&a, &published).await["state"], "succeeded");
        assert_eq!(h.supervisor().publisher.registry_len(), 1);
        assert_eq!(
            call(&a, "unpublish_site", args(1211)).await["reason"],
            "key_collision"
        );
        until(|| !fake.local.load(SeqCst)).await;
        until(|| revocations_settled(h.supervisor())).await;
        assert_eq!(h.supervisor().publisher.registry_len(), 0);
        settle(&h, &a, &vm).await;

        // Shutdown: its first stop fails, then a queued deferred stop
        // succeeds. The drain checks the final local state.
        let published = call(&a, "publish_site", args(1212)).await;
        assert_eq!(terminal(&a, &published).await["state"], "succeeded");
        let (stop, in_gate, release) = gate();
        *h.supervisor().publisher.stop_gate.lock().unwrap() = Some(stop);
        assert_eq!(
            call(&a, "unpublish_site", args(1212)).await["reason"],
            "key_collision"
        );
        entered(in_gate).await;
        fake.stop_failures.store(1, SeqCst);
        let releaser = {
            let fake = fake.clone();
            tokio::spawn(async move {
                // Released only after the drain's first stop has failed.
                while fake.stop_failures.load(SeqCst) != 0 {
                    tokio::time::sleep(Duration::from_millis(2)).await;
                }
                release.send(()).unwrap();
            })
        };
        let supervisor = h.supervisor().clone();
        drop(a);
        h.shutdown().await.unwrap();
        releaser.await.unwrap();
        assert_eq!(supervisor.publisher.registry_len(), 0);
        assert!(!supervisor.publisher.stop_unconfirmed());
        assert!(revocations_settled(&supervisor));
        assert!(!fake.local.load(SeqCst));
    }

    #[tokio::test]
    async fn cancellation_before_the_first_post_poll_keeps_the_no_submission_proof() {
        use std::sync::atomic::Ordering::SeqCst;
        let mut h = Harness::start_with_web(false, false, true).await;
        let a = h.client(0).await;
        let (vm, fake) = enrolled(&h, &a, 1220).await;
        let args = |n| json!({"vm_id":vm,"request_id":key(n)});
        let (submit, in_gate, release) = gate();
        *h.supervisor().publisher.submit_gate.lock().unwrap() = Some(submit);
        let pending = call(&a, "publish_site", args(1230)).await;
        // Past the pre-submission check, before the POST future is polled.
        entered(in_gate).await;
        fake.routes.lock().unwrap().clear();
        let stop = call(&a, "vm_stop", args(1231)).await;
        assert_eq!(terminal(&a, &stop).await["state"], "succeeded");
        release.send(()).unwrap();
        let failed = terminal(&a, &pending).await;
        assert_eq!(
            (&failed["state"], &failed["reason"], &failed["cleanup"]),
            (
                &json!("failed"),
                &json!("origin_unavailable"),
                &json!("confirmed")
            ),
            "{failed}"
        );
        assert_eq!(phase(&a, &vm).await["phase"], "unpublished");
        // No DNS call of any kind after the marker.
        assert_eq!(fake.posts.load(SeqCst), 0);
        assert!(fake.routes.lock().unwrap().is_empty());
        let start = call(&a, "vm_start", args(1232)).await;
        assert_eq!(terminal(&a, &start).await["state"], "succeeded");
        let published = call(&a, "publish_site", args(1233)).await;
        assert_eq!(terminal(&a, &published).await["state"], "succeeded");
        let revoke = call(&a, "unpublish_site", args(1234)).await;
        assert_eq!(terminal(&a, &revoke).await["state"], "succeeded");
        drop(a);
        h.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn owner_revocation_before_the_first_post_poll_never_starts_dns() {
        use std::sync::atomic::Ordering::SeqCst;
        let mut h = Harness::start_with_web(false, false, true).await;
        let a = h.client(0).await;
        let (vm, fake) = enrolled(&h, &a, 1300).await;
        let args = |n| json!({"vm_id":vm,"request_id":key(n)});
        let (submit, in_gate, release) = gate();
        *h.supervisor().publisher.submit_gate.lock().unwrap() = Some(submit);
        let pending = call(&a, "publish_site", args(1310)).await;
        // Past the pre-submission check, before the POST future is polled.
        entered(in_gate).await;
        // A colliding owner unpublish revokes; its deferred stop is joined and
        // the connector is confirmed down before the worker resumes.
        assert_eq!(
            call(&a, "unpublish_site", args(1310)).await["reason"],
            "key_collision"
        );
        until(|| !fake.local.load(SeqCst)).await;
        until(|| revocations_settled(h.supervisor())).await;
        assert_eq!(h.supervisor().publisher.registry_len(), 0);
        release.send(()).unwrap();
        let failed = terminal(&a, &pending).await;
        assert_eq!(
            (&failed["state"], &failed["reason"], &failed["cleanup"]),
            (&json!("failed"), &json!("cancelled"), &json!("confirmed")),
            "{failed}"
        );
        // DNS start was never called, and the publication released.
        assert_eq!(fake.posts.load(SeqCst), 0);
        assert_eq!(phase(&a, &vm).await["phase"], "unpublished");
        let published = call(&a, "publish_site", args(1311)).await;
        assert_eq!(terminal(&a, &published).await["state"], "succeeded");
        let revoke = call(&a, "unpublish_site", args(1312)).await;
        assert_eq!(terminal(&a, &revoke).await["state"], "succeeded");
        drop(a);
        h.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn a_refused_publish_keeps_the_retired_revocation_cause() {
        use std::sync::atomic::Ordering::SeqCst;
        let mut h = Harness::start_with_web(false, false, true).await;
        let a = h.client(0).await;
        let (vm, fake) = enrolled(&h, &a, 1320).await;
        let args = |n| json!({"vm_id":vm,"request_id":key(n)});
        let published = call(&a, "publish_site", args(1330)).await;
        assert_eq!(terminal(&a, &published).await["state"], "succeeded");
        // A colliding owner unpublish stops the connector; no cleanup has run.
        assert_eq!(
            call(&a, "unpublish_site", args(1330)).await["reason"],
            "key_collision"
        );
        until(|| !fake.local.load(SeqCst)).await;
        until(|| revocations_settled(h.supervisor())).await;
        assert_eq!(h.supervisor().publisher.registry_len(), 0);
        // A refused republish registers and abandons a candidate.
        assert_eq!(
            call(&a, "publish_site", args(1331)).await["reason"],
            "publish_cleanup_pending"
        );
        // The reaper's cleanup still reports the owner revocation as its cause.
        fake.credential.store(false, SeqCst);
        h.tick().await;
        let pending = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let row = phase(&a, &vm).await;
                if row["phase"] == "cleanup_pending" {
                    break row;
                }
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .expect("cleanup did not record its pending state");
        assert_eq!(pending["reason"], "cancelled", "{pending}");
        fake.credential.store(true, SeqCst);
        assert_eq!(settle(&h, &a, &vm).await["reason"], "cancelled");
        let published = call(&a, "publish_site", args(1332)).await;
        assert_eq!(terminal(&a, &published).await["state"], "succeeded");
        let revoke = call(&a, "unpublish_site", args(1333)).await;
        assert_eq!(terminal(&a, &revoke).await["state"], "succeeded");
        drop(a);
        h.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn shutdown_leaves_an_unstarted_registration_to_its_worker() {
        let mut h = Harness::start_with_web(false, false, true).await;
        let a = h.client(0).await;
        let (vm, _fake) = enrolled(&h, &a, 1340).await;
        // The worker parks uncancellably before any effect, so it can only
        // settle once released, after the drain's first stop pass.
        let (worker, in_gate, release) = gate();
        *h.supervisor().publisher.activate_gate.lock().unwrap() = Some(worker);
        let pending = call(
            &a,
            "publish_site",
            json!({"vm_id":vm,"request_id":key(1350)}),
        )
        .await;
        assert_eq!(pending["ok"], true, "{pending}");
        entered(in_gate).await;
        // Registered and admitted, with no connector yet.
        assert_eq!(h.supervisor().publisher.registry_len(), 1);
        let supervisor = h.supervisor().clone();
        supervisor
            .publisher
            .cleanup_registry
            .lock()
            .unwrap()
            .clear();
        let (passed, first_pass) = tokio::sync::oneshot::channel();
        *supervisor.drain_first_pass.lock().unwrap() = Some(passed);
        let releaser = tokio::spawn(async move {
            first_pass.await.unwrap();
            release.send(()).unwrap();
        });
        drop(a);
        h.shutdown().await.unwrap();
        releaser.await.unwrap();
        // The worker settled with its registration still present; only then
        // did the drain clear the registry.
        assert_eq!(
            *supervisor.publisher.cleanup_registry.lock().unwrap(),
            vec![1]
        );
        assert_eq!(supervisor.publisher.registry_len(), 0);
    }

    #[tokio::test]
    async fn a_failed_deferred_stop_blocks_publish_only_until_a_confirmed_stop() {
        use std::sync::atomic::Ordering::SeqCst;
        let mut h = Harness::start_with_web(false, false, true).await;
        let a = h.client(0).await;
        let (vm, fake) = enrolled(&h, &a, 1120).await;
        let args = |n| json!({"vm_id":vm,"request_id":key(n)});
        let published = call(&a, "publish_site", args(1130)).await;
        assert_eq!(terminal(&a, &published).await["state"], "succeeded");

        let (stop, stop_entered, release_stop) = gate();
        let (worker, worker_entered, release_worker) = gate();
        *h.supervisor().publisher.stop_gate.lock().unwrap() = Some(stop);
        *h.supervisor().publisher.unpublish_gate.lock().unwrap() = Some(worker);
        fake.stop_failures.store(1, SeqCst);
        let revoke = call(&a, "unpublish_site", args(1131)).await;
        assert_eq!(revoke["ok"], true, "{revoke}");
        entered(stop_entered).await;
        entered(worker_entered).await;
        // The deferred stop fails and leaves the connector live.
        release_stop.send(()).unwrap();
        until(|| h.supervisor().publisher.stop_unconfirmed()).await;
        assert!(fake.local.load(SeqCst));
        assert_eq!(
            call(&a, "publish_site", args(1132)).await["reason"],
            "state_unavailable"
        );
        // The unpublish worker's stop is confirmed, which reopens admission.
        release_worker.send(()).unwrap();
        assert_eq!(terminal(&a, &revoke).await["state"], "succeeded");
        assert!(!h.supervisor().publisher.stop_unconfirmed());
        assert!(!fake.local.load(SeqCst));
        let published = call(&a, "publish_site", args(1133)).await;
        assert_eq!(terminal(&a, &published).await["state"], "succeeded");
        let revoke = call(&a, "unpublish_site", args(1134)).await;
        assert_eq!(terminal(&a, &revoke).await["state"], "succeeded");
        drop(a);
        h.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn static_web_vm_local_sdk_keeps_independent_storage_through_expiry_and_restart() {
        let mut h = Harness::start_with_web(false, false, true).await;
        let a = h.client(0).await;
        let b = h.client(1).await;
        let web = call(
            &a,
            "vm_template_build",
            json!({
                "recipe_id":hex::encode(WEB_RECIPE_ID),"request_id":key(101)
            }),
        )
        .await;
        h.hold_build.notify_one();
        let template = terminal(&a, &web).await["template_id"].clone();
        assert!(template.is_string());
        let clone = call(
            &a,
            "vm_create",
            json!({
                "template_id":template,"request_id":key(102),"ttl_s":60
            }),
        )
        .await;
        assert_eq!(clone["expires_at_ms"], UTC + 60_000);
        let test = terminal(&a, &clone).await["vm_id"].clone();

        {
            let mut gate = h.supervisor().gate.lock().unwrap();
            gate.fixture.as_mut().unwrap().host.capacity = Some(Capacity {
                free_bytes: 0,
                managed_bytes: 0,
            });
        }
        assert_eq!(
            call(
                &a,
                "static_vm_create",
                json!({
                    "name":"no-capacity","template_id":template,"request_id":key(110)
                })
            )
            .await["reason"],
            "disk"
        );
        assert_eq!(
            call(&a, "static_vm_list", json!({})).await["entries"],
            json!([])
        );
        let hold = Arc::new(Notify::new());
        {
            let mut gate = h.supervisor().gate.lock().unwrap();
            let fixture = gate.fixture.as_mut().unwrap();
            fixture.host = HOST;
            fixture.static_hold = Some(hold.clone());
        }
        let args = json!({"name":"First-Site","template_id":template,"request_id":key(103)});
        let create = call(&a, "static_vm_create", args.clone()).await;
        let pending = call(&a, "static_vm_list", json!({})).await;
        assert_eq!(pending["entries"][0]["state"], "creating");
        let charges = || {
            let db = rusqlite::Connection::open_with_flags(
                h.temp.path().join("activity.sqlite3"),
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .unwrap();
            db.query_row(
                "SELECT persistent,staging FROM vm_static_instances WHERE name='first-site'",
                [],
                |row| Ok((row.get::<_, u64>(0)?, row.get::<_, u64>(1)?)),
            )
            .unwrap()
        };
        let (persistent, staging) = charges();
        assert_eq!(
            (persistent, staging),
            (65_536 + b"vars".len() as u64 + (1 << 20) + 4096, 65_536)
        );
        assert_eq!(
            call(
                &b,
                "static_vm_create",
                json!({"name":"first-site","template_id":template,
            "request_id":key(104)})
            )
            .await["reason"],
            "name_unavailable"
        );
        hold.notify_one();
        let settled = terminal(&a, &create).await;
        assert_eq!(settled["state"], "succeeded", "{settled}");
        assert_eq!(charges(), (persistent, 0));
        let vm = settled["vm_id"].clone();
        let get = json!({"vm_id":vm});
        let stopped = call(&a, "static_vm_get", get.clone()).await;
        assert_eq!(stopped["name"], "first-site");
        assert_eq!(stopped["state"], "stopped");
        assert!(stopped.get("expires_at_ms").is_none());
        assert_eq!(
            call(
                &a,
                "static_vm_create",
                json!({"name":"first-site","template_id":template,
            "request_id":key(103)})
            )
            .await,
            create
        );
        assert_eq!(
            call(
                &a,
                "static_vm_create",
                json!({"name":"second-site","template_id":template,
            "request_id":key(103)})
            )
            .await["reason"],
            "key_collision"
        );
        assert_eq!(
            call(&b, "static_vm_get", get.clone()).await["reason"],
            "not_found"
        );
        assert_eq!(
            call(&b, "static_vm_list", json!({})).await["entries"],
            json!([])
        );
        assert_eq!(
            call(
                &a,
                "vm_extend",
                json!({"vm_id":vm,"request_id":key(105),"ttl_s":600})
            )
            .await["reason"],
            "not_found"
        );
        for tool in ["static_vm_delete", "backup_delete"] {
            assert_eq!(
                call(&a, tool, get.clone()).await["reason"],
                "static_delete_forbidden"
            );
        }
        let static_files = || {
            state_bytes(&h.media)
                .into_iter()
                .filter(|(path, _)| {
                    path.file_name()
                        .unwrap()
                        .to_string_lossy()
                        .starts_with("static_")
                })
                .collect::<std::collections::BTreeMap<_, _>>()
        };
        let files = static_files();
        assert_eq!(files.len(), 2);
        let start = call(&a, "vm_start", json!({"vm_id":vm,"request_id":key(106)})).await;
        assert_eq!(terminal(&a, &start).await["state"], "succeeded");
        let running = call(&a, "static_vm_get", get.clone()).await;
        assert_eq!(running["state"], "running");
        assert_eq!(running["web_health"], "healthy");
        assert_eq!(running["ready"], true);
        let script = call(
            &a,
            "vm_run_script",
            json!({"vm_id":vm,"request_id":key(107),"script":"exit 7"}),
        )
        .await;
        h.hold_script.notify_one();
        assert_eq!(terminal(&a, &script).await["exit_code"], 7);
        h.supervisor()
            .gate
            .lock()
            .unwrap()
            .fixture
            .as_mut()
            .unwrap()
            .host = HostObservation {
            owner_idle: Some(false),
            logical_cpus: Some(1),
            available_ram: Some(0),
            capacity: None,
        };
        let stop = call(&a, "vm_stop", json!({"vm_id":vm,"request_id":key(108)})).await;
        assert_eq!(terminal(&a, &stop).await["state"], "succeeded");

        h.now.store(UTC + 60_000, Ordering::Relaxed);
        h.tick().await;
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if call(&a, "vm_get", json!({"vm_id":test})).await["state"] == "expired" {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let tombstone = call(&a, "vm_get", json!({"vm_id":test})).await;
        assert_eq!(tombstone["cleanup"], "confirmed");
        let cleanup_id = tombstone["cleanup_activity_id"].as_i64().unwrap();
        let db = rusqlite::Connection::open(h.temp.path().join("activity.sqlite3")).unwrap();
        db.execute(
            "UPDATE activity SET cleanup_confirmed=0 WHERE id=?1",
            [cleanup_id],
        )
        .unwrap();
        drop(db);
        let tombstone = call(&a, "vm_get", json!({"vm_id":test})).await;
        assert_eq!(tombstone["cleanup"], "cleanup_unconfirmed");
        assert_eq!(tombstone["cleanup_activity_id"], cleanup_id);
        assert_eq!(
            call(&a, "list_activity", json!({"activity_id":cleanup_id})).await["activities"],
            json!([])
        );
        assert_eq!(call(&a, "static_vm_get", get.clone()).await, stopped);
        assert_eq!(static_files(), files);
        assert_eq!(charges(), (persistent, 0));

        h.supervisor()
            .gate
            .lock()
            .unwrap()
            .fixture
            .as_mut()
            .unwrap()
            .host = HOST;
        // Change the original fixture through its known private object. A
        // fresh copy now fails before any destination effect, so the
        // untouched candidate releases its name, slot and charges. The
        // committed pair is independent.
        let golden = {
            let gate = h.supervisor().gate.lock().unwrap();
            let fixture = gate.fixture.as_ref().unwrap();
            let live = fixture.live.lock().unwrap();
            let (slots, _) = live.templates[&parse_id(template.as_str().unwrap()).unwrap()];
            fixture.artifacts.0.path(slots.disk.role, slots.disk.key)
        };
        fs::write(golden, b"changed fixture source").unwrap();
        let failed = call(
            &a,
            "static_vm_create",
            json!({
                "name":"failed-copy","template_id":template,"request_id":key(109)
            }),
        )
        .await;
        let failed = terminal(&a, &failed).await;
        assert_eq!(failed["reason"], "clone_failed");
        assert_eq!(failed["cleanup"], "confirmed");
        assert_eq!(static_files(), files);
        let db = rusqlite::Connection::open(h.temp.path().join("activity.sqlite3")).unwrap();
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM vm_static_instances WHERE name='failed-copy'",
                [],
                |row| row.get::<_, u64>(0)
            )
            .unwrap(),
            0
        );
        drop(db);
        assert_eq!(charges(), (persistent, 0));

        let start = call(&a, "vm_start", json!({"vm_id":vm,"request_id":key(111)})).await;
        assert_eq!(terminal(&a, &start).await["state"], "succeeded");
        let stop = call(&a, "vm_stop", json!({"vm_id":vm,"request_id":key(112)})).await;
        assert_eq!(terminal(&a, &stop).await["state"], "succeeded");
        assert_eq!(static_files(), files);
        drop((a, b));
        h.interrupt.take().unwrap().send(()).unwrap();
        h.server.take().unwrap().await.unwrap().unwrap();
        let host = h.supervisor().host_id;
        h.supervisor = None;
        let reopened = Supervisor::new(
            h.temp.path(),
            VmPolicy::default(),
            ForegroundClock {
                utc: Some(h.now.clone()),
            },
            None,
            NativeRefusal::QualificationMissing,
            None,
        )
        .unwrap();
        assert_eq!(reopened.host_id, host);
        let connection = reopened.connection("alpha".into());
        let inventory = connection.inventory();
        let mut expected = stopped;
        expected.as_object_mut().unwrap().remove("ok");
        // The SDK adds the success flag after dispatch.
        assert_eq!(
            inventory
                .dispatch(request("static_vm_get", get), || false)
                .unwrap(),
            expected
        );
        assert_eq!(static_files(), files);
    }

    async fn web_template(h: &Harness, client: &Client, number: u64) -> Value {
        let web = call(
            client,
            "vm_template_build",
            json!({"recipe_id":hex::encode(WEB_RECIPE_ID),"request_id":key(number)}),
        )
        .await;
        h.hold_build.notify_one();
        let template = terminal(client, &web).await["template_id"].clone();
        assert!(template.is_string());
        template
    }

    fn static_paths(media: &Path) -> std::collections::BTreeSet<std::path::PathBuf> {
        state_bytes(media)
            .into_iter()
            .map(|(path, _)| path)
            .filter(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("static_")
            })
            .collect()
    }

    fn read_state(h: &Harness) -> rusqlite::Connection {
        rusqlite::Connection::open_with_flags(
            h.temp.path().join("activity.sqlite3"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap()
    }

    /// (state, persistent, staging) for a named static row, if present.
    fn static_charge(h: &Harness, name: &str) -> Option<(i64, u64, u64)> {
        use rusqlite::OptionalExtension;
        read_state(h)
            .query_row(
                "SELECT state,persistent,staging FROM vm_static_instances WHERE name=?1",
                [name],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .unwrap()
    }

    #[tokio::test]
    async fn static_failed_copies_release_only_proven_untouched_candidates() {
        let mut h = Harness::start_with_web(false, false, true).await;
        let a = h.client(0).await;
        let template = web_template(&h, &a, 201).await;
        let create = |name: &str, number| json!({"name":name,"template_id":template,"request_id":key(number)});

        // The disk leaf is written, the VARS copy fails, and the worker
        // disposes of the exact disk it identified: nothing stays charged.
        h.fixture(|f| f.static_fault = Some(StaticFault::AfterDisk { dispose: true }));
        let failed = call(&a, "static_vm_create", create("site-one", 202)).await;
        let failed = terminal(&a, &failed).await;
        assert_eq!(
            (&failed["state"], &failed["reason"], &failed["cleanup"]),
            (
                &json!("failed"),
                &json!("clone_failed"),
                &json!("confirmed")
            )
        );
        assert_eq!(static_charge(&h, "site-one"), None);
        assert!(static_paths(&h.media).is_empty());

        // Disposal unproven: the identified leaf is quarantined and charged
        // and the candidate keeps its name.
        h.fixture(|f| f.static_fault = Some(StaticFault::AfterDisk { dispose: false }));
        let failed = call(&a, "static_vm_create", create("site-two", 203)).await;
        let failed = terminal(&a, &failed).await;
        assert_eq!(
            (&failed["reason"], &failed["cleanup"]),
            (&json!("clone_failed"), &json!("cleanup_unconfirmed"))
        );
        let (state, persistent, staging) = static_charge(&h, "site-two").unwrap();
        assert_eq!((state, staging), (4, 65_536));
        assert!(persistent > 0);
        assert_eq!(static_paths(&h.media).len(), 1);
        {
            let db = read_state(&h);
            let mut stmt = db
                .prepare(
                    "SELECT a.role,a.state,a.file_id IS NOT NULL FROM vm_artifacts a
                     JOIN vm_generations g ON g.id=a.generation_id
                     JOIN vm_static_instances s ON s.id=g.static_id
                     WHERE s.name='site-two' ORDER BY a.role",
                )
                .unwrap();
            let rows = stmt
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, bool>(2)?,
                    ))
                })
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap();
            assert_eq!(
                rows,
                [
                    ("static_disk".into(), "quarantined".into(), true),
                    ("static_vars".into(), "quarantined".into(), false)
                ]
            );
        }
        assert_eq!(
            call(&a, "static_vm_create", create("site-two", 204)).await["reason"],
            "name_unavailable"
        );

        // An interrupted VARS write has unknown effects: quarantined too.
        h.fixture(|f| f.static_fault = Some(StaticFault::Interrupted));
        let failed = call(&a, "static_vm_create", create("site-three", 205)).await;
        assert_eq!(
            terminal(&a, &failed).await["cleanup"],
            "cleanup_unconfirmed"
        );
        assert_eq!(static_charge(&h, "site-three").unwrap().0, 4);
        assert_eq!(static_paths(&h.media).len(), 3);

        // The released name and instance slot are reusable.
        let created = call(&a, "static_vm_create", create("site-one", 206)).await;
        assert_eq!(terminal(&a, &created).await["state"], "succeeded");
        assert_eq!(static_charge(&h, "site-one").unwrap().0, 1);
        let states = call(&a, "static_vm_list", json!({})).await["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| {
                (
                    row["name"].as_str().unwrap().to_owned(),
                    row["state"].as_str().unwrap().to_owned(),
                )
            })
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            states,
            [
                ("site-one".to_owned(), "stopped".to_owned()),
                ("site-three".to_owned(), "failed".to_owned()),
                ("site-two".to_owned(), "failed".to_owned()),
            ]
            .into()
        );

        // Cancellation before any copy effect releases the candidate too.
        let hold = Arc::new(Notify::new());
        h.fixture(|f| f.static_hold = Some(hold.clone()));
        let cancelled = call(&a, "static_vm_create", create("site-four", 207)).await;
        assert_eq!(cancelled["ok"], true);
        drop(a);
        h.shutdown().await.unwrap();
        assert_eq!(static_charge(&h, "site-four"), None);
        let h = h.reopen(|_| {}).await;
        let a = h.client(0).await;
        let cancelled = call(
            &a,
            "vm_operation",
            json!({"operation_id":cancelled["operation_id"]}),
        )
        .await;
        assert_eq!(
            (&cancelled["reason"], &cancelled["cleanup"]),
            (&json!("cancelled"), &json!("confirmed"))
        );
    }

    #[tokio::test]
    async fn static_vm_recovers_after_clean_and_dirty_shutdown() {
        let mut h = Harness::start_with_web(false, false, true).await;
        let a = h.client(0).await;
        let template = web_template(&h, &a, 301).await;
        let mut ids = vec![];
        for (name, number) in [("site-a", 302), ("site-b", 303)] {
            let created = call(
                &a,
                "static_vm_create",
                json!({"name":name,"template_id":template,"request_id":key(number)}),
            )
            .await;
            let created = terminal(&a, &created).await;
            assert_eq!(created["state"], "succeeded");
            ids.push(created["vm_id"].clone());
        }
        let [site_a, site_b] = [ids[0].clone(), ids[1].clone()];
        let vm_b = parse_id(site_b.as_str().unwrap()).unwrap();
        let lifecycle = |tool: &'static str, vm: &Value, number| {
            (tool, json!({"vm_id":vm,"request_id":key(number)}))
        };
        let start = lifecycle("vm_start", &site_a, 304);
        let started = call(&a, start.0, start.1).await;
        assert_eq!(terminal(&a, &started).await["state"], "succeeded");
        let recovery = |h: &Harness, name: &str| -> (i64, i64, Option<bool>) {
            read_state(h)
                .query_row(
                    "SELECT s.state,a.status,a.cleanup_confirmed FROM vm_static_instances s
                     JOIN activity a ON a.vm=s.id AND a.action=10
                     WHERE s.name=?1 ORDER BY a.id DESC LIMIT 1",
                    [name],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .unwrap()
        };

        // A clean drain while running releases the boot, then settles the
        // same generation as stopped without touching its files.
        let files = static_paths(&h.media);
        drop(a);
        h.shutdown().await.unwrap();
        assert_eq!(recovery(&h, "site-a"), (1, 1, Some(true)));
        assert_eq!(static_paths(&h.media), files);

        // The reopened process rehydrates committed custody, restarts the
        // same VM and admits a later unrelated launch.
        let mut h = h.reopen(|_| {}).await;
        let a = h.client(0).await;
        let get = |vm: &Value| json!({"vm_id":vm});
        assert_eq!(
            call(&a, "static_vm_get", get(&site_a)).await["state"],
            "stopped"
        );
        for (tool, vm, number) in [
            ("vm_start", &site_a, 305),
            ("vm_stop", &site_a, 306),
            ("vm_start", &site_b, 307),
        ] {
            let (tool, args) = lifecycle(tool, vm, number);
            let operation = call(&a, tool, args).await;
            assert_eq!(
                terminal(&a, &operation).await["state"],
                "succeeded",
                "{tool}"
            );
        }
        let running = call(&a, "static_vm_get", get(&site_b)).await;
        assert_eq!(
            (&running["state"], &running["ready"]),
            (&json!("running"), &json!(true))
        );

        // Dirty: the runtime cannot prove release, so the drain is
        // unconfirmed and the VM stays fenced in recovery across reopen.
        h.fixture(|f| f.live.lock().unwrap().stuck.insert(vm_b));
        drop(a);
        assert!(h.shutdown().await.is_err());
        assert_eq!(recovery(&h, "site-b"), (3, 0, Some(false)));
        let h = h
            .reopen(|f| {
                f.live.lock().unwrap().stuck.insert(vm_b);
            })
            .await;
        let a = h.client(0).await;
        assert_eq!(
            call(&a, "static_vm_get", get(&site_b)).await["state"],
            "recovering"
        );
        let (tool, args) = lifecycle("vm_start", &site_a, 308);
        assert_eq!(call(&a, tool, args).await["reason"], "busy");
        let (tool, args) = lifecycle("vm_start", &site_b, 309);
        assert_eq!(call(&a, tool, args).await["reason"], "busy");

        // Once release is proven the same generation returns to stopped.
        h.fixture(|f| f.live.lock().unwrap().stuck.clear());
        h.tick().await;
        tokio::time::timeout(Duration::from_secs(2), async {
            while call(&a, "static_vm_get", get(&site_b)).await["state"] != "stopped" {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(recovery(&h, "site-b"), (1, 1, Some(true)));
        for (tool, vm, number) in [
            ("vm_start", &site_b, 310),
            ("vm_stop", &site_b, 311),
            ("vm_start", &site_a, 312),
            ("vm_stop", &site_a, 313),
        ] {
            let (tool, args) = lifecycle(tool, vm, number);
            let operation = call(&a, tool, args).await;
            assert_eq!(
                terminal(&a, &operation).await["state"],
                "succeeded",
                "{tool}"
            );
        }
        assert_eq!(static_paths(&h.media), files);
    }

    #[test]
    fn candidate_disposal_and_boot_require_the_recorded_object_not_its_content() {
        let temp = crate::service::application::tests::private_temp();
        let media = temp.path().join("fixture-media");
        fs::create_dir(&media).unwrap();
        protect_fixture(&media, true).unwrap();
        let artifacts = Artifacts(Arc::new(FixtureCustody::open(&media).unwrap()));
        let slot = |key: u8| ArtifactSlot {
            generation: [9; 16],
            role: "static_disk",
            key: [key; 16],
        };
        let staged = artifacts.stage(slot(1), b"static leaf").unwrap();
        let path = artifacts.0.path(slot(1).role, slot(1).key);
        // The original moves aside and an identical replacement takes the slot.
        let moved = media.join("moved-original");
        fs::rename(&path, &moved).unwrap();
        let replacement = artifacts.stage(slot(1), b"static leaf").unwrap();
        assert_eq!(replacement.pin, staged.pin);
        assert_ne!(replacement.identity, staged.identity);
        assert!(artifacts.verify(slot(1), staged).is_err());
        assert!(artifacts.remove(slot(1), staged).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"static leaf");
        assert_eq!(fs::read(&moved).unwrap(), b"static leaf");
        // The genuine object is disposed of and proven gone.
        artifacts.verify(slot(1), replacement).unwrap();
        artifacts.remove(slot(1), replacement).unwrap();
        assert!(!path.exists());
        assert!(moved.exists());
        // A recorded directory that differs refuses before any unlink.
        let other = artifacts.stage(slot(2), b"static leaf").unwrap();
        let wrong = ArtifactEvidence {
            directory: [0xee; 16],
            ..other
        };
        assert!(artifacts.remove(slot(2), wrong).is_err());
        assert!(artifacts.0.path(slot(2).role, slot(2).key).exists());
    }

    /// A change after verification and before disposal never removes the
    /// other entry. POSIX moves only a proven name; Windows holds the leaf
    /// exclusively, so the slot cannot change at all.
    #[test]
    fn candidate_disposal_survives_a_replacement_after_verification() {
        let temp = crate::service::application::tests::private_temp();
        let media = temp.path().join("fixture-media");
        fs::create_dir(&media).unwrap();
        protect_fixture(&media, true).unwrap();
        let artifacts = Artifacts(Arc::new(FixtureCustody::open(&media).unwrap()));
        let slot = ArtifactSlot {
            generation: [9; 16],
            role: "static_disk",
            key: [3; 16],
        };
        let staged = artifacts.stage(slot, b"static leaf").unwrap();
        let path = artifacts.0.path(slot.role, slot.key);
        let moved = media.join("moved-original");
        #[cfg_attr(windows, allow(unused_mut))]
        let mut replacement = None;
        let result = artifacts.remove_with(slot, staged, || {
            #[cfg(unix)]
            {
                fs::rename(&path, &moved).unwrap();
                replacement = Some(artifacts.stage(slot, b"static leaf").unwrap());
            }
            #[cfg(windows)]
            {
                assert!(fs::rename(&path, &moved).is_err());
                assert!(fs::remove_file(&path).is_err());
            }
        });
        let names = || {
            fs::read_dir(&media)
                .unwrap()
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .collect::<std::collections::BTreeSet<_>>()
        };
        match replacement {
            Some(replacement) => {
                assert!(result.is_err());
                // The replacement is back in its slot and the original kept.
                artifacts.verify(slot, replacement).unwrap();
                assert_eq!(fs::read(&moved).unwrap(), b"static leaf");
                assert!(!names().iter().any(|name| name.starts_with(".arx-dispose-")));
            }
            None => {
                result.unwrap();
                assert!(!path.exists());
            }
        }
    }

    #[tokio::test]
    async fn static_first_boot_requires_the_recorded_pair() {
        let h = Harness::start_with_web(false, false, true).await;
        let a = h.client(0).await;
        let template = web_template(&h, &a, 401).await;
        h.fixture(|f| f.static_fault = Some(StaticFault::ReplaceBeforeBoot));
        let failed = call(
            &a,
            "static_vm_create",
            json!({"name":"replaced-site","template_id":template,"request_id":key(402)}),
        )
        .await;
        let failed = terminal(&a, &failed).await;
        assert_eq!(
            (&failed["reason"], &failed["cleanup"]),
            (&json!("clone_failed"), &json!("cleanup_unconfirmed"))
        );
        let (state, persistent, staging) = static_charge(&h, "replaced-site").unwrap();
        assert_eq!((state, staging), (4, 65_536));
        assert!(persistent > 0);
        // Both recorded objects and the replacement survive, quarantined.
        assert!(h.media.join("moved-original").exists());
        assert_eq!(static_paths(&h.media).len(), 2);
        let db = read_state(&h);
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM vm_artifacts a JOIN vm_generations g ON g.id=a.generation_id
                 JOIN vm_static_instances s ON s.id=g.static_id
                 WHERE s.name='replaced-site' AND a.state='quarantined' AND a.file_id IS NOT NULL",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            2
        );
    }

    #[tokio::test]
    async fn static_origin_probe_refuses_a_different_generation_name() {
        use crate::vm::static_web::{OriginPortLease, StaticOriginIdentity};
        let identity = FixtureOrigin::new([0x33; 16]).unwrap();
        let lease = OriginPortLease::acquire().unwrap();
        let other = StaticOriginIdentity::for_generation([0x44; 16]).provisioning();
        assert!(
            identity
                .probe(&lease, other.server_name().into())
                .await
                .is_err()
        );
        identity.probe(&lease, identity.name.clone()).await.unwrap();
    }

    #[tokio::test]
    async fn authenticated_windows_fixture_build_and_clone_keep_guest_kind() {
        let mut h = Harness::start_configured(false, false, true, |fixture| {
            fixture.windows_recipe = true;
        })
        .await;
        let client = h.client(0).await;
        let windows = hex::encode(Controller::windows_recipe_id());
        let first = call(&client, "vm_catalog", json!({"limit":1})).await;
        assert_eq!(
            first["entries"],
            json!([{"recipe_id":hex::encode(RECIPE.0),"guest":"ubuntu","state":"buildable"}])
        );
        let rest = call(
            &client,
            "vm_catalog",
            json!({"cursor":first["next_cursor"]}),
        )
        .await;
        assert_eq!(
            rest["entries"],
            json!([
                {"recipe_id":windows,"guest":"windows","state":"buildable"},
                {"recipe_id":hex::encode(WEB_RECIPE_ID),"guest":"ubuntu","state":"buildable"},
            ])
        );
        let build_args = json!({"recipe_id":windows,"request_id":key(90)});
        let build = call(&client, "vm_template_build", build_args.clone()).await;
        assert_eq!(build["ok"], true);
        assert_eq!(call(&client, "vm_template_build", build_args).await, build);
        let before = call(&client, "vm_catalog", json!({})).await;
        assert_eq!(
            before["entries"][3],
            json!({
                "operation_id":build["operation_id"],"recipe_id":windows,"state":"building"
            })
        );
        h.hold_build.notify_one();
        let sealed = terminal(&client, &build).await;
        assert_eq!(sealed["state"], "succeeded", "{sealed}");
        let template = &sealed["template_id"];
        assert_eq!(
            call(&client, "vm_catalog", json!({})).await["entries"][3],
            json!({
                "template_id":template,"recipe_id":windows,"guest":"windows","state":"sealed"
            })
        );
        let create = call(
            &client,
            "vm_create",
            json!({"template_id":template,"request_id":key(91)}),
        )
        .await;
        let cloned = terminal(&client, &create).await;
        let vm = &cloned["vm_id"];
        let get = json!({"vm_id":vm});
        let stopped = call(&client, "vm_get", get.clone()).await;
        assert_eq!(stopped["guest"], "windows");
        assert_eq!(stopped["state"], "stopped");
        assert_eq!(stopped["ready"], false);
        let start = call(
            &client,
            "vm_start",
            json!({"vm_id":vm,"request_id":key(92)}),
        )
        .await;
        assert_eq!(terminal(&client, &start).await["state"], "succeeded");
        assert_eq!(call(&client, "vm_get", get).await["ready"], true);
        let script = call(
            &client,
            "vm_run_script",
            json!({
                "vm_id":vm,"request_id":key(93),"script":"exit 0"
            }),
        )
        .await;
        h.hold_script.notify_one();
        assert_eq!(terminal(&client, &script).await["state"], "succeeded");
        let stop = call(&client, "vm_stop", json!({"vm_id":vm,"request_id":key(94)})).await;
        assert_eq!(terminal(&client, &stop).await["state"], "succeeded");
        client.cancel().await.unwrap();
        h.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn real_local_sdk_sqlite_journey() {
        tokio::time::timeout(Duration::from_secs(8), journey())
            .await
            .unwrap();
    }

    async fn journey() {
        let mut h = Harness::start(false, false).await;
        let a = h.client(0).await;
        let b = h.client(1).await;
        let tools = a.list_tools(None).await.unwrap();
        let names = tools
            .tools
            .iter()
            .map(|tool| tool.name.as_ref())
            .collect::<Vec<_>>();
        assert_eq!(names, ToolName::ALL.map(ToolName::name));
        assert!(
            tools.tools.iter().all(|tool| {
                tool.input_schema.get("additionalProperties") == Some(&json!(false))
            })
        );
        for name in ["run_command", "discover_tools"] {
            assert_eq!(
                call(&a, name, json!({})).await,
                json!({"ok":false,"reason":"host_execution_disabled"})
            );
        }
        assert_eq!(
            call(&a, "no_such_tool", json!({})).await,
            json!({"ok":false,"reason":"unknown_tool"})
        );
        assert_eq!(
            call(&a, "list_hosts", json!({})).await["hosts"][0]["execution"],
            json!({"available":false,"reason":"host_execution_disabled"})
        );
        let rejected = h.connect(&"c".repeat(64)).await;
        assert_eq!(rejected.err().unwrap().to_string(), "peer-auth-rejected");
        #[cfg(windows)]
        let wrong = String::from("S-1-5-18");
        #[cfg(unix)]
        let wrong = (unsafe { libc::geteuid() } ^ 1).to_string();
        let rejected = local::connect_vm_fixture(&h.endpoint.address, &wrong, &h.tokens[0]).await;
        assert_eq!(
            rejected.err().unwrap().to_string(),
            "peer-connect-or-identity"
        );

        let recipe = hex::encode(RECIPE.0);
        let build_args = json!({"recipe_id":recipe,"request_id":key(1)});
        let call_build = |id: u64| {
            json!({"jsonrpc":"2.0","id":id,"method":"tools/call",
                "params":{"name":"vm_template_build","arguments":build_args}})
        };

        // MCP cancellation through the SDK before admission opens no work
        // and leaves the session usable.
        let mut raw = h.raw().await;
        let (entered, release, done) = h.hold();
        wire(&mut raw, call_build(7)).await;
        entered.notified().await;
        wire(
            &mut raw,
            json!({"jsonrpc":"2.0","method":"notifications/cancelled",
            "params":{"requestId":7,"reason":"fixture"}}),
        )
        .await;
        wire(&mut raw, json!({"jsonrpc":"2.0","id":8,"method":"ping"})).await;
        assert!(reply(&mut raw, 8).await["result"].is_object());
        release.notify_one();
        done.await.unwrap();
        // EOF while a call waits for admission closes that connection's path.
        let (entered, release, done) = h.hold();
        let closed = h.supervisor().closed_connections.load(Ordering::Relaxed);
        wire(&mut raw, call_build(9)).await;
        entered.notified().await;
        drop(raw);
        tokio::time::timeout(Duration::from_secs(2), async {
            while h.supervisor().closed_connections.load(Ordering::Relaxed) == closed {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        release.notify_one();
        done.await.unwrap();
        let refusals = h.audit();
        assert_eq!(refusals.len(), 3, "cancelled calls opened work");
        // Two host-execution refusals and one unknown tool name, each a
        // bounded uncorrelated row that stores no caller-supplied label.
        assert!(refusals.iter().all(|row| row.action == HOST_REFUSAL_ACTION
            && row.vm.is_none()
            && row.operation.is_none()));
        let mut reasons = refusals.iter().map(|row| row.reason).collect::<Vec<_>>();
        let mut expected = vec![
            Some(Refusal::HostExecutionDisabled as i64),
            Some(Refusal::HostExecutionDisabled as i64),
            Some(Refusal::UnknownTool as i64),
        ];
        reasons.sort();
        expected.sort();
        assert_eq!(reasons, expected);
        // Direct detached handlers obey the same closed flags.
        let closed = h.supervisor().connection("alpha".into());
        let detached = closed.inventory();
        drop(closed);
        assert_eq!(
            detached
                .dispatch(request("vm_template_build", build_args.clone()), || false)
                .unwrap_err()
                .0,
            "shutdown"
        );
        let active = h.supervisor().connection("alpha".into());

        let first = call(&a, "vm_catalog", json!({})).await;
        assert_eq!(
            first,
            json!({"ok":true,"arch":"x86_64","capability":{"available":true},
                "entries":[{"recipe_id":recipe,"guest":"ubuntu","state":"buildable"}]})
        );
        let build = call(&a, "vm_template_build", build_args.clone()).await;
        let pending = call(&a, "vm_catalog", json!({"limit":1})).await;
        let cursor = pending["next_cursor"].as_str().unwrap().to_owned();
        let second = call(&a, "vm_catalog", json!({"cursor":cursor})).await;
        assert_eq!(
            second["entries"],
            json!([{"operation_id":build["operation_id"],"recipe_id":recipe,"state":"building"}])
        );
        assert!(second.get("next_cursor").is_none());
        assert_eq!(
            call(&b, "vm_catalog", json!({})).await["entries"],
            first["entries"]
        );
        let mut altered = cursor.clone();
        altered.replace_range(0..1, "Z");
        for (peer, tool, item) in [
            (&b, "vm_catalog", cursor.as_str()),
            (&a, "vm_list", cursor.as_str()),
            (&a, "vm_catalog", altered.as_str()),
        ] {
            assert_eq!(
                call(peer, tool, json!({"cursor":item})).await,
                json!({"ok":false,"reason":"invalid_cursor"})
            );
        }
        let operation = json!({"operation_id":build["operation_id"]});
        assert_eq!(
            call(&b, "vm_operation", operation.clone()).await,
            json!({"ok":false,"reason":"not_found"})
        );
        assert_eq!(
            call(&a, "vm_operation", operation).await,
            json!({"ok":true,"operation_id":build["operation_id"],"action":"template_build",
                "state":"pending","stage":"staging","recipe_id":recipe})
        );
        h.hold_build.notify_one();
        let built = terminal(&a, &build).await;
        let template = built["template_id"].clone();
        assert_eq!(
            built,
            json!({"ok":true,"operation_id":build["operation_id"],"action":"template_build",
                "state":"succeeded","cleanup":"confirmed","recipe_id":recipe,"template_id":template})
        );
        let reused = call(&b, "vm_template_build", build_args.clone()).await;
        assert_ne!(reused["operation_id"], build["operation_id"]);
        assert_eq!(terminal(&b, &reused).await["template_id"], template);

        // Scope and subject state precede native or fixture eligibility.
        assert_eq!(
            call(
                &a,
                "vm_create",
                json!({"template_id":"0".repeat(32),"request_id":key(20)})
            )
            .await,
            json!({"ok":false,"reason":"not_found"})
        );
        let create = json!({"template_id":template,"request_id":key(2),"ttl_s":60});
        let clone = call(&a, "vm_create", create.clone()).await;
        let clone_result = terminal(&a, &clone).await;
        let vm = clone_result["vm_id"].clone();
        assert_eq!(
            clone_result,
            json!({"ok":true,"operation_id":clone["operation_id"],"action":"create",
                "state":"succeeded","cleanup":"confirmed","vm_id":vm,"template_id":template,"expires_at_ms":UTC+60_000})
        );
        assert_eq!(call(&a, "vm_create", create).await, clone);
        assert_eq!(
            call(
                &a,
                "vm_create",
                json!({"template_id":template,"request_id":key(2),"ttl_s":61})
            )
            .await,
            json!({"ok":false,"reason":"key_collision"})
        );
        let other = call(
            &a,
            "vm_create",
            json!({"template_id":template,"request_id":key(21)}),
        )
        .await;
        let other_vm = terminal(&a, &other).await["vm_id"].clone();
        let get = json!({"vm_id":vm});
        assert_eq!(
            call(&a, "vm_get", get.clone()).await,
            json!({"ok":true,"vm_id":vm,"template_id":template,"guest":"ubuntu",
                "state":"stopped","ready":false,"expires_at_ms":UTC+60_000,"snapshots":[]})
        );
        // Two caller-owned VMs page with a principal- and tool-bound cursor.
        let page = call(&a, "vm_list", json!({"limit":1})).await;
        let list_cursor = page["next_cursor"].as_str().unwrap().to_owned();
        let rest = call(&a, "vm_list", json!({"cursor":list_cursor})).await;
        let mut seen = [
            page["entries"][0]["vm_id"].clone(),
            rest["entries"][0]["vm_id"].clone(),
        ];
        seen.sort_by_key(|id| id.to_string());
        let mut expected = [vm.clone(), other_vm.clone()];
        expected.sort_by_key(|id| id.to_string());
        assert_eq!(seen, expected);
        assert!(rest.get("next_cursor").is_none());
        assert_eq!(
            call(&b, "vm_list", json!({"cursor":list_cursor})).await["reason"],
            "invalid_cursor"
        );
        assert_eq!(call(&b, "vm_get", get.clone()).await["reason"], "not_found");
        assert_eq!(call(&b, "vm_list", json!({})).await["entries"], json!([]));
        for (tool, args) in [
            ("vm_stop", json!({"vm_id":vm,"request_id":key(3)})),
            ("vm_start", json!({"vm_id":vm,"request_id":key(3)})),
            (
                "vm_extend",
                json!({"vm_id":vm,"request_id":key(3),"ttl_s":60}),
            ),
        ] {
            assert_eq!(
                call(&b, tool, args).await,
                json!({"ok":false,"reason":"not_found"})
            );
        }
        assert_eq!(
            call(
                &a,
                "vm_run_script",
                json!({"vm_id":vm,"request_id":key(3),"script":"exit 0"})
            )
            .await,
            json!({"ok":false,"reason":"not_ready"})
        );
        let start = call(&a, "vm_start", json!({"vm_id":vm,"request_id":key(4)})).await;
        assert_eq!(
            terminal(&a, &start).await,
            json!({"ok":true,"operation_id":start["operation_id"],"action":"start",
                "state":"succeeded","cleanup":"confirmed","vm_id":vm})
        );
        assert_eq!(call(&a, "vm_get", get.clone()).await["ready"], true);

        // A committed script survives the caller's lost reply and disconnect.
        let mut raw = h.raw().await;
        let script_args = json!({"vm_id":vm,"request_id":key(5),"script":"exit 7"});
        wire(
            &mut raw,
            json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
            "params":{"name":"vm_run_script","arguments":script_args}}),
        )
        .await;
        let pending = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let row =
                    call(&a, "list_activity", json!({"limit":1})).await["activities"][0].clone();
                if row["action"] == "run_script" && row["state"] == "pending" {
                    break row;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        drop(raw);
        // The channel stays healthy while its one exchange slot is held.
        assert_eq!(call(&a, "vm_get", get.clone()).await["ready"], true);
        let script = call(
            &a,
            "vm_run_script",
            json!({"vm_id":vm,"request_id":key(5),"script":"exit 0","timeout_s":1}),
        )
        .await;
        assert_eq!(script["operation_id"], pending["operation_id"]);
        let audit = h.audit().len();
        for args in [
            json!({"vm_id":vm,"request_id":key(6),"script":null}),
            json!({"vm_id":vm,"request_id":key(6),"script":"\0"}),
            json!({"vm_id":vm,"request_id":key(6),"script":"é".repeat(2049)}),
            json!({"vm_id":vm,"request_id":key(6),"script":"exit 0","timeout_s":1.5}),
            json!({"vm_id":vm,"request_id":key(6),"script":"exit 0","timeout_s":"30"}),
            json!({"vm_id":vm,"request_id":key(6),"script":"exit 0","timeout_s":121}),
            json!({"vm_id":vm,"request_id":key(6).to_uppercase(),"script":"exit 0"}),
            json!({"vm_id":"F".repeat(32),"request_id":key(6),"script":"exit 0"}),
            json!({"vm_id":vm,"request_id":key(6),"script":"exit 0","interpreter":"sh"}),
        ] {
            assert_eq!(
                call(&a, "vm_run_script", args).await,
                json!({"ok":false,"reason":"invalid"})
            );
        }
        // Trusted invalid calls are audited once each, correlated only to
        // the caller-owned VM; the foreign-shaped ID stays uncorrelated.
        let rows = h.audit();
        assert_eq!(rows.len(), audit + 9, "one refusal per trusted call");
        assert_eq!(rows[..9].iter().filter(|row| row.vm.is_none()).count(), 1);
        assert!(
            rows[..9]
                .iter()
                .all(|row| { row.state == 4 && row.reason == Some(Refusal::Invalid as i64) })
        );
        let mut raw = h.raw().await;
        for (id, name, arguments) in [
            (2, "vm_template_build", Value::Null),
            (3, "vm_list", json!("x")),
            (4, "vm_list", json!([])),
        ] {
            wire(
                &mut raw,
                json!({"jsonrpc":"2.0","id":id,"method":"tools/call",
                "params":{"name":name,"arguments":arguments}}),
            )
            .await;
            let result = reply(&mut raw, id).await["result"].clone();
            assert_eq!(result["isError"], true);
            assert_eq!(
                result["structuredContent"],
                json!({"ok":false,"reason":"invalid"})
            );
        }
        drop(raw);
        let rows = h.audit();
        assert_eq!(rows.len(), audit + 12);
        for (row, action) in
            rows.iter()
                .zip([READ_REFUSAL_ACTION, READ_REFUSAL_ACTION, BUILD_ACTION])
        {
            assert_eq!(
                (row.action, row.state, row.reason, row.vm),
                (action, 4, Some(Refusal::Invalid as i64), None)
            );
        }

        h.hold_script.notify_one();
        let done = terminal(&a, &script).await;
        assert_eq!(
            done,
            json!({"ok":true,"operation_id":script["operation_id"],"action":"run_script",
                "state":"failed","cleanup":"confirmed","vm_id":vm,"script_status":"exited",
                "exit_code":7})
        );
        let stop_vm = call(&a, "vm_stop", json!({"vm_id":vm,"request_id":key(7)})).await;
        assert_eq!(
            terminal(&a, &stop_vm).await,
            json!({"ok":true,"operation_id":stop_vm["operation_id"],"action":"stop",
                "state":"succeeded","cleanup":"confirmed","vm_id":vm})
        );
        assert_eq!(
            call(&a, "vm_stop", json!({"vm_id":vm,"request_id":key(8)})).await,
            json!({"ok":false,"reason":"busy"})
        );
        let extend = call(
            &a,
            "vm_extend",
            json!({"vm_id":vm,"request_id":key(9),"ttl_s":120}),
        )
        .await;
        assert_eq!(extend["expires_at_ms"], UTC + 120_000);
        assert_eq!(
            call(
                &a,
                "vm_extend",
                json!({"vm_id":vm,"request_id":key(9),"ttl_s":180})
            )
            .await,
            extend
        );
        assert_eq!(
            call(
                &a,
                "vm_operation",
                json!({"operation_id":extend["operation_id"]})
            )
            .await,
            json!({"ok":true,"operation_id":extend["operation_id"],"action":"extend",
                "state":"succeeded","vm_id":vm,"expires_at_ms":UTC+120_000})
        );

        // Expiry through the supervised reaper runner on an injected tick.
        h.now.store(UTC + 120_000, Ordering::Relaxed);
        assert_eq!(call(&a, "vm_get", get.clone()).await["state"], "teardown");
        h.tick().await;
        let tombstone = json!({"ok":true,"vm_id":vm,"state":"expired","ready":false,
            "expired_at_ms":UTC+120_000,"cleanup":"confirmed","snapshots":[]});
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let mut current = call(&a, "vm_get", get.clone()).await;
                if current["state"] == "expired" {
                    assert!(
                        current
                            .as_object_mut()
                            .unwrap()
                            .remove("cleanup_activity_id")
                            .and_then(|value| value.as_i64())
                            .is_some_and(|id| id > 0)
                    );
                    assert_eq!(current, tombstone);
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(call(&b, "vm_get", get.clone()).await["reason"], "not_found");
        assert_eq!(
            call(&a, "vm_start", json!({"vm_id":vm,"request_id":key(10)})).await,
            json!({"ok":false,"reason":"expired"})
        );
        let listed = call(&a, "vm_list", json!({})).await["entries"].clone();
        assert_eq!(listed.as_array().unwrap().len(), 1);
        assert_eq!(listed[0]["vm_id"], other_vm);
        // The golden pair and the live clone remain; the expired pair is gone.
        assert_eq!(fs::read_dir(&h.media).unwrap().count(), 4);

        let activity = call(
            &a,
            "list_activity",
            json!({"limit":50,"include_reads":true}),
        )
        .await;
        assert!(
            activity["activities"]
                .as_array()
                .unwrap()
                .iter()
                .all(|row| row["identity_mode"] == "local_vm_principal")
        );
        let own = activity["activities"][0]["activity_id"].clone();
        assert_eq!(
            call(&b, "list_activity", json!({"activity_id":own})).await["activities"],
            json!([])
        );
        let files = state_bytes(h.temp.path());
        assert!(
            files
                .iter()
                .any(|(path, _)| path.ends_with("activity.sqlite3"))
        );
        for sentinel in [
            "exit 7",
            "exit 0",
            h.tokens[0].as_str(),
            h.tokens[1].as_str(),
            h.media.to_str().unwrap(),
        ] {
            assert!(!activity.to_string().contains(sentinel));
            for (path, bytes) in &files {
                assert!(
                    !bytes
                        .windows(sentinel.len())
                        .any(|window| window == sentinel.as_bytes()),
                    "sentinel in {}",
                    path.display()
                );
            }
        }

        let audit = h.audit().len();
        frame_cases(&h).await;
        assert_eq!(h.audit().len(), audit, "untrusted frames were audited");
        // A trusted call with non-object arguments at the exact frame limit
        // keeps its classification through the pump's rewrite, including the
        // one-byte value that grows the most. One byte more is untrusted.
        let exact = |arguments: &str, extra: usize| {
            let fixed = format!(
                r#"{{"jsonrpc":"2.0","id":"","method":"tools/call","params":{{"name":"vm_template_build","arguments":{arguments}}}}}"#
            );
            let long_id = "x".repeat(local::FRAME_LIMIT - fixed.len() - 1 + extra);
            let frame = fixed.replacen(r#""id":"""#, &format!(r#""id":"{long_id}""#), 1);
            assert_eq!(frame.len() + 1, local::FRAME_LIMIT + extra);
            (format!("{frame}\n"), long_id)
        };
        for (count, arguments) in [(1, "null"), (2, "0")] {
            let mut raw = h.raw().await;
            let (frame, long_id) = exact(arguments, 0);
            raw.get_mut().write_all(frame.as_bytes()).await.unwrap();
            let mut line = String::new();
            assert!(
                raw.read_line(&mut line).await.unwrap() > 0,
                "the exact-limit trusted call was dropped"
            );
            let response: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(response["id"], long_id);
            assert_eq!(response["result"]["isError"], true);
            assert_eq!(
                response["result"]["structuredContent"],
                json!({"ok":false,"reason":"invalid"})
            );
            let rows = h.audit();
            assert_eq!(rows.len(), audit + count);
            // The extra receipt is exactly the uncorrelated build refusal.
            assert_eq!(
                (rows[0].action, rows[0].state, rows[0].reason, rows[0].vm),
                (BUILD_ACTION, 4, Some(Refusal::Invalid as i64), None)
            );
            assert_eq!((rows[0].operation, rows[0].cleanup), (None, None));
        }
        let mut raw = h.raw().await;
        let _ = raw.get_mut().write_all(exact("0", 1).0.as_bytes()).await;
        let mut line = String::new();
        assert!(
            matches!(raw.read_line(&mut line).await, Ok(0) | Err(_)),
            "an oversized rewrite-leg frame had a reply"
        );
        assert_eq!(h.audit().len(), audit + 2, "an oversized frame was audited");
        drop(raw);

        // Ctrl+C equivalent: drain, join and recover, then refuse detached work.
        h.interrupt.take().unwrap().send(()).unwrap();
        h.server.take().unwrap().await.unwrap().unwrap();
        assert_eq!(
            active
                .inventory()
                .dispatch(request("vm_template_build", build_args.clone()), || false)
                .unwrap_err()
                .0,
            "shutdown"
        );
        drop((a, b, active, detached));

        // A restart keeps durable receipts but invalidates old cursors.
        h.supervisor = None;
        let reopened = Supervisor::new(
            h.temp.path(),
            VmPolicy::default(),
            ForegroundClock {
                utc: Some(h.now.clone()),
            },
            Some(RECIPE),
            NativeRefusal::QualificationMissing,
            None,
        )
        .unwrap();
        let connection = reopened.connection("alpha".into());
        let inventory = connection.inventory();
        assert_eq!(
            inventory
                .dispatch(request("vm_list", json!({"cursor":list_cursor})), || false)
                .unwrap_err()
                .0,
            "invalid_cursor"
        );
        assert_eq!(
            inventory
                .dispatch(
                    request(
                        "vm_operation",
                        json!({"operation_id":build["operation_id"]})
                    ),
                    || false
                )
                .unwrap()["template_id"],
            template
        );
    }

    async fn frame_cases(h: &Harness) {
        const PREFIX: &[u8] = b"{\"jsonrpc\":\"2.0\",\"method\":\"tools/call\",\"id\":1,\"params\":{\"name\":\"vm_list\",\"arguments\":{}}}";
        for (frame, valid) in [
            ([PREFIX, &vec![b' '; local::FRAME_LIMIT - PREFIX.len() - 1], b"\n"].concat(), true),
            ([PREFIX, &vec![b' '; local::FRAME_LIMIT - PREFIX.len()], b"\n"].concat(), false),
            (b"{\"jsonrpc\":\"2.0\",\"method\":\"tools/call\",\"id\":1,\"params\":{\"name\":\"vm_list\",\"arguments\":{\"limit\":1,\"l\\u0069mit\":2}}}\n".to_vec(), false),
            (b"{\"jsonrpc\":\"2.0\",\"method\":\"tools/call\",\"id\":1,\"params\":{\"name\":\"vm_template_build\",\"arguments\":null},\"params\":{}}\n".to_vec(), false),
            ([b"{\"text\":\"".as_slice(), &[255], b"\"}\n"].concat(), false),
            ([vec![b'['; 17], vec![b']'; 17], vec![b'\n']].concat(), false),
            ([PREFIX, b" {}\n"].concat(), false),
        ] {
            let mut raw = h.raw().await;
            let _ = raw.get_mut().write_all(&frame).await;
            let mut line = String::new();
            let read = raw.read_line(&mut line).await;
            if valid {
                assert!(read.unwrap() > 0);
                assert_eq!(serde_json::from_str::<Value>(&line).unwrap()["result"]["structuredContent"]["ok"], true);
            } else {
                assert!(matches!(read, Ok(0) | Err(_)), "rejected frame had a reply");
            }
        }
    }

    #[tokio::test]
    async fn fatal_listener_failure_drains_and_drain_failure_takes_precedence() {
        // Both orderings: the listener failure wins the select, or an
        // interrupt wins after the listener failed. The fatal category is
        // kept either way; an unresolved drain still takes precedence.
        for (fail_cancelled, interrupt_on_close, category) in [
            (false, false, "server-listener"),
            (true, false, "server-drain-unconfirmed"),
            (false, true, "server-listener"),
            (true, true, "server-drain-unconfirmed"),
        ] {
            let mut h = Harness::start(fail_cancelled, interrupt_on_close).await;
            let a = h.client(0).await;
            let build = call(
                &a,
                "vm_template_build",
                json!({"recipe_id":hex::encode(RECIPE.0),"request_id":key(1)}),
            )
            .await;
            assert_eq!(build["ok"], true, "{build}");
            // The outstanding worker is held when the listener fails.
            h.trip.notify_one();
            let result = h.server.take().unwrap().await.unwrap();
            assert_eq!(result.unwrap_err().to_string(), category);
            let operation =
                Id::try_from(hex::decode(build["operation_id"].as_str().unwrap()).unwrap())
                    .unwrap();
            let status = h
                .supervisor()
                .controller
                .operation_status("alpha", operation)
                .unwrap()
                .unwrap();
            // A build cancelled before it touched storage settles as failed
            // with confirmed cleanup (4c2a `cancel_untouched_build`).
            assert_eq!(
                (status.state, status.cleanup),
                (Status::Failed, Some("confirmed"))
            );
            assert!(call_after_close(&a).await, "admission survived the failure");
        }
    }

    /// The supervisor side of a stall (dispatch, sweep or recovery): signals
    /// entry, blocks its blocking-pool thread until released (by send or by
    /// dropping the sender), and signals completion when the stalled work
    /// returns on any path.
    pub(super) struct StallHook {
        entered: tokio::sync::oneshot::Sender<()>,
        release: std::sync::mpsc::Receiver<()>,
        completed: tokio::sync::oneshot::Sender<()>,
    }

    pub(super) struct Completed(Option<tokio::sync::oneshot::Sender<()>>);

    impl Drop for Completed {
        fn drop(&mut self) {
            if let Some(completed) = self.0.take() {
                let _ = completed.send(());
            }
        }
    }

    impl StallHook {
        pub(super) fn enter(self) -> Completed {
            let _ = self.entered.send(());
            let _ = self.release.recv();
            Completed(Some(self.completed))
        }
    }

    /// The test side. Dropping it releases the stall on every test outcome.
    struct Stall {
        entered: tokio::sync::oneshot::Receiver<()>,
        release: Option<std::sync::mpsc::Sender<()>>,
        completed: tokio::sync::oneshot::Receiver<()>,
    }

    impl Stall {
        fn install(slot: &Mutex<Option<StallHook>>) -> Self {
            let (entered, entered_rx) = tokio::sync::oneshot::channel();
            let (release, release_rx) = std::sync::mpsc::channel();
            let (completed, completed_rx) = tokio::sync::oneshot::channel();
            *slot.lock().unwrap() = Some(StallHook {
                entered,
                release: release_rx,
                completed,
            });
            Self {
                entered: entered_rx,
                release: Some(release),
                completed: completed_rx,
            }
        }

        async fn entered(&mut self) {
            tokio::time::timeout(Duration::from_secs(5), &mut self.entered)
                .await
                .expect("stalled work was never entered")
                .unwrap();
        }

        fn not_entered(&mut self) -> bool {
            matches!(
                self.entered.try_recv(),
                Err(tokio::sync::oneshot::error::TryRecvError::Empty)
            )
        }

        /// Release the stalled work and wait until it returned.
        async fn release_and_join(mut self) {
            drop(self.release.take());
            tokio::time::timeout(Duration::from_secs(5), &mut self.completed)
                .await
                .expect("released work did not complete")
                .unwrap();
        }
    }

    struct Never;

    impl local::Acceptor for Never {
        type Stream = DuplexStream;
        async fn accept(&mut self) -> Result<Option<DuplexStream>> {
            std::future::pending().await
        }
    }

    fn idle_supervisor(temp: &tempfile::TempDir) -> Arc<Supervisor> {
        Supervisor::new(
            temp.path(),
            VmPolicy::default(),
            ForegroundClock { utc: None },
            None,
            NativeRefusal::Disabled,
            None,
        )
        .unwrap()
    }

    /// Serve with an idle listener, interrupt at once and return the
    /// teardown result, which must arrive well within the outer bound.
    async fn interrupted_teardown(supervisor: &Arc<Supervisor>) -> String {
        let started = Instant::now();
        let result = tokio::time::timeout(
            Duration::from_secs(4),
            local::serve_vm_profile_within(
                Never,
                Arc::new(Config::default()),
                supervisor.clone(),
                async { Ok(()) },
                Duration::from_millis(200),
            ),
        )
        .await
        .expect("teardown outlived its stage bounds");
        assert!(started.elapsed() < Duration::from_secs(3));
        result.unwrap_err().to_string()
    }

    #[tokio::test]
    async fn stalled_final_recovery_is_bounded() {
        let temp = crate::service::application::tests::private_temp();
        let supervisor = idle_supervisor(&temp);
        // Directly: the stage returns at its deadline, not when work ends.
        let mut stall = Stall::install(&supervisor.stall_recovery);
        supervisor.close();
        let result = tokio::time::timeout(
            Duration::from_secs(2),
            supervisor.recover(Duration::from_millis(50)),
        )
        .await
        .expect("final recovery outlived its bound");
        assert_eq!(result.unwrap_err().to_string(), "VM recovery unconfirmed");
        stall.entered().await;
        stall.release_and_join().await;

        // Composed: a stalled recovery makes the whole teardown unconfirmed.
        let temp = crate::service::application::tests::private_temp();
        let supervisor = idle_supervisor(&temp);
        let mut stall = Stall::install(&supervisor.stall_recovery);
        assert_eq!(
            interrupted_teardown(&supervisor).await,
            "server-drain-unconfirmed"
        );
        stall.entered().await;
        stall.release_and_join().await;
    }

    #[tokio::test]
    async fn stuck_close_gate_cannot_hold_teardown_or_reach_recovery() {
        let temp = crate::service::application::tests::private_temp();
        let supervisor = idle_supervisor(&temp);
        let mut stall = Stall::install(&supervisor.stall_recovery);
        let (locked, held) = std::sync::mpsc::channel();
        let (unlock, release) = std::sync::mpsc::channel::<()>();
        let holder = std::thread::spawn({
            let supervisor = supervisor.clone();
            move || {
                let _gate = supervisor.gate.lock().unwrap();
                locked.send(()).unwrap();
                let _ = release.recv();
            }
        });
        held.recv().unwrap();
        let result = interrupted_teardown(&supervisor).await;
        assert_eq!(result, "server-drain-unconfirmed");
        assert!(
            stall.not_entered(),
            "recovery ran past an unresolved close barrier"
        );
        drop(unlock);
        holder.join().unwrap();
        drop(stall);
    }

    /// Failure-path bound for a one-worker runtime whose own timers would stop
    /// if the scheduler were blocked: a thread that releases the stall only if
    /// the test has not disarmed it within the bound. The success path never
    /// waits on it.
    struct Watchdog {
        disarm: std::sync::mpsc::Sender<()>,
        thread: std::thread::JoinHandle<bool>,
    }

    impl Watchdog {
        fn arm(release: std::sync::mpsc::Sender<()>) -> Self {
            let (disarm, disarmed) = std::sync::mpsc::channel::<()>();
            let thread = std::thread::spawn(move || {
                let fired = matches!(
                    disarmed.recv_timeout(Duration::from_secs(4)),
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout)
                );
                drop(release);
                fired
            });
            Self { disarm, thread }
        }

        /// Disarm, release the stall and report whether the bound fired.
        fn disarm(self) -> bool {
            let _ = self.disarm.send(());
            self.thread.join().unwrap()
        }
    }

    /// With a single Tokio worker, a dispatch stalled inside the gate (as on
    /// a stuck SQLite write) or a stalled periodic reaper turn must not stop
    /// the teardown deadlines: the result is `server-drain-unconfirmed`
    /// while the stall is still held.
    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn one_worker_stalled_dispatch_or_reaper_turn_cannot_hold_shutdown() {
        for reaper in [false, true] {
            let temp = crate::service::application::tests::private_temp();
            let supervisor = idle_supervisor(&temp);
            let ticks = reaper.then(|| {
                let (ticks, receiver) = tokio::sync::mpsc::unbounded_channel();
                *supervisor.ticks.lock().unwrap() = Some(TestTicks(receiver));
                ticks
            });
            let mut stall = Stall::install(if reaper {
                &supervisor.stall_sweep
            } else {
                &supervisor.stall_dispatch
            });
            // Armed before the stall can be entered, so even a blocked sole
            // worker is released within the bound.
            let watchdog = Watchdog::arm(stall.release.take().unwrap());
            let endpoint = Endpoint {
                #[cfg(unix)]
                address: temp.path().join("vm.sock").to_str().unwrap().into(),
                #[cfg(windows)]
                address: format!(
                    r"\\.\pipe\arxcovo-mcp-{}",
                    hex::encode(crate::store::vm::new_id().unwrap())
                ),
                client_identity: local::vm_supervisor_identity().unwrap(),
            };
            let token = "a".repeat(64);
            let config = Arc::new(Config {
                agents: vec![Agent {
                    principal: "alpha".into(),
                    token_sha256: hex::encode(Sha256::digest(token.as_bytes())),
                }],
                ..Config::default()
            });
            let (interrupt, interrupted) = tokio::sync::oneshot::channel::<()>();
            let server = tokio::spawn(local::serve_vm_profile_within(
                Listener::bind(&endpoint).unwrap(),
                config,
                supervisor.clone(),
                async move {
                    let _ = interrupted.await;
                    Ok(())
                },
                Duration::from_millis(200),
            ));
            let call = match &ticks {
                Some(ticks) => {
                    ticks.send(()).unwrap();
                    None
                }
                None => {
                    let stream = local::connect_vm_fixture(
                        &endpoint.address,
                        &endpoint.client_identity,
                        &token,
                    )
                    .await
                    .unwrap();
                    let (read, write) = split(stream);
                    let client: Client =
                        ().serve(AsyncRwTransport::new_client(read, write))
                            .await
                            .unwrap();
                    Some(tokio::spawn(async move {
                        let _ = client.call_tool(request("vm_catalog", json!({}))).await;
                        client
                    }))
                }
            };
            stall.entered().await;
            interrupt.send(()).unwrap();
            let result = server.await.unwrap();
            let fired = watchdog.disarm();
            assert!(
                !fired,
                "teardown waited for the stalled work (reaper={reaper})"
            );
            assert_eq!(
                result.unwrap_err().to_string(),
                "server-drain-unconfirmed",
                "reaper={reaper}"
            );
            stall.release_and_join().await;
            if let Some(call) = call {
                drop(call.await.unwrap());
            }
        }
    }

    /// After teardown the client's session is gone or refuses fresh work.
    async fn call_after_close(client: &Client) -> bool {
        match client.call_tool(request("vm_catalog", json!({}))).await {
            Err(_) => true,
            Ok(result) => result.structured_content.unwrap()["reason"] == "shutdown",
        }
    }
}
