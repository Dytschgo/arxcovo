//! Foreground-only VM tools. This module is selected after local authentication.
#[cfg(all(windows, target_arch = "x86_64"))]
mod native_run;
mod owned_worker;
mod selected_static;
#[cfg(all(windows, target_arch = "x86_64"))]
mod selected_test;
mod vault_test;
use crate::{
    config::{TestVms, VmRuntimeProfile},
    service::Shutdown,
    store::ListActivity,
    vm::{
        runtime::{
            NativeRefusal,
            publication::{Drained, Origin, Profile, Publication},
        },
        snapshot::{Request as SnapshotRequest, SnapshotName},
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
struct Fault(&'static str, Retry);

/// Whether a refusal advertises a retry hint on the wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Retry {
    Never,
    /// The next production reaper sweep is expected to clear the refusal.
    AfterReaperSweep,
}

impl Fault {
    const fn new(reason: &'static str) -> Self {
        Self(reason, Retry::Never)
    }
}

const INVALID: Fault = Fault::new("invalid");
const INVALID_CURSOR: Fault = Fault::new("invalid_cursor");
const SHUTDOWN: Fault = Fault::new("shutdown");
const NOT_FOUND: Fault = Fault::new("not_found");
const STATE_UNAVAILABLE: Fault = Fault::new("state_unavailable");
const NOT_READY: Fault = Fault::new("not_ready");
const HOST_EXECUTION_DISABLED: Fault = Fault::new("host_execution_disabled");

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
        Refusal::SnapshotPublishNotQuiesced => "snapshot_publish_not_quiesced",
        Refusal::StaticBackupDeleteForbidden => "static_backup_delete_forbidden",
        Refusal::SnapshotOfflineRequired => "snapshot_offline_required",
        Refusal::ComposeRecoveryRequired => "compose_recovery_required",
        Refusal::BackupLimit => "backup_limit",
    }
}

/// One production reaper period, checked at compile time so a future
/// cadence can never silently truncate.
const REAPER_RETRY_AFTER_MS: u64 = {
    let millis = crate::vm::reaper::PERIOD.as_millis();
    assert!(
        millis <= u64::MAX as u128,
        "reaper period overflows retry_after_ms"
    );
    millis as u64
};

/// `vm_snapshot_delete` refused only by accounted post-restore retire work
/// that the reaper is authorized to dispose (`Refused::Retiring`). Same wire
/// reason and audit code as every other `cleanup_unconfirmed`.
const SNAPSHOT_DELETE_RETIRING: Fault = Fault("cleanup_unconfirmed", Retry::AfterReaperSweep);

/// The wire refusal object. Only [`Retry::AfterReaperSweep`] adds a retry
/// hint; every other refusal keeps its exact two-key shape.
fn refusal_value(reason: Fault) -> Value {
    let mut value = json!({"ok":false,"reason":reason.0});
    if reason.1 == Retry::AfterReaperSweep {
        value["retryable"] = Value::Bool(true);
        value["retry_after_ms"] = REAPER_RETRY_AFTER_MS.into();
    }
    value
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

const REFUSAL_CODES: [Refusal; 53] = {
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
        SnapshotPublishNotQuiesced,
        StaticBackupDeleteForbidden,
        SnapshotOfflineRequired,
        ComposeRecoveryRequired,
        BackupLimit,
    ]
};

impl From<Refusal> for Fault {
    fn from(reason: Refusal) -> Self {
        Self::new(refusal_wire(reason))
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
const WEB_RECIPE_ID: Id = crate::vm::WEB_RECIPE_ID;
type Recipe = (Id, Option<[u8; 32]>);

struct Gate {
    closed: bool,
    allow_agent_public_sites: bool,
    publish_profile: Option<Arc<Profile>>,
    publish_jobs: tokio::task::JoinSet<anyhow::Result<()>>,
    compose_jobs: tokio::task::JoinSet<anyhow::Result<()>>,
    test_workers: owned_worker::TestWorkers,
    compose_active: Arc<Mutex<std::collections::HashSet<Id>>>,
    #[cfg(test)]
    fixture: Option<tests::Fixture>,
    #[cfg(test)]
    drained: Option<tests::FixtureDrain>,
}

pub(crate) struct Supervisor {
    controller: Arc<Controller>,
    #[cfg(all(windows, target_arch = "x86_64"))]
    native_runs: owned_worker::Registry<Arc<native_run::Run>>,
    #[cfg(all(windows, target_arch = "x86_64"))]
    selected_test: Option<Arc<selected_test::SelectedTest>>,
    #[cfg(all(windows, target_arch = "x86_64"))]
    test_staging: owned_worker::Registry<()>,
    #[cfg(all(windows, target_arch = "x86_64"))]
    test_generations: crate::vm::qualification::GenerationGuards,
    #[cfg(all(windows, target_arch = "x86_64"))]
    native_scripts: std::sync::OnceLock<native_run::NativeScripts>,
    gate: Mutex<Gate>,
    /// Lock-free foreground admission fence, sampled inside the gate.
    fenced: AtomicBool,
    cancel: Shutdown,
    cursor_key: hmac::Key,
    host_id: Id,
    // TEST, A2 web and the separately selected Compose class share a page.
    recipes: [Option<Recipe>; 4],
    selected_static: Option<Result<selected_static::SelectedStatic, NativeRefusal>>,
    native_refusal: NativeRefusal,
    /// The runtime profile also serves the independent legacy candidate.
    runtime: Option<VmRuntimeProfile>,
    publisher: Publication,
    compose: compose::Host,
    compose_backup: compose_backup::BackupHost,
    vault: std::sync::OnceLock<Arc<crate::vault::Vault>>,
    vault_jobs: Mutex<vault_test::Jobs>,
    #[cfg(test)]
    vault_cleanup_waiting: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    #[cfg(test)]
    stall_vault_teardown: Mutex<Option<tests::StallHook>>,
    #[cfg(test)]
    stall_vault_acquisition: Mutex<Option<tests::StallHook>>,
    #[cfg(test)]
    vault_drain_waiting: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
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
    custody: Option<Arc<crate::config::VmCustody>>,
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
    /// Blocks the next early owner revocation after its transition recorded
    /// the stop obligations and before their runners are spawned.
    #[cfg(test)]
    stall_revoked: Mutex<Option<tests::StallHook>>,
    /// Acknowledges a fatal accept after the fence, then holds the listener
    /// until its stop is issued.
    #[cfg(test)]
    fatal_accept: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
}

impl Supervisor {
    fn has_selected_test(&self) -> bool {
        #[cfg(all(windows, target_arch = "x86_64"))]
        {
            self.selected_test.is_some()
        }
        #[cfg(not(all(windows, target_arch = "x86_64")))]
        {
            false
        }
    }

    pub(crate) fn open(
        root: &Path,
        mut profile: TestVms,
        custody: Arc<crate::config::VmCustody>,
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
        custody.verify()?;
        let static_enabled = profile.has_static_compose();
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
        #[cfg(all(windows, target_arch = "x86_64"))]
        if custody.has_compose_root() {
            *state
                .compose
                .secrets
                .get_mut()
                .map_err(|_| anyhow::anyhow!("Compose custody unavailable"))? = Some(Arc::new(
                crate::vm::compose_secrets::Dpapi::new(custody.clone()),
            ));
        }
        // The static enrollment transaction is bound to the selected recipe.
        if !static_enabled && !profile.compose_releases.is_empty() {
            let enrollments = std::mem::take(&mut profile.compose_releases)
                .into_iter()
                .map(compose::ReleaseInput::enroll)
                .collect::<anyhow::Result<Vec<_>>>()?;
            custody.verify()?;
            crate::store::vm::compose::enroll(state.controller.compose_store(), &enrollments)?;
        }
        #[cfg(all(windows, target_arch = "x86_64"))]
        if profile.enabled {
            state.install_test();
        }
        state.recipes[1] = Some((WEB_RECIPE_ID, None));
        state.recipes[2].get_or_insert((Controller::windows_recipe_id(), None));
        if static_enabled {
            #[cfg(all(windows, target_arch = "x86_64"))]
            let selected = state
                .selected_test
                .as_ref()
                .ok_or(state.native_refusal)
                .and_then(|m4| selected_static::SelectedStatic::open(&custody, m4));
            #[cfg(not(all(windows, target_arch = "x86_64")))]
            let selected = Err(NativeRefusal::UnsupportedPlatform);
            custody.verify()?;
            state.install_static_selection(selected)?;
            #[cfg(all(windows, target_arch = "x86_64"))]
            if let Some(Ok(selection)) = &state.selected_static {
                selection.enroll_runtime(
                    state.controller.compose_store(),
                    &mut state.compose_backup,
                    &mut profile,
                    &custody,
                )?;
            }
        }
        state.custody = Some(custody);
        let gate = state
            .gate
            .get_mut()
            .map_err(|_| anyhow::anyhow!("VM gate unavailable"))?;
        gate.allow_agent_public_sites = profile.allow_agent_public_sites;
        gate.publish_profile = profile.publication.map(Arc::from);
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
                compose_jobs: Default::default(),
                test_workers: Default::default(),
                compose_active: Default::default(),
                #[cfg(test)]
                fixture: None,
                #[cfg(test)]
                drained: None,
            }),
            #[cfg(all(windows, target_arch = "x86_64"))]
            native_runs: Default::default(),
            #[cfg(all(windows, target_arch = "x86_64"))]
            selected_test: None,
            #[cfg(all(windows, target_arch = "x86_64"))]
            test_staging: Default::default(),
            #[cfg(all(windows, target_arch = "x86_64"))]
            test_generations: Default::default(),
            #[cfg(all(windows, target_arch = "x86_64"))]
            native_scripts: Default::default(),
            fenced: AtomicBool::new(false),
            cancel: Shutdown::default(),
            cursor_key: hmac::Key::new(hmac::HMAC_SHA256, &secret),
            host_id,
            recipes: [
                recipe.map(|(id, digest)| (id, Some(digest))),
                None,
                None,
                None,
            ],
            selected_static: None,
            native_refusal,
            runtime,
            publisher: Publication::default(),
            compose: compose::Host::default(),
            compose_backup: compose_backup::BackupHost::default(),
            vault: Default::default(),
            vault_jobs: Default::default(),
            #[cfg(test)]
            vault_cleanup_waiting: Default::default(),
            #[cfg(test)]
            stall_vault_teardown: Default::default(),
            #[cfg(test)]
            stall_vault_acquisition: Default::default(),
            #[cfg(test)]
            vault_drain_waiting: Default::default(),
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
            custody: None,
            #[cfg(test)]
            stall_recovery: Mutex::new(None),
            #[cfg(test)]
            stall_dispatch: Mutex::new(None),
            #[cfg(test)]
            stall_admission: Mutex::new(None),
            #[cfg(test)]
            stall_sweep: Mutex::new(None),
            #[cfg(test)]
            stall_revoked: Mutex::new(None),
            #[cfg(test)]
            fatal_accept: Mutex::new(None),
        }))
    }

    /// The configured roots still denote the held directories.
    pub(crate) fn verify_roots(&self) -> anyhow::Result<()> {
        match &self.custody {
            Some(custody) => custody.verify(),
            None => anyhow::bail!("VM state root not pinned"),
        }
    }

    pub(crate) async fn wait_for_root_loss(&self) {
        match &self.custody {
            Some(custody) => custody.lost().await,
            None => std::future::pending().await,
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
        // The same transition seals publication producers: after it no
        // activation registers, no revocation records a stop obligation and
        // no first DNS transport write passes. It then signals cancellation.
        self.publisher.seal(&self.cancel);
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
        let fixture = self.lock()?.fixture.as_ref().map(tests::Fixture::reaper);
        #[cfg(test)]
        if let Some(reaper) = fixture {
            self.controller.sweep_with(&reaper, &*reaper.custody)?;
            return Ok(());
        }
        #[cfg(all(windows, target_arch = "x86_64"))]
        if let Some(cleanup) = self.test_cleanup() {
            self.controller.sweep_with(&cleanup, &cleanup.custody)?;
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
                    let _ = tokio::task::spawn_blocking(move || supervisor.sweep()).await?;
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

    /// Spawns runners for stop obligations a revocation already recorded.
    /// A runner only executes its obligation: a coalesced or stale runner
    /// finds it claimed or settled, and the final drain services any
    /// obligation whose runner never started.
    fn spawn_stops(self: &Arc<Self>, generations: Vec<u64>) {
        if generations.is_empty() {
            return;
        }
        let Ok(mut stops) = self.stops.lock() else {
            return;
        };
        stops.reap(&self.publisher);
        for generation in generations {
            let supervisor = self.clone();
            stops.set.spawn(async move {
                supervisor.publisher.run_stop(generation).await;
            });
            #[cfg(test)]
            self.stops_spawned.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// Records and drops completed stop runners.
    fn reap_stops(&self) {
        if let Ok(mut stops) = self.stops.lock() {
            stops.reap(&self.publisher);
        }
    }

    /// Cancel and join every admitted worker and its terminal audit write.
    /// The caller has passed the close barrier, so no new worker can be
    /// registered.
    ///
    /// `Err` means the drain is not quiescent: a worker or stop did not
    /// finish, or local ingress is not confirmed down. `Ok` carries whether
    /// the publication then settled cleanly; custody recovery needs only
    /// quiescence.
    pub(crate) async fn drain_workers(&self, bound: Duration) -> anyhow::Result<Drained> {
        let deadline = tokio::time::Instant::now() + bound;
        let (mut publish_jobs, mut compose_jobs, test_workers) = {
            let mut gate = self.lock()?;
            (
                std::mem::take(&mut gate.publish_jobs),
                std::mem::take(&mut gate.compose_jobs),
                std::mem::take(&mut gate.test_workers),
            )
        };
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
            // A worker that returned an error left its durable intent and
            // receipt for reconciliation, which the final turn below performs
            // and reports. A worker that did not finish is a lost completion.
            while let Some(job) = publish_jobs.join_next().await {
                self.publisher.record_join(&job);
            }
            // The fence sealed every stop producer, so the recorded obligations
            // are final: run those whose runner never started and wait for the
            // running ones. Remaining runners then find nothing to do.
            self.publisher.settle_stops().await;
            let mut stops = self
                .stops
                .lock()
                .map(|mut jobs| std::mem::take(&mut *jobs))
                .map_err(|_| anyhow::anyhow!("publication settlement unconfirmed"))?;
            while let Some(job) = stops.set.join_next().await {
                self.publisher.record_join(&job);
            }
            // A deferred stop may have confirmed what the first attempt could
            // not, so the final local state decides.
            let local = self.publisher.stop_local(None).await;
            anyhow::ensure!(
                local && !self.publisher.stop_unconfirmed(),
                "publication settlement unconfirmed"
            );
            while let Some(job) = compose_jobs.join_next().await {
                job??;
            }
            test_workers.join().await?;
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
        self.drain_vault_jobs(deadline).await?;
        // Quiescent: every worker and stop joined and local ingress is down.
        // The final reconciliation decides the publication settlement; a
        // remote timeout or failure leaves the intent intact for startup
        // reconciliation and is reported as unconfirmed.
        Ok(self
            .publisher
            .finish(&self.controller, &self.cancel, deadline)
            .await)
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

    fn install_static_selection(
        &mut self,
        selected: Result<selected_static::SelectedStatic, NativeRefusal>,
    ) -> anyhow::Result<()> {
        if let Ok(selection) = &selected {
            let recipe = selection.recipe();
            anyhow::ensure!(
                !self
                    .recipes
                    .iter()
                    .flatten()
                    .any(|(id, _)| *id == recipe.recipe_id),
                "static Compose recipe ID conflicts with an existing entry"
            );
            self.recipes[3] = Some((recipe.recipe_id, Some(recipe.recipe_sha256)));
        }
        self.selected_static = Some(selected);
        Ok(())
    }

    fn static_selection(&self) -> Option<&selected_static::SelectedStatic> {
        self.selected_static.as_ref()?.as_ref().ok()
    }

    fn static_refusal(&self) -> Option<NativeRefusal> {
        match self.selected_static.as_ref()? {
            Ok(selected) => selected.refusal(),
            Err(reason) => Some(*reason),
        }
    }

    fn capability(&self, gate: &Gate) -> Value {
        #[cfg(test)]
        if gate.fixture.is_some() {
            return json!({"available":true});
        }
        let _ = gate;
        #[cfg(all(windows, target_arch = "x86_64"))]
        if let Some(selected) = &self.selected_test {
            return selected.catalog().unwrap_or_else(
                |reason| json!({"available":false,"reason":native_reason(reason)}),
            );
        }
        json!({"available":false,"reason":native_reason(self.native_refusal)})
    }
}

impl Drop for Supervisor {
    /// Production store users reach the store only through this supervisor,
    /// so the pinned root outlives them all.
    fn drop(&mut self) {
        #[cfg(all(windows, target_arch = "x86_64"))]
        {
            self.native_scripts.take();
            self.selected_test.take();
        }
        debug_assert!(
            self.custody.is_none()
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

/// Runners of recorded stop obligations. The obligations themselves live in
/// the publication's transition authority.
#[derive(Default)]
struct StopJobs {
    set: tokio::task::JoinSet<()>,
}

impl StopJobs {
    fn reap(&mut self, publisher: &Publication) {
        while let Some(done) = self.set.try_join_next() {
            publisher.record_join(&done);
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
    SnapshotCreate,
    SnapshotList,
    SnapshotRestore,
    SnapshotDelete,
    ComposeDeploy,
    ComposeStart,
    ComposeStop,
    ComposeDown,
    ComposeStatus,
    ComposeLogs,
    ComposeBackup,
}

impl ToolName {
    const ALL: [Self; 29] = [
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
        Self::SnapshotCreate,
        Self::SnapshotList,
        Self::SnapshotRestore,
        Self::SnapshotDelete,
        Self::ComposeDeploy,
        Self::ComposeStart,
        Self::ComposeStop,
        Self::ComposeDown,
        Self::ComposeStatus,
        Self::ComposeLogs,
        Self::ComposeBackup,
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
            Self::SnapshotCreate => "vm_snapshot_create",
            Self::SnapshotList => "vm_snapshot_list",
            Self::SnapshotRestore => "vm_snapshot_restore",
            Self::SnapshotDelete => "vm_snapshot_delete",
            Self::ComposeDeploy => "compose_deploy",
            Self::ComposeStart => "compose_start",
            Self::ComposeStop => "compose_stop",
            Self::ComposeDown => "compose_down",
            Self::ComposeStatus => "compose_status",
            Self::ComposeLogs => "compose_logs",
            Self::ComposeBackup => "compose_backup",
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
            Self::SnapshotCreate => Action::Snapshot as i64,
            Self::SnapshotRestore => Action::Restore as i64,
            Self::SnapshotDelete => SNAPSHOT_DELETE_ACTION,
            Self::ComposeDeploy => compose::Kind::Deploy.action(),
            Self::ComposeStart => compose::Kind::Start.action(),
            Self::ComposeStop => compose::Kind::Stop.action(),
            Self::ComposeDown => compose::Kind::Down.action(),
            Self::ComposeBackup => COMPOSE_BACKUP_ACTION,
            _ => return None,
        })
    }

    fn schema(self) -> Tool {
        let id = json!({"type":"string","pattern":"^[0-9a-f]{32}$"});
        let ttl = json!({"type":"integer","minimum":60,"maximum":2592000});
        let project = json!({"type":"string","pattern":"^[a-z](?:[a-z0-9-]{0,22}[a-z0-9])?$"});
        let (description, properties, required): (_, _, &[&str]) = match self {
            Self::ComposeDeploy => (
                "Deploy an owner-enrolled Compose release",
                json!({"vm_id":id,"project_id":project,"release_id":id,"request_id":id}),
                &["vm_id", "project_id", "release_id", "request_id"],
            ),
            Self::ComposeStart | Self::ComposeStop | Self::ComposeDown => (
                match self {
                    Self::ComposeStart => "Start the committed Compose revision",
                    Self::ComposeStop => "Stop the committed Compose revision",
                    _ => "Remove the project's containers and network while retaining volumes",
                },
                json!({"vm_id":id,"project_id":project,"request_id":id}),
                &["vm_id", "project_id", "request_id"],
            ),
            Self::ComposeStatus => (
                "Read durable Compose project status",
                json!({"vm_id":id,"project_id":project}),
                &["vm_id", "project_id"],
            ),
            Self::ComposeLogs => (
                "Read typed Compose lifecycle diagnostics",
                json!({"vm_id":id,"project_id":project,
                    "limit":{"type":"integer","minimum":1,"maximum":arx_guest::compose::MAX_LOG_ENTRIES},
                    "cursor":{"type":"integer","minimum":0,"maximum":i64::MAX}}),
                &["vm_id", "project_id"],
            ),
            Self::ComposeBackup => (
                "Create a protected host SQLite backup for an enrolled Compose project",
                json!({"vm_id":id,"project_id":project,"request_id":id}),
                &["vm_id", "project_id", "request_id"],
            ),
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
            Self::SnapshotCreate => (
                "Capture a stopped VM disk and firmware pair",
                json!({"vm_id":id,"request_id":id,
                    "name":{"type":"string","minLength":1,"maxLength":48,
                    "pattern":"^[A-Za-z0-9]+(?:-[A-Za-z0-9]+)*$"}}),
                &["vm_id", "request_id"],
            ),
            Self::SnapshotList => (
                "List caller-owned VM restore points",
                json!({"vm_id":id,"limit":{"type":"integer","minimum":1,"maximum":4,"default":4},
                    "cursor":{"type":"string","maxLength":96,"pattern":"^[A-Za-z0-9_-]+$"}}),
                &["vm_id"],
            ),
            Self::SnapshotRestore | Self::SnapshotDelete => (
                if self == Self::SnapshotRestore {
                    "Restore a stopped VM from a sealed pair"
                } else {
                    "Delete a stopped TEST VM restore point"
                },
                json!({"vm_id":id,"snapshot_id":id,"request_id":id}),
                &["vm_id", "snapshot_id", "request_id"],
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
        let mut tools: Vec<_> = ToolName::ALL.map(ToolName::schema).into();
        if self.supervisor.vault.get().is_some() {
            crate::vault::mcp::extend_activity_schema(&mut tools[1]);
            tools.extend(crate::vault::mcp::tools());
        }
        Ok(ListToolsResult {
            tools,
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
            Err(reason) => (refusal_value(reason), true),
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
    Snapshot(SnapshotRequest),
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
            ToolName::SnapshotCreate => Work::Snapshot(SnapshotRequest::Create(
                args.optional::<String>("name")?
                    .map(|name| SnapshotName::parse(&name).map_err(|_| INVALID))
                    .transpose()?,
            )),
            ToolName::SnapshotRestore => {
                Work::Snapshot(SnapshotRequest::Restore(args.id("snapshot_id")?))
            }
            ToolName::SnapshotDelete => {
                Work::Snapshot(SnapshotRequest::Delete(args.id("snapshot_id")?))
            }
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
        // One session/cancellation sample at the dispatch linearization
        // point, before the exact stop route and the admission fences.
        if self.closed.load(Ordering::SeqCst) || cancelled() {
            return Err(SHUTDOWN);
        }
        // A current TEST run owner's stop is admitted ahead of the fresh
        // effect fences, so it still reaches a starting run after closure.
        #[cfg(all(windows, target_arch = "x86_64"))]
        if request.name == ToolName::Stop.name()
            && let Some(result) = self.stop_subject(request.arguments.as_ref())
        {
            let (reply, owner) = result?;
            drop(gate);
            owner.notify_retry();
            return Ok(reply);
        }
        if gate.closed || self.supervisor.fenced.load(Ordering::SeqCst) {
            return Err(SHUTDOWN);
        }
        let name = request.name.as_ref();
        if let Some(vault) = self.supervisor.vault.get()
            && crate::vault::mcp::handles(name, request.arguments.as_ref())
        {
            let refusal = (!self.supervisor.vault_test_available(&gate))
                .then_some(crate::vault::Refusal::VmAdmissionClosed);
            return crate::vault::mcp::invoke(
                vault,
                &self.principal,
                name,
                request.arguments.unwrap_or_default(),
                refusal,
            )
            .map_err(|reason| Fault::new(reason.as_str()));
        }
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
        if matches!(
            name,
            "compose_backup_delete"
                | "compose_volume_delete"
                | "compose_project_delete"
                | "compose_backup_restore"
        ) {
            return self.refuse(
                HOST_REFUSAL_ACTION,
                None,
                Refusal::StaticBackupDeleteForbidden.into(),
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
        if matches!(
            tool,
            ToolName::ComposeDeploy
                | ToolName::ComposeStart
                | ToolName::ComposeStop
                | ToolName::ComposeDown
                | ToolName::ComposeStatus
                | ToolName::ComposeLogs
                | ToolName::ComposeBackup
        ) {
            let vm = args
                .0
                .get("vm_id")
                .and_then(Value::as_str)
                .and_then(|s| parse_id(s).ok());
            let result = if tool == ToolName::ComposeLogs {
                drop(gate);
                self.compose_logs(args)
            } else if tool == ToolName::ComposeBackup {
                self.compose_backup_dispatch(&mut gate, args)
            } else {
                self.compose_dispatch(&mut gate, tool, args)
            };
            return match result {
                Ok(value) => Ok(value),
                Err(reason) => {
                    self.refuse(tool.action().unwrap_or(READ_REFUSAL_ACTION), vm, reason)
                }
            };
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
            ToolName::Get | ToolName::StaticGet | ToolName::PublishStatus | ToolName::SnapshotList
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
    fn refuse<T>(&self, action: i64, vm: Option<Id>, reason: Fault) -> Outcome<T> {
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
            Work::Snapshot(snapshot) => snapshot.binding(request.id),
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
                if let Err(reason) = controller.foreground_static_template(
                    request.id,
                    web_digest,
                    self.supervisor
                        .static_selection()
                        .map(|selected| selected.recipe().recipe_sha256),
                )? {
                    return self.refuse(action, None, reason.into());
                }
                None
            }
            Work::Build => None,
            Work::Create(_) => Some(ForegroundSubject::Template(request.id)),
            Work::Start => Some(ForegroundSubject::Start(request.id)),
            Work::Stop => Some(ForegroundSubject::Stop(request.id)),
            Work::Script(..) => Some(ForegroundSubject::Script(request.id)),
            Work::Extend(_) => Some(ForegroundSubject::Extend {
                vm: request.id,
                selected_test_installed: self.supervisor.has_selected_test(),
            }),
            Work::Publish | Work::Unpublish => return self.publication(gate, action, request),
            Work::Snapshot(snapshot) => {
                let result = controller.foreground_snapshot_precheck(
                    &self.principal,
                    request.id,
                    snapshot,
                )?;
                let is_static = match result {
                    Ok(is_static) => is_static,
                    Err(crate::store::vm::snapshot::Refused::Retiring)
                        if matches!(snapshot, crate::vm::snapshot::Request::Delete(_)) =>
                    {
                        return self.refuse(action, request.vm(), SNAPSHOT_DELETE_RETIRING);
                    }
                    Err(reason) => {
                        return self.refuse(action, request.vm(), reason.reason().into());
                    }
                };
                let route = if is_static {
                    let Some(proof) = self
                        .supervisor
                        .publisher
                        .snapshot_route(request.id)
                        .filter(|proof| proof.vm() == request.id)
                    else {
                        return self.refuse(
                            action,
                            request.vm(),
                            Refusal::SnapshotPublishNotQuiesced.into(),
                        );
                    };
                    Some(proof)
                } else {
                    None
                };
                #[cfg(test)]
                if let Some(fixture) = &mut gate.fixture {
                    return fixture.snapshot(self, action, request, route);
                }
                let _route = route;
                #[cfg(all(windows, target_arch = "x86_64"))]
                if !is_static && let Some(selected) = &self.supervisor.selected_test {
                    return selected.dispatch(self, gate, action, request);
                }
                return self.refuse(
                    action,
                    request.vm(),
                    Fault::new(native_reason(self.supervisor.native_refusal)),
                );
            }
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
        let static_effect = match request.work {
            Work::StaticCreate(_) => true,
            Work::Build => {
                request.id == WEB_RECIPE_ID
                    || self.supervisor.recipes[3].is_some_and(|(id, _)| id == request.id)
            }
            Work::Start | Work::Stop | Work::Script(..) | Work::Extend(_) => !controller
                .foreground_static_rows(&self.principal, Some(request.id), None, 1)?
                .is_empty(),
            _ => false,
        };
        if static_effect {
            return self.static_effect(gate, action, request, digest);
        }
        // A current TEST run owner serves its scripts on the owned channel.
        #[cfg(all(windows, target_arch = "x86_64"))]
        if let Work::Script(script, timeout) = &request.work
            && let Some(result) = self.script_subject(action, &request, script, *timeout)
        {
            return result;
        }
        #[cfg(test)]
        if let Some(fixture) = &mut gate.fixture {
            return fixture.admit(self, action, request, digest);
        }
        #[cfg(all(windows, target_arch = "x86_64"))]
        if let Some(selected) = &self.supervisor.selected_test {
            return selected.dispatch(self, gate, action, request);
        }
        let _ = gate;
        self.refuse(
            action,
            request.vm(),
            Fault::new(native_reason(self.supervisor.native_refusal)),
        )
    }

    fn static_effect(
        &self,
        gate: &mut Gate,
        action: i64,
        request: Mutation,
        digest: Option<[u8; 32]>,
    ) -> Outcome<Value> {
        if let Some(reason) = self.supervisor.static_refusal() {
            return self.refuse(action, request.vm(), Fault::new(native_reason(reason)));
        }
        #[cfg(test)]
        if let Some(fixture) = &mut gate.fixture {
            return fixture.admit(self, action, request, digest);
        }
        let _ = (gate, digest);
        self.refuse(
            action,
            request.vm(),
            Fault::new(native_reason(self.supervisor.native_refusal)),
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
        let obligations = supervisor.publisher.revoke_owned(&self.principal, vm);
        // The old lookup-then-enqueue gap: the obligations are recorded, and
        // only their runners are not yet spawned.
        #[cfg(test)]
        let _gap = supervisor
            .stall_revoked
            .lock()
            .ok()
            .and_then(|mut stall| stall.take())
            .map(tests::StallHook::enter);
        supervisor.spawn_stops(obligations);
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
        while let Some(joined) = gate.publish_jobs.try_join_next() {
            supervisor.publisher.record_join(&joined);
        }
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
            if let Some(reason) = supervisor.static_refusal() {
                return self.refuse(action, Some(vm), Fault::new(native_reason(reason)));
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
            let credentials = supervisor
                .publisher
                .prepare(profile, supervisor.compose.session(vm));
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
        let activation = match prepared
            .as_ref()
            .map(|(intent, _, _)| publisher.register(vm, &self.principal, &intent.nonce))
        {
            // Sealed for shutdown after the fence check above.
            Some(None) => return Err(SHUTDOWN),
            registered => registered.flatten(),
        };
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
                if let Some(ticket) = &activation {
                    publisher.abandon(ticket.generation);
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
        if let Some(ticket) = &activation {
            publisher.admitted(ticket.generation);
        } else {
            // The admitted unpublish committed `revoking`: revoke again for an
            // activation admitted between the early check and this point.
            let obligations = publisher.revoke(vm);
            supervisor.spawn_stops(obligations);
        }
        let supervisor = supervisor.clone();
        gate.publish_jobs.spawn(async move {
            let _tracked = tracked;
            match (prepared, activation) {
                (Some((intent, credentials, origin)), Some(ticket)) => {
                    let _slot = slot;
                    supervisor
                        .publisher
                        .publish(
                            &supervisor.controller,
                            operation,
                            intent,
                            credentials,
                            origin,
                            &ticket,
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
            ToolName::SnapshotList => self.snapshot_page(args),
            ToolName::Catalog | ToolName::List | ToolName::StaticList => {
                self.page(gate, tool, args)
            }
            ToolName::PublishStatus => {
                let id = args.id("vm_id")?;
                args.end()?;
                let status = controller.publication_status(&self.principal, id)??;
                let hosts: Vec<Value> = status
                    .hosts
                    .iter()
                    .map(|host| {
                        let mut item = json!({
                            "position": host.position,
                            "state": host.state.wire(),
                            "ambiguous": host.ambiguous,
                        });
                        if let Some(slot) = host.route_slot {
                            item["route_slot"] = json!(slot);
                        }
                        if let Some(project_id) = &host.project_id {
                            item["project_id"] = json!(project_id);
                        }
                        if let Some(service) = &host.service {
                            item["service"] = json!(service);
                        }
                        if let Some(reason) = host.reason {
                            item["reason"] = json!(reason.wire());
                        }
                        item
                    })
                    .collect();
                let mut value = json!({
                    "vm_id":hex::encode(id),
                    "phase":status.phase.map_or("unpublished", |phase| phase.wire()),
                    "host_count": hosts.len(),
                    "hosts": hosts,
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
                let row = vm_row(status, gate, true);
                #[cfg(all(windows, target_arch = "x86_64"))]
                let row = self.project_native_run(id, row);
                Ok(row)
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
        let mut capability = self.supervisor.capability(gate);
        let mut recipes = self.supervisor.recipes.map(|entry| entry.map(|(id, _)| id));
        let mut static_available = false;
        let mut static_blocked = false;
        if tool == ToolName::Catalog {
            if let Some(reason) = self.supervisor.static_refusal() {
                static_blocked = true;
                capability["static_compose"] =
                    json!({"available":false,"reason":native_reason(reason)});
                recipes[3] = None;
            } else if let Some(selected) = self.supervisor.static_selection() {
                static_available = true;
                capability["static_compose"] = json!({"available":true});
                capability["selected_static_qualification"] = selected.catalog();
            }
        }
        let controller = &self.supervisor.controller;
        let available = capability["available"] == true;
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
                    let buildable =
                        if self.supervisor.recipes[3].is_some_and(|(id, _)| id == entry.id) {
                            static_available
                        } else {
                            available
                                && (entry.id != WEB_RECIPE_ID || !static_blocked)
                                && self
                                    .supervisor
                                    .recipes
                                    .iter()
                                    .flatten()
                                    .any(|(id, digest)| *id == entry.id && digest.is_some())
                        };
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

    fn snapshot_page(&self, mut args: Args) -> Outcome<Value> {
        let vm = args.id("vm_id")?;
        let limit: u32 = args.optional("limit")?.unwrap_or(snapshot::POINTS_PER_VM);
        if !(1..=snapshot::POINTS_PER_VM).contains(&limit) {
            return Err(INVALID);
        }
        let cursor: Option<String> = args.optional("cursor")?;
        args.end()?;
        let tool = ToolName::SnapshotList;
        let after = cursor
            .as_deref()
            .map(|cursor| {
                self.decode_scoped_cursor(tool, Some(vm), cursor)
                    .map(|(_, id)| id)
            })
            .transpose()?;
        let mut points = self.supervisor.controller.foreground_snapshot_list(
            &self.principal,
            vm,
            after,
            limit + 1,
        )??;
        let mut page = json!({"vm_id":hex::encode(vm)});
        if points.len() > limit as usize {
            points.pop();
            let last = points.last().expect("positive limit");
            page["next_cursor"] = json!(self.scoped_cursor(tool, Some(vm), (0, last.id)));
        }
        page["entries"] = Value::Array(
            points
                .into_iter()
                .map(|point| {
                    let mut row = json!({
                        "snapshot_id":hex::encode(point.id),
                        "captured_at_ms":point.captured_ms,
                        "charged_bytes":point.charge,
                        "protected":point.protected,
                        "restore_eligible":point.restore_eligible,
                    });
                    if let Some(name) = point.name {
                        row["name"] = json!(name);
                    }
                    row
                })
                .collect(),
        );
        Ok(page)
    }

    fn cursor(&self, tool: ToolName, key: (u8, Id)) -> String {
        self.scoped_cursor(tool, None, key)
    }

    fn scoped_cursor(&self, tool: ToolName, vm: Option<Id>, (kind, id): (u8, Id)) -> String {
        let mut bytes = [0; 51];
        bytes[0] = 1;
        bytes[1] = kind;
        bytes[2..18].copy_from_slice(&id);
        let tag = self.cursor_mac(tool, vm, &bytes[..19]);
        bytes[19..].copy_from_slice(tag.as_ref());
        encode_cursor(bytes)
    }

    fn cursor_mac(&self, tool: ToolName, vm: Option<Id>, bytes: &[u8]) -> hmac::Tag {
        let mut mac = hmac::Context::with_key(&self.supervisor.cursor_key);
        mac.update(b"arxcovo.vm.cursor.v1\0");
        mac.update(tool.name().as_bytes());
        mac.update(&[0]);
        mac.update(self.principal.as_bytes());
        mac.update(&[0]);
        if let Some(vm) = vm {
            mac.update(&self.supervisor.host_id);
            mac.update(&vm);
        }
        mac.update(bytes);
        mac.sign()
    }

    fn decode_cursor(&self, tool: ToolName, value: &str) -> Outcome<(u8, Id)> {
        self.decode_scoped_cursor(tool, None, value)
    }

    fn decode_scoped_cursor(
        &self,
        tool: ToolName,
        vm: Option<Id>,
        value: &str,
    ) -> Outcome<(u8, Id)> {
        use subtle::ConstantTimeEq;
        let bytes = decode_cursor(value)?;
        let valid_kind = if tool == ToolName::Catalog {
            (1..=3).contains(&bytes[1])
        } else {
            bytes[1] == 0
        };
        let expected = self.cursor_mac(tool, vm, &bytes[..19]);
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
    if let Some(snapshot) = operation.snapshot {
        value["snapshot_id"] = json!(hex::encode(snapshot));
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
        SNAPSHOT_DELETE_ACTION => "snapshot_delete",
        20 => "compose_deploy",
        21 => "compose_start",
        22 => "compose_stop",
        23 => "compose_down",
        COMPOSE_BACKUP_ACTION => "compose_backup",
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

impl VmInventory {
    fn compose_target(args: &mut Args) -> Outcome<compose::Target> {
        let vm = args.id("vm_id")?;
        let project = args.get::<String>("project_id")?;
        if !arx_guest::compose::is_project_id(&project) {
            return Err(INVALID);
        }
        Ok(compose::Target { vm, project })
    }

    fn compose_dispatch(&self, gate: &mut Gate, tool: ToolName, mut args: Args) -> Outcome<Value> {
        use crate::store::vm::compose as db;
        let target = Self::compose_target(&mut args)?;
        let store = self.supervisor.controller.compose_store();
        let static_refusal = self.supervisor.static_refusal();
        let session = if static_refusal.is_none() {
            self.supervisor.compose.session(target.vm)
        } else {
            None
        };
        if tool == ToolName::ComposeStatus {
            args.end()?;
            let mut status = store
                .transact(|tx| db::status(tx, &self.principal, &target))?
                .ok_or(NOT_FOUND)?;
            let generation = store.transact(|tx| {
                if let Some(s) = &session
                    && db::live_boot(tx, target.vm, s.generation, s.wire_boot)?
                {
                    db::boot(tx, &self.principal, &target)
                } else {
                    Ok(None)
                }
            })?;
            let current = session.as_ref().is_some_and(|s| {
                s.is_open()
                    && status.pending.is_none()
                    && generation == Some(s.generation)
                    && status.generation == generation
                    && status.boot == Some(s.wire_boot)
            });
            if let Some(view) = &mut status.observation {
                view.observation = arx_guest::compose::Observation::Recorded;
            }
            return Ok(json!({
                "desired":status.desired, "revision":status.revision,
                "observed":status.observation, "current_boot":current,
                "operation_id":status.pending.map(hex::encode),
                "last_receipt":status.last_receipt,
                "recovery":status.recovery_id.map(|id|json!({"operation_id":hex::encode(id),
                    "reason":"compose_recovery_required","next_action":"retry_mutation_on_verified_boot_to_query_guest_receipt"})),
            }));
        }
        let kind = match tool {
            ToolName::ComposeDeploy => compose::Kind::Deploy,
            ToolName::ComposeStart => compose::Kind::Start,
            ToolName::ComposeStop => compose::Kind::Stop,
            ToolName::ComposeDown => compose::Kind::Down,
            _ => return Err(INVALID),
        };
        let request_id = RequestId::parse(&args.get::<String>("request_id")?)?;
        let key = request_id.key;
        let release = (kind == compose::Kind::Deploy)
            .then(|| args.id("release_id"))
            .transpose()?;
        args.end()?;
        let request = compose::Mutation {
            target,
            kind,
            key: request_id,
            release,
        };
        let admission = store.transact(|tx| {
            if let Some(known) = db::replay(tx, &self.principal, key)? {
                return Ok(if known.replay_matches(&request) {
                    Ok(known)
                } else {
                    Err(compose::Fault::KeyCollision)
                });
            }
            if crate::store::vm::count(
                tx,
                "SELECT count(*) FROM vm_operations WHERE principal=?1 AND request_key=?2",
                rusqlite::params![self.principal, key],
            )? != 0
            {
                return Ok(Err(compose::Fault::KeyCollision));
            }
            if db::status(tx, &self.principal, &request.target)?.is_none() {
                return Ok(Err(compose::Fault::NotFound));
            }
            let Some(session) = &session else {
                return Ok(Err(compose::Fault::NotReady));
            };
            if db::boot(tx, &self.principal, &request.target)? != Some(session.generation)
                || !db::live_boot(tx, request.target.vm, session.generation, session.wire_boot)?
            {
                return Ok(Err(compose::Fault::NotReady));
            }
            if !compose::deployment_preserves_routes(tx, &request)? {
                return Ok(Err(compose::Fault::Busy));
            }
            db::admit(
                tx,
                &self.principal,
                &request,
                session.generation,
                session.wire_boot,
                self.supervisor.controller.compose_now()?,
            )
        })?;
        let (intent, drive_key, repair) = match admission {
            Ok(intent) => (intent, key, false),
            Err(compose::Fault::RecoveryRequired(old_key)) => {
                let prior = store
                    .transact(|tx| db::replay(tx, &self.principal, old_key))?
                    .ok_or(STATE_UNAVAILABLE)?;
                (prior, old_key, true)
            }
            Err(compose::Fault::NotFound) => return Err(NOT_FOUND),
            Err(compose::Fault::NotReady) => {
                return Err(static_refusal.map_or(NOT_READY, |r| Fault::new(native_reason(r))));
            }
            Err(compose::Fault::Busy) => return Err(Refusal::Busy.into()),
            Err(compose::Fault::StaleKey) => return Err(Refusal::StaleKey.into()),
            Err(compose::Fault::ReplayFull) => return Err(Refusal::ReplayFull.into()),
            Err(compose::Fault::KeyCollision) => return Err(Refusal::KeyCollision.into()),
        };
        while let Some(joined) = gate.compose_jobs.try_join_next() {
            if !matches!(joined, Ok(Ok(()))) {
                return Err(STATE_UNAVAILABLE);
            }
        }
        if let Some(session) = session
            .filter(|s| s.generation == intent.generation && (repair || s.wire_boot == intent.boot))
            && (repair || intent.receipt.is_none())
            && (repair || intent.closed.is_none())
        {
            let active = gate.compose_active.clone();
            let id = intent.id;
            let started = active.lock().map_err(|_| STATE_UNAVAILABLE)?.insert(id);
            if started {
                let inventory = self.clone();
                gate.compose_jobs.spawn(async move {
                    let _running = ComposeRunning { active, id };
                    let supervisor = &inventory.supervisor;
                    // Durable pending state is the recovery authority. Drop
                    // transport and state errors without logging their contents.
                    let _ = supervisor
                        .compose
                        .drive(
                            &supervisor.controller,
                            &inventory.principal,
                            drive_key,
                            &request.target,
                            session,
                            &supervisor.cancel,
                        )
                        .await;
                    Ok(())
                });
            }
        }
        if repair {
            return Err(Refusal::ComposeRecoveryRequired.into());
        }
        Ok(
            json!({"operation_id":hex::encode(intent.id),"revision":intent.revision,
                "state":intent.receipt.as_ref().map_or_else(|| json!(intent.pending_state()), |r| json!(r.state)),
                "receipt":intent.receipt}),
        )
    }

    fn compose_logs(&self, mut args: Args) -> Outcome<Value> {
        use crate::store::vm::compose as db;
        let target = Self::compose_target(&mut args)?;
        let limit = args
            .optional::<u32>("limit")?
            .unwrap_or(arx_guest::compose::MAX_LOG_ENTRIES);
        let cursor = args.optional::<i64>("cursor")?.unwrap_or(0);
        args.end()?;
        if !(1..=arx_guest::compose::MAX_LOG_ENTRIES).contains(&limit) || cursor < 0 {
            return Err(INVALID);
        }
        let store = self.supervisor.controller.compose_store();
        let (generation, services) = store
            .transact(|tx| {
                if db::status(tx, &self.principal, &target)?.is_none() {
                    return Ok(None);
                }
                Ok(Some((
                    db::boot(tx, &self.principal, &target)?,
                    db::service_names(tx, &target)?,
                )))
            })?
            .ok_or(NOT_FOUND)?;
        let session = self
            .supervisor
            .compose
            .session(target.vm)
            .filter(|s| Some(s.generation) == generation)
            .ok_or(Fault::new("not_ready"))?;
        if !store
            .transact(|tx| db::live_boot(tx, target.vm, session.generation, session.wire_boot))?
        {
            return Err(Fault::new("not_ready"));
        }
        let (entries, next_cursor) = tokio::runtime::Handle::current()
            .block_on(self.supervisor.compose.logs(
                session,
                target,
                compose::LogRead {
                    cursor: cursor as u64,
                    limit,
                    services,
                },
                &self.supervisor.cancel,
            ))
            .ok_or(Fault::new("not_ready"))?;
        Ok(json!({"entries":entries,"next_cursor":next_cursor}))
    }

    fn compose_backup_dispatch(&self, gate: &mut Gate, mut args: Args) -> Outcome<Value> {
        use crate::store::vm::compose as db;
        use crate::vm::compose_backup::{BackupFault, BackupOpStatus};

        let target = Self::compose_target(&mut args)?;
        let request_id = RequestId::parse(&args.get::<String>("request_id")?)?;
        let key = request_id.key;
        args.end()?;

        let store = self.supervisor.controller.compose_store();
        let _ = self.supervisor.compose_backup.reconcile_partials(store);
        let static_refusal = self.supervisor.static_refusal();
        let session = if static_refusal.is_none() {
            self.supervisor.compose_backup.session(target.vm)
        } else {
            None
        };
        let custody_present = self.supervisor.compose_backup.custody().is_some();

        let admission = store.transact(|tx| {
            if let Some(known) = db::backup_replay(tx, &self.principal, key)? {
                return Ok(if known.target == target {
                    Ok(known)
                } else {
                    Err(BackupFault::KeyCollision)
                });
            }
            if db::status(tx, &self.principal, &target)?.is_none() {
                return Ok(Err(BackupFault::NotFound));
            }
            let Some(session) = &session else {
                return Ok(Err(BackupFault::NotReady));
            };
            if !custody_present {
                return Ok(Err(BackupFault::NotReady));
            }
            if db::boot(tx, &self.principal, &target)? != Some(session.generation)
                || !db::live_boot(tx, target.vm, session.generation, session.wire_boot)?
            {
                return Ok(Err(BackupFault::NotReady));
            }
            db::admit_backup(
                tx,
                &self.principal,
                &target,
                request_id,
                session.generation,
                session.wire_boot,
                self.supervisor.controller.compose_now()?,
                |tx, staging_bytes| {
                    self.supervisor.controller.validate_compose_backup_disk(
                        tx,
                        self.supervisor.compose_backup.capacity(),
                        staging_bytes,
                    )
                },
            )
        })?;

        let intent = match admission {
            Ok(intent) => intent,
            Err(BackupFault::NotFound) => return Err(NOT_FOUND),
            Err(BackupFault::NotReady) => {
                return Err(static_refusal.map_or(NOT_READY, |r| Fault::new(native_reason(r))));
            }
            Err(BackupFault::Busy) => return Err(Refusal::Busy.into()),
            Err(BackupFault::BackupLimit) => return Err(Refusal::BackupLimit.into()),
            Err(BackupFault::Disk) => return Err(Refusal::Disk.into()),
            Err(BackupFault::RecoveryRequired) => {
                return Err(Refusal::ComposeRecoveryRequired.into());
            }
            Err(BackupFault::StaleKey) => return Err(Refusal::StaleKey.into()),
            Err(BackupFault::ReplayFull) => return Err(Refusal::ReplayFull.into()),
            Err(BackupFault::KeyCollision) => return Err(Refusal::KeyCollision.into()),
        };

        while let Some(joined) = gate.compose_jobs.try_join_next() {
            if !matches!(joined, Ok(Ok(()))) {
                return Err(STATE_UNAVAILABLE);
            }
        }
        if intent.status == BackupOpStatus::Pending
            && let Some(session) =
                session.filter(|s| s.generation == intent.generation && s.wire_boot == intent.boot)
        {
            let active = gate.compose_active.clone();
            let id = intent.id;
            let started = active.lock().map_err(|_| STATE_UNAVAILABLE)?.insert(id);
            if started {
                let inventory = self.clone();
                let target_clone = target.clone();
                gate.compose_jobs.spawn(async move {
                    let _running = ComposeRunning { active, id };
                    let supervisor = &inventory.supervisor;
                    supervisor
                        .compose_backup
                        .drive(
                            &supervisor.controller,
                            &inventory.principal,
                            key,
                            &target_clone,
                            session,
                            &supervisor.cancel,
                        )
                        .await
                });
            }
        }

        // Only return opaque backup_id after durable host settlement commits.
        let mut response = json!({
            "operation_id": hex::encode(intent.id),
            "revision": intent.revision,
            "state": intent.status.wire(),
        });
        if intent.status == BackupOpStatus::Succeeded {
            response["backup_id"] = json!(hex::encode(intent.backup_id));
            if let Some(size) = intent.size_bytes {
                response["size_bytes"] = json!(size);
            }
            if let Some(sha) = intent.sha256 {
                response["sha256"] = json!(hex::encode(sha));
            }
        } else if let Some(failure) = &intent.failure {
            response["failure"] = json!(failure);
        }
        Ok(response)
    }
}

struct ComposeRunning {
    active: Arc<Mutex<std::collections::HashSet<Id>>>,
    id: Id,
}

impl Drop for ComposeRunning {
    fn drop(&mut self) {
        if let Ok(mut active) = self.active.lock() {
            active.remove(&self.id);
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
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
        sync::atomic::{AtomicI64, AtomicU64},
    };
    use tokio::{
        io::{
            AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, DuplexStream, ReadHalf,
            WriteHalf, split,
        },
        sync::Notify,
        task::{JoinHandle, JoinSet},
    };

    pub(super) const HOST: HostObservation = HostObservation {
        owner_idle: Some(true),
        logical_cpus: Some(4),
        available_ram: Some(u64::MAX),
        capacity: Some(Capacity {
            free_bytes: u64::MAX,
            managed_bytes: 0,
        }),
    };
    pub(super) const RESOURCES: Resources = Resources {
        vcpus: 1,
        guest_ram: 1024,
        overhead_ram: 1024,
    };
    pub(super) const RETAINED: DiskClaim = DiskClaim {
        staging: 0,
        images: 128,
        metadata: 0,
    };
    pub(super) const TRANSIENT: DiskClaim = DiskClaim {
        staging: 128,
        images: 128,
        metadata: 0,
    };
    pub(super) const UTC: i64 = 1_800_000_000_000;
    const VAULT_CANARY: &str = "arx-vault-fixture-canary-a58c";
    const VAULT_EXIT_0: &str = "echo arx-vault-fixture-canary-a58c; exit 0";
    const VAULT_EXIT_7: &str = "echo arx-vault-fixture-canary-a58c; exit 7";
    pub(super) const RECIPE: (Id, [u8; 32]) = ([3; 16], [4; 32]);
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
    /// Hang guard for the fixture loopback TLS proof. A valid exchange
    /// returns when both sides finish; this bound only stops a stuck socket.
    /// Real TLS and I/O errors still return immediately and fence the boot.
    const FIXTURE_TLS_DEADLINE: Duration = Duration::from_secs(10);
    /// Outer hang guard for an expected static fixture start and the standalone
    /// origin probe. Longer than [`FIXTURE_TLS_DEADLINE`] so a slow valid
    /// exchange can publish its receipt. A settled operation returns immediately.
    /// Other fixture operations keep the two-second guard in [`terminal`].
    const FIXTURE_START_GUARD: Duration = Duration::from_secs(15);

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
            let mut held = self.hold_identity(slot, proof, delete)?;
            ensure!(
                proof.pin.verify(&mut held.file, SNAPSHOT_FILE_LIMIT)?,
                "fixture artifact changed"
            );
            Ok(held)
        }

        fn hold_identity(
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
            let file = custody::open_leaf(&directory, &path, name, delete)?;
            ensure!(
                custody::identity(&file, false)? == proof.identity,
                "fixture artifact identity changed"
            );
            Ok(custody::Held {
                directory,
                file,
                name: name.to_owned(),
            })
        }

        /// Observe the stopped fixture writer's current bytes through the
        /// recorded identities. The handles stay held through stop settlement.
        fn released_pair(&self, (slots, [disk, vars]): Pair) -> Result<([custody::Held; 2], Pair)> {
            let mut held = [
                self.hold_identity(slots.disk, disk, false)?,
                self.hold_identity(slots.vars, vars, false)?,
            ];
            let observe = |file: &mut fs::File, mut evidence: ArtifactEvidence| {
                use std::io::Read;
                let mut hash = Sha256::new();
                let size = std::io::copy(&mut file.take(SNAPSHOT_FILE_LIMIT + 1), &mut hash)?;
                ensure!(
                    (1..=SNAPSHOT_FILE_LIMIT).contains(&size),
                    "fixture pair exceeds bound"
                );
                evidence.pin = crate::vm::paths::ArtifactPin {
                    size,
                    sha256: hash.finalize().into(),
                };
                Ok::<_, anyhow::Error>(evidence)
            };
            let proof = [
                observe(&mut held[0].file, disk)?,
                observe(&mut held[1].file, vars)?,
            ];
            ensure!(
                proof[1].pin.size == vars.pin.size,
                "fixture firmware variables size changed"
            );
            Ok((held, (slots, proof)))
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

    const SNAPSHOT_VM_BYTES: u64 = 1 << 20;
    const SNAPSHOT_FILE_LIMIT: u64 = 131_072;

    fn fixture_snapshot_path(slot: ArtifactSlot) -> crate::vm::paths::LocalPath {
        crate::vm::paths::LocalPath::parse("D:/fixture-snapshot")
            .unwrap()
            .join(&hex::encode(slot.generation))
            .unwrap()
            .join(&format!("{}-{}", slot.role, hex::encode(slot.key)))
            .unwrap()
    }

    fn fixture_snapshot_overlay(golden: ArtifactSlot) -> Vec<u8> {
        let mut disk = fixture_qcow2();
        let backing = fixture_snapshot_path(golden);
        let backing = backing.as_str().as_bytes();
        disk[8..16].copy_from_slice(&144u64.to_be_bytes());
        disk[16..20].copy_from_slice(&(backing.len() as u32).to_be_bytes());
        disk[112..116].copy_from_slice(&0xe279_2acau32.to_be_bytes());
        disk[116..120].copy_from_slice(&5u32.to_be_bytes());
        disk[120..125].copy_from_slice(b"qcow2");
        disk[144..144 + backing.len()].copy_from_slice(backing);
        disk
    }

    fn qcow2_virtual_size(file: &mut fs::File) -> Result<u64> {
        use std::io::{Read, Seek, SeekFrom};
        file.seek(SeekFrom::Start(24))?;
        let mut bytes = [0u8; 8];
        file.read_exact(&mut bytes)?;
        file.rewind()?;
        Ok(u64::from_be_bytes(bytes))
    }

    struct FixtureCopy<'a> {
        controller: &'a Controller,
        work: &'a crate::store::vm::snapshot::Work,
        host: Arc<dyn Fn() -> HostObservation + Send + Sync>,
        staging_disk_path: crate::vm::paths::LocalPath,
        active: [custody::Held; 2],
        source: Option<[custody::Held; 2]>,
        golden: Option<custody::Held>,
        staging: [fs::File; 2],
    }

    impl FixtureCopy<'_> {
        fn files(&mut self, side: snapshot::Side) -> [&mut fs::File; 2] {
            match side {
                snapshot::Side::Staging => self.staging.each_mut(),
                snapshot::Side::Active => self.active.each_mut().map(|held| &mut held.file),
                snapshot::Side::Source => self
                    .source
                    .as_mut()
                    .unwrap_or(&mut self.active)
                    .each_mut()
                    .map(|held| &mut held.file),
            }
        }
    }

    impl snapshot::CopyCustody for FixtureCopy<'_> {
        fn checkpoint(
            &mut self,
            _: &snapshot::CopyPlan,
            worker: &PendingOperation,
            _: &snapshot::CopyBudget<'_>,
        ) -> Result<(), snapshot::Refusal> {
            let observation = (self.host)();
            self.controller
                .snapshot_checkpoint(worker, self.work, observation, RESOURCES)
                .map_err(|_| snapshot::Refusal::Custody)
        }

        fn observe(
            &mut self,
            side: snapshot::Side,
            _: &snapshot::CopyBudget<'_>,
        ) -> Result<[snapshot::Observation; 2], snapshot::Refusal> {
            let [disk, vars] = self.files(side);
            let inspect = |file: &fs::File| -> Result<snapshot::Observation, snapshot::Refusal> {
                Ok(snapshot::Observation {
                    identity: custody::identity(file, false)
                        .map_err(|_| snapshot::Refusal::Identity)?,
                    size: file.metadata().map_err(|_| snapshot::Refusal::Io)?.len(),
                })
            };
            Ok([inspect(disk)?, inspect(vars)?])
        }

        fn read(
            &mut self,
            side: snapshot::Side,
            role: snapshot::FileRole,
            offset: u64,
            bytes: &mut [u8],
            _: &snapshot::CopyBudget<'_>,
        ) -> Result<(), snapshot::Refusal> {
            use std::io::{Read, Seek, SeekFrom};
            let index = usize::from(role == snapshot::FileRole::Vars);
            let file = &mut self.files(side)[index];
            file.seek(SeekFrom::Start(offset))
                .and_then(|_| file.read_exact(bytes))
                .map_err(|_| snapshot::Refusal::Io)
        }

        fn write_staging(
            &mut self,
            role: snapshot::FileRole,
            offset: u64,
            bytes: &[u8],
            _: &snapshot::CopyBudget<'_>,
        ) -> Result<(), snapshot::Refusal> {
            use std::io::{Seek, SeekFrom, Write};
            let file = &mut self.staging[usize::from(role == snapshot::FileRole::Vars)];
            file.seek(SeekFrom::Start(offset))
                .and_then(|_| file.write_all(bytes))
                .map_err(|_| snapshot::Refusal::Io)
        }

        fn golden_header(
            &mut self,
            golden: &snapshot::Golden,
            header: &mut [u8],
            _: &snapshot::CopyBudget<'_>,
        ) -> Result<(), snapshot::Refusal> {
            use std::io::{Read, Seek};
            let file = &mut self.golden.as_mut().ok_or(snapshot::Refusal::Custody)?.file;
            file.rewind().map_err(|_| snapshot::Refusal::Io)?;
            if custody::identity(file, false).map_err(|_| snapshot::Refusal::Identity)?
                != golden.identity
                || !golden
                    .pin
                    .verify(&mut *file, SNAPSHOT_FILE_LIMIT)
                    .map_err(|_| snapshot::Refusal::Pin)?
            {
                return Err(snapshot::Refusal::Pin);
            }
            file.rewind()
                .and_then(|_| file.read_exact(header))
                .map_err(|_| snapshot::Refusal::Io)
        }

        fn inspect(
            &mut self,
            info: &crate::vm::qemu_args::SnapshotInfo<'_>,
            output: &mut [u8],
            _: &snapshot::CopyBudget<'_>,
        ) -> Result<usize, snapshot::Refusal> {
            let path = info.path().as_str();
            if info.args() != ["info", "--output=json", "-f", "qcow2", path] {
                return Err(snapshot::Refusal::Inspection);
            }
            let active_path = fixture_snapshot_path(self.work.active.pair.0.disk);
            let source_path = fixture_snapshot_path(self.work.source.0.disk);
            let file = if self.staging_disk_path.as_str() == path {
                &mut self.staging[0]
            } else if source_path.as_str() == path && self.source.is_some() {
                &mut self.source.as_mut().unwrap()[0].file
            } else if active_path.as_str() == path {
                &mut self.active[0].file
            } else {
                return Err(snapshot::Refusal::Inspection);
            };
            let actual_size = file.metadata().map_err(|_| snapshot::Refusal::Io)?.len();
            let virtual_size =
                qcow2_virtual_size(file).map_err(|_| snapshot::Refusal::Inspection)?;
            let mut body = json!({
                "filename":path,
                "format":"qcow2",
                "virtual-size":virtual_size,
                "actual-size":actual_size,
                "dirty-flag":false,
            });
            if let Some(golden) = info.golden() {
                let golden = golden
                    .path_for(crate::vm::paths::ArtifactRole::Golden)
                    .map_err(|_| snapshot::Refusal::Inspection)?
                    .as_str();
                body["backing-filename"] = json!(golden);
                body["full-backing-filename"] = json!(golden);
                body["backing-filename-format"] = json!("qcow2");
            }
            let encoded = serde_json::to_vec(&body).map_err(|_| snapshot::Refusal::Inspection)?;
            output
                .get_mut(..encoded.len())
                .ok_or(snapshot::Refusal::Inspection)?
                .copy_from_slice(&encoded);
            Ok(encoded.len())
        }

        fn flush_staging(&mut self, _: &snapshot::CopyBudget<'_>) -> Result<(), snapshot::Refusal> {
            for leaf in &self.staging {
                leaf.sync_all().map_err(|_| snapshot::Refusal::Io)?;
            }
            #[cfg(unix)]
            self.active[0]
                .directory
                .sync_all()
                .map_err(|_| snapshot::Refusal::Io)?;
            Ok(())
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn copy_fixture_snapshot(
        artifacts: &Artifacts,
        controller: &Controller,
        work: &crate::store::vm::snapshot::Work,
        worker: &PendingOperation,
        route: Option<&runtime::publication::SnapshotRoute>,
        host: Arc<dyn Fn() -> HostObservation + Send + Sync>,
        hold: Option<(Arc<Notify>, Arc<Notify>)>,
        fault: Option<SnapshotFault>,
        cancel: &Shutdown,
        vm_cancel: &Shutdown,
    ) -> std::result::Result<Pair, [crate::store::vm::snapshot::StagedLeaf; 2]> {
        use crate::store::vm::snapshot::StagedLeaf;
        let mut leaves = [StagedLeaf::Untouched; 2];
        let result = copy_fixture_snapshot_inner(
            artifacts,
            controller,
            work,
            worker,
            route,
            host,
            hold,
            fault,
            cancel,
            vm_cancel,
            &mut leaves,
        )
        .await;
        result.map_err(|_| leaves)
    }

    #[allow(clippy::too_many_arguments)]
    async fn copy_fixture_snapshot_inner(
        artifacts: &Artifacts,
        controller: &Controller,
        work: &crate::store::vm::snapshot::Work,
        worker: &PendingOperation,
        route: Option<&runtime::publication::SnapshotRoute>,
        host: Arc<dyn Fn() -> HostObservation + Send + Sync>,
        hold: Option<(Arc<Notify>, Arc<Notify>)>,
        fault: Option<SnapshotFault>,
        cancel: &Shutdown,
        vm_cancel: &Shutdown,
        leaves: &mut [crate::store::vm::snapshot::StagedLeaf; 2],
    ) -> Result<Pair> {
        use crate::store::vm::snapshot::StagedLeaf;
        use snapshot::{
            ActiveRelease, Compatibility, CopyPlan, Destination, Golden, Leaf, Limits, Lineage,
            StoredPair,
        };
        let slots = work.candidate.expect("copy candidate");
        let held_pair = |(slots, [disk, vars]): Pair| {
            Ok::<_, anyhow::Error>([
                artifacts.hold(slots.disk, disk, false)?,
                artifacts.hold(slots.vars, vars, false)?,
            ])
        };
        let mut active = held_pair(work.active.pair)?;
        let mut source = (work.action == 6)
            .then(|| held_pair(work.source))
            .transpose()?;
        let (lineage, golden) = if let Some((golden_slots, [golden, _])) = work.active.golden {
            let slot = golden_slots.disk;
            (
                Lineage::Test {
                    template: work.active.template,
                    golden: Golden {
                        object: slot.key,
                        path: fixture_snapshot_path(slot),
                        identity: golden.identity,
                        pin: golden.pin,
                    },
                },
                Some(artifacts.hold(slot, golden, false)?),
            )
        } else {
            (
                Lineage::Static {
                    origin: work.active.origin,
                },
                None,
            )
        };
        let firmware = crate::vm::paths::ArtifactPin {
            size: b"fixture firmware".len() as u64,
            sha256: Sha256::digest(b"fixture firmware").into(),
        };
        let active_virtual = qcow2_virtual_size(&mut active[0].file)?;
        let source_virtual = match source.as_mut() {
            Some(held) => qcow2_virtual_size(&mut held[0].file)?,
            None => active_virtual,
        };
        let map_leaf = |slot: ArtifactSlot, proof: ArtifactEvidence| Leaf {
            key: slot.key,
            path: fixture_snapshot_path(slot),
            identity: proof.identity,
            pin: proof.pin,
        };
        let stored = |(slots, [disk, vars]): Pair, virtual_disk_bytes: u64| StoredPair {
            compatibility: Compatibility {
                host: work.active.host,
                vm: work.vm,
                lineage: lineage.clone(),
                recipe: work.active.recipe,
                firmware,
                virtual_disk_bytes,
                vars_bytes: vars.pin.size,
            },
            generation: slots.disk.generation,
            disk: map_leaf(slots.disk, disk),
            vars: map_leaf(slots.vars, vars),
        };
        let action = if work.action == 5 {
            snapshot::Action::Capture {
                snapshot: work.target,
            }
        } else {
            snapshot::Action::Restore {
                snapshot: work.target,
            }
        };
        let [vm_points, host_points] = work.points;
        let plan = CopyPlan::new(
            snapshot::Intent {
                activity_id: work.activity,
                action,
                active: stored(work.active.pair, active_virtual),
                active_writer_epoch: work.active.epoch,
                release: ActiveRelease {
                    generation: work.active.pair.0.disk.generation,
                    writer_epoch: work.active.epoch,
                    disposition: if work.active.clean {
                        runtime::StopDisposition::CleanRelease
                    } else {
                        runtime::StopDisposition::DirtyRelease
                    },
                    endpoints_released: true,
                },
                selected: (work.action == 6)
                    .then(|| (work.target, stored(work.source, source_virtual))),
                destination: Destination {
                    generation: slots.disk.generation,
                    disk_key: slots.disk.key,
                    vars_key: slots.vars.key,
                },
            },
            &crate::vm::paths::LocalPath::parse("D:/fixture-snapshot").unwrap(),
            work.active.pair.1[0].identity.volume,
            Limits {
                vm_points,
                host_points,
                max_file_bytes: SNAPSHOT_FILE_LIMIT,
                max_virtual_bytes: SNAPSHOT_VM_BYTES,
                restore_growth_bytes: if work.action == 6 { source_virtual } else { 0 },
                staging_bytes: work.claim.staging,
                metadata_bytes: work.claim.metadata,
            },
        )
        .map_err(|reason| anyhow::anyhow!("fixture snapshot plan: {reason:?}"))?;
        ensure!(
            plan.additional_claim().total() == work.claim.total(),
            "fixture snapshot charge changed"
        );
        if matches!(fault, Some(SnapshotFault::FailBeforeCreate)) {
            anyhow::bail!("injected snapshot fail before create");
        }
        let mut hold = hold;
        if matches!(fault, Some(SnapshotFault::HoldBeforeCreate))
            && let Some((entered, release)) = hold.take()
        {
            entered.notify_one();
            tokio::select! {
                biased;
                _ = cancel.cancelled() => anyhow::bail!("fixture snapshot cancelled"),
                _ = vm_cancel.cancelled() => anyhow::bail!("fixture snapshot lease cancelled"),
                _ = release.notified() => {}
            }
        }
        let mut create = |index: usize, slot: ArtifactSlot, directory| {
            let path = artifacts.0.path(slot.role, slot.key);
            controller.snapshot_leaf_begin(worker, work, slot)?;
            leaves[index] = StagedLeaf::Unknown;
            let file = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .open(&path)?;
            if (index == 0 && matches!(fault, Some(SnapshotFault::FailAfterDiskAttemptNoId)))
                || (index == 1 && matches!(fault, Some(SnapshotFault::FailAfterVarsAttemptNoId)))
            {
                anyhow::bail!("injected snapshot attempt without recorded identity");
            }
            let identity = custody::identity(&file, false)?;
            leaves[index] = StagedLeaf::Identified(identity, directory);
            controller.snapshot_leaf_identity(worker, work, slot, identity, directory)?;
            protect_fixture(&path, false)?;
            Ok::<_, anyhow::Error>((file, identity))
        };
        controller.snapshot_checkpoint(worker, work, host(), RESOURCES)?;
        let directory = work.active.pair.1[0].directory;
        let (disk, disk_identity) = create(0, slots.disk, directory)?;
        if matches!(fault, Some(SnapshotFault::FailAfterDiskIdentified)) {
            anyhow::bail!("injected snapshot fail after disk identified");
        }
        if matches!(fault, Some(SnapshotFault::OrphanPanicAfterDiskOnly)) {
            panic!("injected snapshot orphan panic after disk identified");
        }
        let (vars, vars_identity) = create(1, slots.vars, directory)?;
        if matches!(fault, Some(SnapshotFault::OrphanPanicAfterIdentified)) {
            panic!("injected snapshot orphan panic after both leaves identified");
        }
        if let Some((entered, release)) = hold {
            entered.notify_one();
            tokio::select! {
                biased;
                _ = cancel.cancelled() => anyhow::bail!("fixture snapshot cancelled"),
                _ = vm_cancel.cancelled() => anyhow::bail!("fixture snapshot lease cancelled"),
                _ = release.notified() => {}
            }
        }
        let mut custody = FixtureCopy {
            controller,
            work,
            host,
            staging_disk_path: plan.destination_paths()[0].clone(),
            active,
            source,
            golden,
            staging: [disk, vars],
        };
        let cancelled = || cancel.is_cancelled() || vm_cancel.is_cancelled();
        let mut copied = snapshot::copy_pair(
            &plan,
            worker,
            &mut custody,
            [disk_identity, vars_identity],
            &cancelled,
        )
        .map_err(|reason| anyhow::anyhow!("fixture snapshot copy: {reason:?}"))?;
        let [disk, vars] =
            [&copied.pair().disk, &copied.pair().vars].map(|leaf| ArtifactEvidence {
                identity: leaf.identity,
                pin: leaf.pin,
                directory,
            });
        controller.snapshot_leaf_complete(worker, work, slots.disk, disk)?;
        controller.snapshot_leaf_complete(worker, work, slots.vars, vars)?;
        copied
            .recheck()
            .map_err(|reason| anyhow::anyhow!("fixture snapshot recheck: {reason:?}"))?;
        if matches!(fault, Some(SnapshotFault::FailAfterFlushBeforeCommit)) {
            anyhow::bail!("injected snapshot fail after flush before commit");
        }
        let final_host = (custody.host)();
        controller.finish_snapshot_copy(
            worker,
            work,
            [disk, vars],
            route,
            final_host,
            RESOURCES,
        )?;
        Ok((slots, [disk, vars]))
    }

    /// The fixture's teardown authority for the 4c2b reaper. Release is
    /// offline only when no fixture boot or worker holds the subject.
    #[derive(Clone)]
    pub(super) struct FixtureReaper {
        after_selection: Arc<Mutex<Option<StallHook>>>,
        live: Arc<Mutex<Live>>,
        live_changed: Arc<std::sync::Condvar>,
        handle: tokio::runtime::Handle,
        pub(super) custody: Arc<FixtureCustody>,
    }

    impl runtime::ReaperRuntime for FixtureReaper {
        fn quiesce(&self, work: &CleanupWork, deadline: Instant) -> runtime::Quiesced {
            let stall = self.after_selection.lock().unwrap().take();
            let _completed = stall.map(StallHook::enter);
            let mut live = self.live.lock().unwrap();
            let Some(vm) = work.kind.vm() else {
                return runtime::Quiesced::Offline;
            };
            if live.release_pending.contains(&vm) {
                return runtime::Quiesced::Unconfirmed;
            }
            if live.working.contains(&vm) {
                if matches!(work.kind, crate::vm::reaper::CleanupKind::Expiry { .. })
                    && let Some(cancel) = live.snapshot_cancels.get(&vm)
                {
                    cancel.cancel();
                    let wait_until = deadline.min(Instant::now() + Duration::from_millis(500));
                    while live.working.contains(&vm) {
                        let Some(remaining) = wait_until.checked_duration_since(Instant::now())
                        else {
                            break;
                        };
                        let (guard, timeout) =
                            self.live_changed.wait_timeout(live, remaining).unwrap();
                        live = guard;
                        if timeout.timed_out() {
                            break;
                        }
                    }
                }
                if live.working.contains(&vm) {
                    return runtime::Quiesced::Unconfirmed;
                }
            }
            if live.boots.contains_key(&vm) {
                if matches!(work.kind, crate::vm::reaper::CleanupKind::Expiry { .. }) {
                    runtime::Quiesced::OwnedTestJob
                } else {
                    runtime::Quiesced::Unconfirmed
                }
            } else {
                runtime::Quiesced::Offline
            }
        }

        fn graceful_stop(&self, work: &CleanupWork, deadline: Instant) -> runtime::StopDisposition {
            let Some(vm) = work.kind.vm() else {
                return runtime::StopDisposition::ChargedQuarantine;
            };
            let boot = {
                let mut live = self.live.lock().unwrap();
                if live.force_reap_only.contains(&vm)
                    || live.stuck.contains(&vm)
                    || live.release_pending.contains(&vm)
                {
                    return runtime::StopDisposition::ChargedQuarantine;
                }
                match live.boots.remove(&vm) {
                    Some(Slot::Idle(boot)) => {
                        live.release_pending.insert(vm);
                        Some(*boot)
                    }
                    Some(slot) => {
                        live.boots.insert(vm, slot);
                        return runtime::StopDisposition::ChargedQuarantine;
                    }
                    None => None,
                }
            };
            let Some(boot) = boot else {
                return runtime::StopDisposition::CleanRelease;
            };
            let timeout = tokio::time::Instant::from_std(deadline);
            match self
                .handle
                .block_on(tokio::time::timeout_at(timeout, boot.stop()))
            {
                Ok(Ok(())) => {
                    self.live.lock().unwrap().release_pending.remove(&vm);
                    runtime::StopDisposition::CleanRelease
                }
                _ => runtime::StopDisposition::ChargedQuarantine,
            }
        }

        fn force_reap(&self, work: &CleanupWork, deadline: Instant) -> runtime::StopDisposition {
            let crate::vm::reaper::CleanupKind::Expiry { vm, .. } = work.kind else {
                return runtime::StopDisposition::ChargedQuarantine;
            };
            let boot = {
                let mut live = self.live.lock().unwrap();
                if live.stuck.contains(&vm) || live.release_pending.contains(&vm) {
                    return runtime::StopDisposition::ChargedQuarantine;
                }
                match live.boots.remove(&vm) {
                    Some(Slot::Idle(boot)) => {
                        live.release_pending.insert(vm);
                        Some(*boot)
                    }
                    Some(slot) => {
                        live.boots.insert(vm, slot);
                        return runtime::StopDisposition::ChargedQuarantine;
                    }
                    None => None,
                }
            };
            let Some(boot) = boot else {
                return runtime::StopDisposition::DirtyRelease;
            };
            let timeout = tokio::time::Instant::from_std(deadline);
            match self
                .handle
                .block_on(tokio::time::timeout_at(timeout, boot.force_release()))
            {
                Ok(Ok(())) => {
                    self.live.lock().unwrap().release_pending.remove(&vm);
                    runtime::StopDisposition::DirtyRelease
                }
                _ => runtime::StopDisposition::ChargedQuarantine,
            }
        }

        fn static_released(&self, vm: Id, _: Id, _: Instant) -> bool {
            let live = self.live.lock().unwrap();
            !live.release_pending.contains(&vm)
                && !live.boots.contains_key(&vm)
                && !live.working.contains(&vm)
                && !live.stuck.contains(&vm)
        }
    }

    type TickCompletion = Option<tokio::sync::oneshot::Sender<()>>;

    /// Injected reaper turns for the supervised runner.
    pub(super) struct TestTicks {
        requests: tokio::sync::mpsc::UnboundedReceiver<TickCompletion>,
        completed: TickCompletion,
    }

    impl TestTicks {
        fn channel() -> (tokio::sync::mpsc::UnboundedSender<TickCompletion>, Self) {
            let (send, requests) = tokio::sync::mpsc::unbounded_channel();
            (
                send,
                Self {
                    requests,
                    completed: None,
                },
            )
        }
    }

    impl ReaperTick for TestTicks {
        async fn next_tick(&mut self) -> bool {
            // The runner requests the next tick after publication and the
            // blocking custody sweep have both returned.
            if let Some(completed) = self.completed.take() {
                let _ = completed.send(());
            }
            let Some(completed) = self.requests.recv().await else {
                return false;
            };
            self.completed = completed;
            true
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
        /// Per-VM scoped cancellation tokens for live snapshot copy workers.
        snapshot_cancels: HashMap<Id, Shutdown>,
        /// Static VMs whose boot release this fixture refuses to prove.
        stuck: HashSet<Id>,
        /// VMs whose next stop simulates a failed graceful powerdown followed
        /// by safe force-release (typed dirty disposition).
        dirty_stop: HashSet<Id>,
        /// A removed Boot remains charged until its peer and guest joins succeed.
        /// A timeout or failed join leaves the fixture unable to prove release.
        release_pending: HashSet<Id>,
        /// TEST VMs whose expiry graceful stop fails so force-reap must run.
        force_reap_only: HashSet<Id>,
        /// Last classified fixture boot failure for this VM. Cleared when a
        /// later start settles successfully.
        boot_failures: HashMap<Id, FixtureBootStage>,
    }

    /// Injected snapshot copy faults, consumed by the next snapshot copy.
    #[derive(Clone, Copy)]
    pub(super) enum SnapshotFault {
        FailBeforeCreate,
        FailAfterDiskIdentified,
        FailAfterDiskAttemptNoId,
        FailAfterVarsAttemptNoId,
        FailAfterFlushBeforeCommit,
        OrphanPanicAfterIdentified,
        OrphanPanicAfterDiskOnly,
        HoldBeforeCreate,
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
        static_compose: bool,
        jobs: JoinSet<Result<()>>,
        snapshot_jobs: JoinSet<Result<()>>,
        live: Arc<Mutex<Live>>,
        live_changed: Arc<std::sync::Condvar>,
        handle: tokio::runtime::Handle,
        origins: Arc<Mutex<HashMap<Id, Arc<FixtureOrigin>>>>,
        artifacts: Artifacts,
        build_hold: Option<Arc<Notify>>,
        script_hold: Option<Arc<Notify>>,
        static_hold: Option<Arc<Notify>>,
        static_fault: Option<StaticFault>,
        windows_recipe: bool,
        pub(super) snapshot_test_media: bool,
        host: HostObservation,
        pub(super) host_observer: Option<Arc<dyn Fn() -> HostObservation + Send + Sync>>,
        pub(super) snapshot_hold: Option<(Arc<Notify>, Arc<Notify>)>,
        pub(super) snapshot_fault: Option<SnapshotFault>,
        pub(super) skip_restore_map_update: bool,
        /// A cancelled build worker reports failure, to witness drain errors.
        fail_cancelled: bool,
        failed: bool,
        snapshot_worker_failed: bool,
        /// Shared with each [`FixtureOrigin`]. Tests store milliseconds here
        /// to delay the TLS client before it connects. Zero does not wait.
        probe_delay_ms: Arc<AtomicU64>,
    }

    pub(super) struct FixtureDrain {
        jobs: JoinSet<Result<()>>,
        snapshot_jobs: JoinSet<Result<()>>,
        live: Arc<Mutex<Live>>,
        failed: bool,
    }

    impl FixtureDrain {
        pub(super) async fn join(mut self) -> Result<Self> {
            while let Some(result) = self.jobs.join_next().await {
                self.failed |= !matches!(result, Ok(Ok(())));
            }
            while let Some(result) = self.snapshot_jobs.join_next().await {
                self.failed |= !matches!(result, Ok(Ok(())));
            }
            ensure!(!self.failed, "fixture job failed");
            Ok(self)
        }

        /// Release idle boots once every worker holding a slot is joined.
        pub(super) async fn release(self) -> Result<()> {
            let (boots, release_pending) = {
                let mut live = self.live.lock().unwrap();
                (
                    std::mem::take(&mut live.boots),
                    !live.release_pending.is_empty(),
                )
            };
            for slot in boots.into_values() {
                if let Slot::Idle(boot) = slot {
                    (*boot).release().await?;
                }
            }
            ensure!(!release_pending, "fixture boot release unconfirmed");
            Ok(())
        }
    }

    impl Fixture {
        fn new(root: &Path, origins: Arc<Mutex<HashMap<Id, Arc<FixtureOrigin>>>>) -> Self {
            Self {
                static_compose: false,
                jobs: JoinSet::new(),
                snapshot_jobs: JoinSet::new(),
                live: Arc::new(Mutex::new(Live::default())),
                live_changed: Arc::new(std::sync::Condvar::new()),
                handle: tokio::runtime::Handle::current(),
                origins,
                artifacts: Artifacts(Arc::new(FixtureCustody::open(root).unwrap())),
                build_hold: None,
                script_hold: None,
                static_hold: None,
                static_fault: None,
                windows_recipe: false,
                snapshot_test_media: false,
                host: HOST,
                host_observer: None,
                snapshot_hold: None,
                snapshot_fault: None,
                skip_restore_map_update: false,
                fail_cancelled: false,
                failed: false,
                snapshot_worker_failed: false,
                probe_delay_ms: Arc::new(AtomicU64::new(0)),
            }
        }

        fn boot_failure_stage(&self, vm: Id) -> Option<&'static str> {
            self.live
                .lock()
                .unwrap()
                .boot_failures
                .get(&vm)
                .copied()
                .map(FixtureBootStage::label)
        }

        pub(super) fn ready(&self, vm: Id) -> bool {
            self.live.lock().unwrap().boots.contains_key(&vm)
        }

        pub(super) fn worker_active(&self, vm: Id) -> bool {
            self.live.lock().unwrap().working.contains(&vm)
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
                after_selection: Default::default(),
                live: self.live.clone(),
                live_changed: self.live_changed.clone(),
                handle: self.handle.clone(),
                custody: self.artifacts.0.clone(),
            }
        }

        fn reap(&mut self) {
            while let Some(result) = self.jobs.try_join_next() {
                self.failed |= !matches!(result, Ok(Ok(())));
            }
            // Snapshot worker failures or panics are preserved for shutdown
            // evidence, while leaving same-process foreground SDK available
            // once the reaper reconciles the orphaned durable intent.
            while let Some(result) = self.snapshot_jobs.try_join_next() {
                self.snapshot_worker_failed |= !matches!(result, Ok(Ok(())));
            }
        }

        pub(super) fn take_jobs(&mut self) -> FixtureDrain {
            FixtureDrain {
                jobs: std::mem::take(&mut self.jobs),
                snapshot_jobs: std::mem::take(&mut self.snapshot_jobs),
                live: self.live.clone(),
                failed: self.failed || self.snapshot_worker_failed,
            }
        }

        pub(super) fn snapshot(
            &mut self,
            inventory: &VmInventory,
            action: i64,
            mutation: Mutation,
            mut route: Option<runtime::publication::SnapshotRoute>,
        ) -> Outcome<Value> {
            self.reap();
            let Mutation {
                key,
                id: vm,
                work: Work::Snapshot(request),
            } = mutation
            else {
                unreachable!("snapshot mutation");
            };
            if self.failed {
                return Err(STATE_UNAVAILABLE);
            }
            let controller = &inventory.supervisor.controller;
            let mut registry = self.live.lock().unwrap();
            if registry.release_pending.contains(&vm)
                || registry.working.contains(&vm)
                || registry.boots.contains_key(&vm)
            {
                return inventory.refuse(action, Some(vm), Refusal::SnapshotOfflineRequired.into());
            }
            if let Some(route) = route.as_mut()
                && let Some((generation, epoch)) = controller.active_release_epoch(vm, true)?
            {
                // Independent local-origin release proof: no fixture boot or
                // live origin listener exists for this exact generation/epoch.
                route.bind_origin_release(generation, epoch);
            }
            let (work, worker, operation) = match controller.admit_snapshot(
                &inventory.principal,
                key,
                vm,
                action,
                &request,
                route.as_ref(),
                self.host,
                RESOURCES,
            )? {
                Ok(work) => work,
                Err(reason) => return inventory.refuse(action, Some(vm), reason.into()),
            };
            if work.candidate.is_none() {
                let reaper = self.reaper();
                drop(registry);
                let controller = controller.clone();
                self.snapshot_jobs.spawn(async move {
                    // Transient SQL settlement failures keep claims armed for
                    // the periodic reaper to retry after backoff.
                    let _ = controller.sweep_with(&reaper, &*reaper.custody);
                    Ok(())
                });
            } else {
                let artifacts = self.artifacts.clone();
                let cancel = inventory.supervisor.cancel.clone();
                let vm_cancel = Shutdown::default();
                let controller = controller.clone();
                registry.working.insert(vm);
                registry.snapshot_cancels.insert(vm, vm_cancel.clone());
                let working = Working(self.live.clone(), self.live_changed.clone(), vm);
                let host = self.host;
                let host_observer = self
                    .host_observer
                    .clone()
                    .unwrap_or_else(|| Arc::new(move || host));
                let hold = self.snapshot_hold.take();
                let fault = self.snapshot_fault.take();
                let skip_map_update = self.skip_restore_map_update;
                self.snapshot_jobs.spawn(async move {
                    let _working = working;
                    let copied = copy_fixture_snapshot(
                        &artifacts,
                        &controller,
                        &work,
                        &worker,
                        route.as_ref(),
                        host_observer,
                        hold,
                        fault,
                        &cancel,
                        &vm_cancel,
                    )
                    .await;
                    match copied {
                        Ok(pair) if work.action == 6 && !skip_map_update => {
                            let mut live = _working.0.lock().unwrap();
                            if work.is_static {
                                if let Some((current, _)) = live.static_instances.get_mut(&vm) {
                                    *current = pair;
                                }
                            } else {
                                live.instances.insert(vm, pair);
                            }
                        }
                        Err(leaves) => controller.fail_snapshot_copy(&worker, &work, leaves)?,
                        _ => {}
                    }
                    Ok(())
                });
            }
            Ok(receipt(operation.id, None))
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
            if registry.release_pending.contains(&id) {
                return inventory.refuse(action, Some(id), Refusal::CleanupUnconfirmed.into());
            }
            let (operation, vm) = match work {
                Work::Build => {
                    let recipe = hex::encode(id);
                    let web = id == WEB_RECIPE_ID
                        || inventory.supervisor.recipes[3].is_some_and(|(recipe, _)| recipe == id);
                    let windows = id == Controller::windows_recipe_id();
                    let snapshot_image = self.snapshot_test_media;
                    let (retained, transient) = if web {
                        (WEB_RETAINED, WEB_TRANSIENT)
                    } else if windows && snapshot_image {
                        (WEB_RETAINED, WINDOWS_TRANSIENT)
                    } else if windows {
                        (RETAINED, WINDOWS_TRANSIENT)
                    } else if snapshot_image {
                        (WEB_RETAINED, WEB_TRANSIENT)
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
                                &|slot, bytes| {
                                    if snapshot_image && slot == slots.pair.disk {
                                        artifacts.stage(slot, &fixture_qcow2())
                                    } else {
                                        artifacts.stage(slot, bytes)
                                    }
                                },
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
                            let file = if (web || snapshot_image) && slot == disk {
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
                    .ok_or(Fault::new("disk"))?;
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
                        (
                            WEB_DIGEST,
                            inventory
                                .supervisor
                                .static_selection()
                                .and_then(|selected| selected.fixture_evidence()),
                        ),
                        disk,
                        self.host,
                        RESOURCES,
                    )? {
                        Ok(admitted) => admitted,
                        Err(reason) => return inventory.refuse(action, None, reason.into()),
                    };
                    let hold = self.static_hold.take();
                    let fault = self.static_fault.take();
                    let origins = self.origins.clone();
                    let probe_delay_ms = self.probe_delay_ms.clone();
                    self.jobs.spawn(async move {
                        let copied = if released(hold, &cancel).await {
                            copy_and_provision(
                                &artifacts,
                                (golden, proof),
                                slots,
                                fault,
                                &cancel,
                                probe_delay_ms,
                            )
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
                            let origin = Arc::new(origin);
                            origins
                                .lock()
                                .unwrap()
                                .insert(slots.disk.generation, origin.clone());
                            registry
                                .static_instances
                                .insert(vm, ((slots, pair), origin));
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
                    let snapshot_image = self.snapshot_test_media;
                    let request = Request {
                        principal,
                        request_id: key,
                        vm: [0; 16],
                        disk: if snapshot_image {
                            WEB_RETAINED
                        } else {
                            RETAINED
                        },
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
                        let disk = if snapshot_image {
                            artifacts.stage(slots.disk, &fixture_snapshot_overlay(golden.disk))?
                        } else {
                            artifacts.write(slots.disk)?
                        };
                        let pair = [disk, artifacts.write(slots.vars)?];
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
                Work::Publish | Work::Unpublish | Work::Snapshot(_) => {
                    unreachable!("separate mutation admission")
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
            if matches!(work, Work::Start) {
                if !controller
                    .foreground_static_rows(principal, Some(vm), None, 1)?
                    .is_empty()
                {
                    // Always rehydrate the committed pair from durable custody
                    // records, preserving host-side FixtureOrigin trust when
                    // already loaded in this process.
                    match controller.static_custody(principal, vm)? {
                        Some((slots, evidence, origin_generation)) => {
                            let origin = registry
                                .static_instances
                                .get(&vm)
                                .filter(|(_, existing)| existing.generation == origin_generation)
                                .map(|(_, existing)| existing.clone())
                                .or_else(|| {
                                    self.origins
                                        .lock()
                                        .unwrap()
                                        .get(&origin_generation)
                                        .cloned()
                                })
                                .map(Ok)
                                .unwrap_or_else(|| {
                                    let origin = Arc::new(FixtureOrigin::with_probe_delay(
                                        origin_generation,
                                        self.probe_delay_ms.clone(),
                                    )?);
                                    self.origins
                                        .lock()
                                        .unwrap()
                                        .insert(origin_generation, origin.clone());
                                    Ok::<_, anyhow::Error>(origin)
                                })
                                .map_err(|_| STATE_UNAVAILABLE)?;
                            registry
                                .static_instances
                                .insert(vm, ((slots, evidence), origin));
                        }
                        None => {
                            return inventory.refuse(action, Some(vm), Refusal::NotReady.into());
                        }
                    }
                } else if controller.has_test_generation(principal, vm)? {
                    // Always rehydrate the committed TEST pair from durable
                    // custody records so reopen or crash-after-commit never
                    // bypasses paired file verification before boot.
                    match controller.test_custody(principal, vm)? {
                        Some(pair) => {
                            registry.instances.insert(vm, pair);
                        }
                        None => {
                            return inventory.refuse(action, Some(vm), Refusal::NotReady.into());
                        }
                    }
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
            let operation_id = controller.foreground_operation_ref(principal, activity)?;
            // Registered synchronously with the committed admission; the
            // guard leaves the reaper's offline proof closed until joined.
            registry.working.insert(vm);
            let working = Working(live.clone(), self.live_changed.clone(), vm);
            match work {
                Work::Start => {
                    let controller = controller.clone();
                    let static_pair = registry.static_instances.get(&vm).cloned();
                    let test_pair = registry.instances.get(&vm).copied();
                    let writer_epoch = operation_id;
                    self.jobs.spawn(async move {
                        let _working = working;
                        let boot = match static_pair {
                            Some((pair, identity)) => {
                                Boot::start_static(&artifacts, pair, &identity, &cancel).await
                            }
                            None => {
                                let verified = test_pair
                                    .map(|(slots, [disk, vars])| {
                                        artifacts.verify(slots.disk, disk)?;
                                        artifacts.verify(slots.vars, vars)
                                    })
                                    .transpose();
                                match verified {
                                    Ok(_) => Boot::start(&cancel).await.map_err(|_| {
                                        FixtureBootFault::at(FixtureBootStage::QmpGuestReadiness)
                                    }),
                                    Err(_) => {
                                        Err(FixtureBootFault::at(FixtureBootStage::ArtifactHold))
                                    }
                                }
                            }
                        };
                        match boot {
                            Ok(mut boot) => {
                                boot.writer_epoch = writer_epoch;
                                // Publish ready only if this worker's settlement
                                // won; a worker that lost to expiry or shutdown
                                // is reaped without a ready boot.
                                let settled = {
                                    let mut registry = live.lock().unwrap();
                                    let status =
                                        controller.finish(&worker, Completion::Succeeded)?;
                                    if status == Status::Succeeded {
                                        registry.boot_failures.remove(&vm);
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
                            Err(fault) => {
                                {
                                    let mut registry = live.lock().unwrap();
                                    match fault.stage {
                                        Some(stage) => {
                                            registry.boot_failures.insert(vm, stage);
                                        }
                                        None => {
                                            registry.boot_failures.remove(&vm);
                                        }
                                    }
                                }
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
                    let writer_epoch = boot.as_ref().map(|boot| boot.writer_epoch);
                    let dirty = registry.dirty_stop.remove(&vm);
                    if let Some(origin) =
                        boot.as_ref().and_then(|boot| boot.publish_origin.as_ref())
                    {
                        origin.retired.retire();
                    }
                    let pair = registry
                        .static_instances
                        .get(&vm)
                        .map(|(pair, _)| *pair)
                        .or_else(|| registry.instances.get(&vm).copied());
                    let supervisor = inventory.supervisor.clone();
                    let controller = controller.clone();
                    self.jobs.spawn(async move {
                        let _working = working;
                        let local = supervisor.publisher.stop_local(Some(vm)).await;
                        if dirty && let Some(boot) = boot {
                            boot.release().await?;
                            if local {
                                match pair.map(|pair| artifacts.released_pair(pair)).transpose() {
                                    Ok(Some((held, pair))) => {
                                        let mut live = live.lock().unwrap();
                                        controller.finish_fixture_stop(
                                            &worker,
                                            vm,
                                            writer_epoch.unwrap_or([0; 16]),
                                            Some(pair),
                                            runtime::StopDisposition::DirtyRelease,
                                        )?;
                                        if let Some((current, _)) =
                                            live.static_instances.get_mut(&vm)
                                        {
                                            *current = pair;
                                        } else {
                                            live.instances.insert(vm, pair);
                                        }
                                        drop(held);
                                    }
                                    Ok(None) => {
                                        controller.finish_fixture_stop(
                                            &worker,
                                            vm,
                                            writer_epoch.unwrap_or([0; 16]),
                                            None,
                                            runtime::StopDisposition::DirtyRelease,
                                        )?;
                                    }
                                    Err(_) => {
                                        controller.finish_with_cause(
                                            &worker,
                                            Completion::CleanupUnconfirmed,
                                            Some(FailureReason::DirtyStop),
                                        )?;
                                    }
                                }
                                return Ok(());
                            }
                            controller.finish_with_cause(
                                &worker,
                                Completion::CleanupUnconfirmed,
                                Some(FailureReason::DirtyStop),
                            )?;
                            anyhow::bail!("publication stop unconfirmed");
                        }
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
                        if local && completion == Completion::Succeeded {
                            match pair.map(|pair| artifacts.released_pair(pair)).transpose() {
                                Ok(Some((held, pair))) => {
                                    let mut live = live.lock().unwrap();
                                    if controller.finish_fixture_stop(
                                        &worker,
                                        vm,
                                        writer_epoch.unwrap_or([0; 16]),
                                        Some(pair),
                                        runtime::StopDisposition::CleanRelease,
                                    )? == Status::Succeeded
                                    {
                                        if let Some((current, _)) =
                                            live.static_instances.get_mut(&vm)
                                        {
                                            *current = pair;
                                        } else {
                                            live.instances.insert(vm, pair);
                                        }
                                    }
                                    drop(held);
                                }
                                Ok(None) => {
                                    controller.finish_fixture_stop(
                                        &worker,
                                        vm,
                                        writer_epoch.unwrap_or([0; 16]),
                                        None,
                                        runtime::StopDisposition::CleanRelease,
                                    )?;
                                }
                                Err(_) => {
                                    controller.finish_with_cause(
                                        &worker,
                                        Completion::CleanupUnconfirmed,
                                        Some(FailureReason::CleanupUnconfirmed),
                                    )?;
                                }
                            }
                        } else {
                            controller.finish_with_cause(&worker, completion, cause)?;
                        }
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
                    worker.extend_deadline_for_test(Duration::from_secs(6));
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
                                origin.retired.retire();
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
        probe_delay_ms: Arc<AtomicU64>,
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
        let origin = FixtureOrigin::with_probe_delay(slots.disk.generation, probe_delay_ms);
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
    struct Working(Arc<Mutex<Live>>, Arc<std::sync::Condvar>, Id);

    impl Drop for Working {
        fn drop(&mut self) {
            if let Ok(mut live) = self.0.lock() {
                live.working.remove(&self.2);
                live.snapshot_cancels.remove(&self.2);
                self.1.notify_all();
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
        writer_epoch: Id,
    }

    /// Classified fixture boot failure. The receipt stays the sanitized
    /// `boot_failed` / `cleanup_unconfirmed` pair; the stage is test-only.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum FixtureBootStage {
        ArtifactHold,
        PortReservation,
        QmpGuestReadiness,
        TlsIo,
        TlsTimeout,
    }

    impl FixtureBootStage {
        const fn label(self) -> &'static str {
            match self {
                Self::ArtifactHold => "artifact_hold",
                Self::PortReservation => "port_reservation",
                Self::QmpGuestReadiness => "qmp_guest_readiness",
                Self::TlsIo => "tls_io",
                Self::TlsTimeout => "tls_timeout",
            }
        }
    }

    #[derive(Debug, PartialEq, Eq)]
    struct FixtureBootFault {
        stage: Option<FixtureBootStage>,
    }

    impl FixtureBootFault {
        const fn at(stage: FixtureBootStage) -> Self {
            Self { stage: Some(stage) }
        }

        const fn unstaged() -> Self {
            Self { stage: None }
        }
    }

    /// Synthetic guest endpoint and independently configured host trust.
    /// This fixture has no production serial encoder, and never logs key bytes.
    struct FixtureOrigin {
        generation: Id,
        name: String,
        guest: Arc<rustls::ServerConfig>,
        expired_guest: Arc<rustls::ServerConfig>,
        untrusted_guest: Arc<rustls::ServerConfig>,
        client: Arc<rustls::ClientConfig>,
        probe_delay_ms: Arc<AtomicU64>,
    }

    impl FixtureOrigin {
        fn new(generation: Id) -> Result<Self> {
            Self::with_probe_delay(generation, Arc::new(AtomicU64::new(0)))
        }

        fn with_probe_delay(generation: Id, probe_delay_ms: Arc<AtomicU64>) -> Result<Self> {
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
            ca.not_before = now - time::Duration::days(3);
            ca.not_after = now + time::Duration::days(3);
            let ca_key = KeyPair::generate()?;
            let certificate = ca.self_signed(&ca_key)?;
            let issuer = Issuer::new(ca, ca_key);
            let mut untrusted_ca = CertificateParams::default();
            untrusted_ca.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
            untrusted_ca.key_usages = vec![KeyUsagePurpose::KeyCertSign];
            untrusted_ca.not_before = now - time::Duration::days(3);
            untrusted_ca.not_after = now + time::Duration::days(3);
            let untrusted_ca_key = KeyPair::generate()?;
            let _ = untrusted_ca.self_signed(&untrusted_ca_key)?;
            let untrusted_issuer = Issuer::new(untrusted_ca, untrusted_ca_key);
            let provider = Arc::new(rustls::crypto::ring::default_provider());
            let sign_leaf = |not_before,
                             not_after,
                             cert: rcgen::Certificate,
                             key: KeyPair|
             -> Result<Arc<rustls::ServerConfig>> {
                let _ = (not_before, not_after);
                Ok(Arc::new(
                    rustls::ServerConfig::builder_with_provider(provider.clone())
                        .with_safe_default_protocol_versions()?
                        .with_no_client_auth()
                        .with_single_cert(
                            vec![cert.der().clone()],
                            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der())),
                        )?,
                ))
            };
            let make_leaf = |not_before, not_after| -> Result<CertificateParams> {
                let mut leaf = CertificateParams::new(vec![name.clone()])?;
                leaf.not_before = not_before;
                leaf.not_after = not_after;
                leaf.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
                Ok(leaf)
            };
            let key = KeyPair::generate()?;
            let cert = make_leaf(
                now - time::Duration::minutes(1),
                now + time::Duration::days(1),
            )?
            .signed_by(&key, &issuer)?;
            let guest = sign_leaf((), (), cert, key)?;
            let expired_key = KeyPair::generate()?;
            let expired_cert =
                make_leaf(now - time::Duration::days(2), now - time::Duration::days(1))?
                    .signed_by(&expired_key, &issuer)?;
            let expired_guest = sign_leaf((), (), expired_cert, expired_key)?;
            let untrusted_key = KeyPair::generate()?;
            let untrusted_cert = make_leaf(
                now - time::Duration::minutes(1),
                now + time::Duration::days(1),
            )?
            .signed_by(&untrusted_key, &untrusted_issuer)?;
            let untrusted_guest = sign_leaf((), (), untrusted_cert, untrusted_key)?;
            let mut roots = rustls::RootCertStore::empty();
            roots.add(certificate.der().clone())?;
            let client = rustls::ClientConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()?
                .with_root_certificates(roots)
                .with_no_client_auth();
            Ok(Self {
                generation,
                name,
                guest,
                expired_guest,
                untrusted_guest,
                client: Arc::new(client),
                probe_delay_ms,
            })
        }

        async fn probe(
            &self,
            lease: &crate::vm::static_web::OriginPortLease,
            expected: String,
        ) -> std::result::Result<(), FixtureBootFault> {
            self.probe_with(lease, expected, self.guest.clone(), &Shutdown::default())
                .await
        }

        /// Both sides of the health exchange, including an optional client
        /// delay, run until they finish or the fixture shuts down.
        /// [`FIXTURE_TLS_DEADLINE`] is only the hang guard.
        async fn probe_with(
            &self,
            lease: &crate::vm::static_web::OriginPortLease,
            expected: String,
            guest_config: Arc<rustls::ServerConfig>,
            cancel: &Shutdown,
        ) -> std::result::Result<(), FixtureBootFault> {
            let listener = tokio::net::TcpListener::from_std(
                lease
                    .fixture_listener()
                    .map_err(|_| FixtureBootFault::at(FixtureBootStage::PortReservation))?,
            )
            .map_err(|_| FixtureBootFault::at(FixtureBootStage::PortReservation))?;
            let address = listener
                .local_addr()
                .map_err(|_| FixtureBootFault::at(FixtureBootStage::PortReservation))?;
            let tls = tokio_rustls::TlsAcceptor::from(guest_config);
            let connector = tokio_rustls::TlsConnector::from(self.client.clone());
            const REQUEST: &[u8] =
                b"GET /arx-health HTTP/1.1\r\nHost: fixture\r\nConnection: close\r\n\r\n";
            const RESPONSE: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\nok\n";
            let delay = Duration::from_millis(
                self.probe_delay_ms
                    .load(std::sync::atomic::Ordering::Relaxed),
            );
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
                if !delay.is_zero() {
                    tokio::time::sleep(delay).await;
                }
                let socket = tokio::net::TcpStream::connect(address).await?;
                let mut stream = connector.connect(expected.try_into()?, socket).await?;
                stream.write_all(REQUEST).await?;
                let mut response = [0; RESPONSE.len()];
                stream.read_exact(&mut response).await?;
                ensure!(response == RESPONSE, "invalid fixture health response");
                Ok::<_, anyhow::Error>(())
            };
            let exchange = async { tokio::try_join!(host, guest).map(|_| ()) };
            tokio::select! {
                biased;
                _ = cancel.cancelled() => Err(FixtureBootFault::unstaged()),
                result = tokio::time::timeout(FIXTURE_TLS_DEADLINE, exchange) => match result {
                    Ok(Ok(())) => Ok(()),
                    Ok(Err(_)) => Err(FixtureBootFault::at(FixtureBootStage::TlsIo)),
                    Err(_) => Err(FixtureBootFault::at(FixtureBootStage::TlsTimeout)),
                },
            }
        }
    }

    const FIXTURE_CERT_MARKER_OFFSET: u64 = 4097;

    impl Boot {
        async fn start_static(
            artifacts: &Artifacts,
            (slots, [disk, vars]): Pair,
            identity: &FixtureOrigin,
            cancel: &Shutdown,
        ) -> std::result::Result<Self, FixtureBootFault> {
            use std::io::{Read, Seek, SeekFrom};
            let artifact = || FixtureBootFault::at(FixtureBootStage::ArtifactHold);
            let port = || FixtureBootFault::at(FixtureBootStage::PortReservation);
            let mut held_disk = artifacts
                .hold(slots.disk, disk, false)
                .map_err(|_| artifact())?;
            let held_vars = artifacts
                .hold(slots.vars, vars, false)
                .map_err(|_| artifact())?;
            let cert_marker = if disk.pin.size > FIXTURE_CERT_MARKER_OFFSET {
                held_disk
                    .file
                    .seek(SeekFrom::Start(FIXTURE_CERT_MARKER_OFFSET))
                    .map_err(|_| artifact())?;
                let mut byte = [0u8; 1];
                held_disk
                    .file
                    .read_exact(&mut byte)
                    .map_err(|_| artifact())?;
                byte[0]
            } else {
                0
            };
            drop(held_disk);
            drop(held_vars);
            let guest_config = match cert_marker {
                b'E' => identity.expired_guest.clone(),
                b'U' => identity.untrusted_guest.clone(),
                _ => identity.guest.clone(),
            };
            let origin = crate::vm::static_web::OriginPortLease::acquire().map_err(|_| port())?;
            if !origin.verify() {
                return Err(port());
            }
            let mut boot = Self::start(cancel)
                .await
                .map_err(|_| FixtureBootFault::at(FixtureBootStage::QmpGuestReadiness))?;
            if let Err(fault) = identity
                .probe_with(&origin, identity.name.clone(), guest_config, cancel)
                .await
            {
                // Keep the probe stage when release itself fails.
                let _ = boot.release().await;
                return Err(fault);
            }
            let port_number = origin.port().try_into().map_err(|_| port())?;
            let boot_id = crate::store::vm::new_id().map_err(|_| FixtureBootFault::unstaged())?;
            boot.publish_origin = Some(Origin {
                generation: slots.disk.generation,
                origin_generation: identity.generation,
                boot: boot_id,
                port: port_number,
                ca_path: artifacts.0.path("origin_ca", slots.disk.generation),
                ca_receipt: crate::vm::paths::ArtifactPin {
                    size: 1,
                    sha256: [1; 32],
                },
                retired: Default::default(),
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
                    writer_epoch: [0; 16],
                }),
                Err(_) => {
                    peer.await??;
                    guest.await??;
                    anyhow::bail!("fixture boot unconfirmed")
                }
            }
        }

        async fn force_release(self) -> Result<()> {
            if let Some(origin) = &self.publish_origin {
                origin.retired.retire();
            }
            self.peer.abort();
            self.guest.abort();
            drop(self.qmp);
            drop(self.channel);
            let _ = self.peer.await;
            let _ = self.guest.await;
            Ok(())
        }

        async fn release(self) -> Result<()> {
            if let Some(origin) = &self.publish_origin {
                origin.retired.retire();
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
                    if matches!(
                        script.as_str(),
                        "exit 7" | "exit 0" | VAULT_EXIT_7 | VAULT_EXIT_0
                    ) =>
                {
                    Response::ScriptResult {
                        id,
                        status: ScriptStatus::Exited,
                        exit_code: Some(if matches!(script.as_str(), "exit 7" | VAULT_EXIT_7) {
                            7
                        } else {
                            0
                        }),
                    }
                }
                _ => anyhow::bail!("unexpected fixture guest request"),
            };
            let mut frame = Vec::new();
            arx_guest::write_frame(&mut frame, &response)?;
            ensure!(
                !frame
                    .windows(VAULT_CANARY.len())
                    .any(|window| window == VAULT_CANARY.as_bytes()),
                "fixture guest response leaked script bytes"
            );
            stream.write_all(&frame).await?;
        }
        Ok(())
    }

    pub(super) fn key(number: u64) -> String {
        format!("{UTC:016x}{number:016x}")
    }

    pub(super) fn request(name: &str, arguments: Value) -> CallToolRequestParams {
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
        // Hang guard only. A settled row returns at once. Expected static
        // starts use [`terminal_static_start`] so a slow probe can finish.
        await_operation(client, reference, Duration::from_secs(2)).await
    }

    async fn terminal_static_start(client: &Client, reference: &Value) -> Value {
        await_operation(client, reference, FIXTURE_START_GUARD).await
    }

    async fn await_operation(client: &Client, reference: &Value, bound: Duration) -> Value {
        assert_eq!(reference["ok"], true, "{reference}");
        tokio::time::timeout(bound, async {
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

    fn fixture_boot_stage(h: &Harness, vm: &Value) -> Option<&'static str> {
        let id = parse_id(vm.as_str().expect("vm id")).expect("vm id");
        h.fixture(|fixture| fixture.boot_failure_stage(id))
    }

    /// Success prints no stage. A failure prints the sanitized receipt
    /// `reason` and `cleanup` beside the retained fixture stage.
    fn assert_settled_static_start(h: &Harness, vm: &Value, started: &Value) {
        let stage = fixture_boot_stage(h, vm);
        assert_eq!(
            (
                started.get("state").and_then(Value::as_str),
                started.get("reason").and_then(Value::as_str),
                started.get("cleanup").and_then(Value::as_str),
                stage,
            ),
            (Some("succeeded"), None, Some("confirmed"), None),
            "fixture boot stage {stage:?}: {started}"
        );
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

    type HarnessState = (
        tempfile::TempDir,
        std::path::PathBuf,
        Arc<AtomicI64>,
        Arc<Mutex<HashMap<Id, Arc<FixtureOrigin>>>>,
    );

    struct Harness {
        temp: tempfile::TempDir,
        media: std::path::PathBuf,
        now: Arc<AtomicI64>,
        origins: Arc<Mutex<HashMap<Id, Arc<FixtureOrigin>>>>,
        supervisor: Option<Arc<Supervisor>>,
        endpoint: Endpoint,
        tokens: [String; 2],
        ticks: tokio::sync::mpsc::UnboundedSender<TickCompletion>,
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
            let origins = Arc::new(Mutex::new(HashMap::new()));
            Self::launch(
                (temp, media, now, origins),
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
            (temp, media, now, origins): HarnessState,
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
            let mut fixture = Fixture::new(&media, origins.clone());
            fixture.build_hold = Some(hold_build.clone());
            fixture.script_hold = Some(hold_script.clone());
            fixture.fail_cancelled = fail_cancelled;
            configure(&mut fixture);
            if fixture.static_compose {
                Arc::get_mut(&mut supervisor)
                    .unwrap()
                    .install_static_selection(Ok(
                        selected_static::FixtureStaticQualification::select(),
                    ))
                    .unwrap();
            }
            if fixture.windows_recipe {
                Arc::get_mut(&mut supervisor).unwrap().recipes[2] = Some((
                    Controller::windows_recipe_id(),
                    Some(Controller::windows_fixture_digest()),
                ));
            }
            supervisor.gate.lock().unwrap().fixture = Some(fixture);
            let (ticks, receiver) = TestTicks::channel();
            *supervisor.ticks.lock().unwrap() = Some(receiver);
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
                origins,
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

        /// Run one synchronous reaper sweep on the blocking pool so workers and
        /// boot tasks on a current-thread test runtime can make progress.
        async fn sweep(&self) {
            let supervisor = self.supervisor().clone();
            tokio::task::spawn_blocking(move || supervisor.sweep())
                .await
                .unwrap()
                .unwrap();
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
                (self.temp, self.media, self.now, self.origins),
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
            self.ticks.send(None).unwrap();
        }

        async fn tick_and_wait(&self) {
            let (completed, wait) = tokio::sync::oneshot::channel();
            self.ticks.send(Some(completed)).unwrap();
            tokio::time::timeout(Duration::from_secs(5), wait)
                .await
                .expect("reaper turn did not complete")
                .expect("reaper exited before completing the turn");
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
        assert_eq!(
            terminal_static_start(&a, &start).await["state"],
            "succeeded"
        );
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
        assert_eq!(
            terminal_static_start(&a, &start).await["state"],
            "succeeded"
        );
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

    fn fixture_pair_paths(h: &Harness, vm: &Value, active: bool) -> [std::path::PathBuf; 2] {
        let vm = parse_id(vm.as_str().unwrap()).unwrap();
        h.supervisor()
            .controller
            .store_for_test()
            .transact(|db| {
                let mut query = db.prepare(
                    "SELECT a.role,a.object_key FROM vm_generations g
                 JOIN vm_artifacts a ON a.generation_id=g.id
                 WHERE (g.vm_id=?1 OR g.static_id=?1) AND g.active=?2
                    AND a.state=CASE WHEN ?2 THEN 'active' ELSE 'pending_disposal' END
                 ORDER BY role LIMIT 3",
                )?;
                let files = query
                    .query_map(rusqlite::params![vm, active], |row| {
                        Ok(h.media.join(format!(
                            "{}-{}",
                            row.get::<_, String>(0)?,
                            hex::encode(row.get::<_, Id>(1)?)
                        )))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(files.try_into().expect("exact fixture pair"))
            })
            .unwrap()
    }

    fn simulate_pair_write(paths: &[std::path::PathBuf; 2]) -> [Vec<u8>; 2] {
        use std::io::{Seek, SeekFrom, Write};
        let saved = paths.each_ref().map(|path| fs::read(path).unwrap());
        for (path, offset, marker) in [(&paths[0], 4096, b'D'), (&paths[1], 0, b'V')] {
            let mut writer = fs::OpenOptions::new().write(true).open(path).unwrap();
            writer.seek(SeekFrom::Start(offset)).unwrap();
            writer.write_all(&[marker]).unwrap();
            writer.sync_all().unwrap();
        }
        saved
    }

    #[tokio::test]
    async fn static_snapshot_keeps_origin_and_refuses_a_durable_b2_route() {
        use std::sync::atomic::Ordering::SeqCst;
        let mut h = Harness::start_with_web(false, false, true).await;
        let a = h.client(0).await;
        let b = h.client(1).await;
        let (vm, fake) = enrolled(&h, &a, 1800).await;
        let args = |nonce| json!({"vm_id":vm,"request_id":key(nonce)});
        let published = call(&a, "publish_site", args(1803)).await;
        assert_eq!(terminal(&a, &published).await["state"], "succeeded");
        let capture = json!({"vm_id":vm,"request_id":key(1804),"name":"Before-Update"});
        assert_eq!(
            call(&a, "vm_snapshot_create", capture.clone()).await["reason"],
            "snapshot_publish_not_quiesced"
        );
        let unpublished = call(&a, "unpublish_site", args(1805)).await;
        assert_eq!(terminal(&a, &unpublished).await["state"], "succeeded");
        let stop = call(&a, "vm_stop", args(1806)).await;
        assert_eq!(terminal(&a, &stop).await["state"], "succeeded");

        let store = h.supervisor().controller.store_for_test();
        let vm_id = parse_id(vm.as_str().unwrap()).unwrap();
        let generation = |column: &str| {
            let sql = match column {
                "origin" => "SELECT origin_generation FROM vm_static_instances WHERE id=?1",
                _ => "SELECT id FROM vm_generations WHERE static_id=?1 AND active=1",
            };
            store
                .transact(|db| Ok(db.query_row(sql, [vm_id], |row| row.get::<_, Id>(0))?))
                .unwrap()
        };
        let original = generation("origin");
        assert_eq!(original, generation("active"));

        let created = call(&a, "vm_snapshot_create", capture.clone()).await;
        let row = terminal(&a, &created).await;
        assert_eq!(row["state"], "succeeded", "{row}");
        let point = row["snapshot_id"].clone();
        assert_eq!(
            call(
                &a,
                "vm_snapshot_create",
                json!({"vm_id":vm,"request_id":key(1804),"name":"before-update"})
            )
            .await,
            created
        );
        assert_eq!(
            call(
                &a,
                "vm_snapshot_create",
                json!({"vm_id":vm,"request_id":key(1804),"name":"other"})
            )
            .await["reason"],
            "key_collision"
        );
        let running = call(&a, "vm_start", args(1811)).await;
        assert_eq!(
            terminal_static_start(&a, &running).await["state"],
            "succeeded"
        );
        let original_paths = fixture_pair_paths(&h, &vm, true);
        let saved = simulate_pair_write(&original_paths);
        let stopped = call(&a, "vm_stop", args(1812)).await;
        assert_eq!(terminal(&a, &stopped).await["state"], "succeeded");
        let restore = json!({"vm_id":vm,"snapshot_id":point,"request_id":key(1807)});
        let restored = call(&a, "vm_snapshot_restore", restore.clone()).await;
        let row = terminal(&a, &restored).await;
        assert_eq!(row["state"], "succeeded", "{row}");
        assert_eq!(row["snapshot_id"], point);
        assert_eq!(
            fixture_pair_paths(&h, &vm, true).map(|path| fs::read(path).unwrap()),
            saved
        );
        assert_eq!(fs::read(&original_paths[0]).unwrap()[4096], b'D');
        assert_eq!(fs::read(&original_paths[1]).unwrap()[0], b'V');
        assert_ne!(generation("active"), original);
        assert_eq!(generation("origin"), original);
        store
            .transact(|db| {
                assert!(
                    db.execute("DELETE FROM vm_restore_points WHERE static_vm=?1", [vm_id])
                        .is_err()
                );
                assert_eq!(
                    db.query_row(
                        "SELECT count(*) FROM vm_restore_points WHERE static_vm=?1",
                        [vm_id],
                        |row| row.get::<_, i64>(0)
                    )?,
                    2
                );
                Ok(())
            })
            .unwrap();
        let list_args = json!({"vm_id":vm,"limit":1});
        let page = call(&a, "vm_snapshot_list", list_args.clone()).await;
        assert_eq!(page["entries"][0]["protected"], true);
        assert!(page["next_cursor"].is_string());
        let next = json!({"vm_id":vm,"limit":1,"cursor":page["next_cursor"]});
        assert_eq!(
            call(&b, "vm_snapshot_list", next.clone()).await["reason"],
            "invalid_cursor"
        );
        let page2 = call(&a, "vm_snapshot_list", next).await;
        assert_eq!(page2["entries"][0]["protected"], true);
        assert!(page2["next_cursor"].is_null());
        let delete = json!({"vm_id":vm,"snapshot_id":point,"request_id":key(1808)});
        // Non-retryable delete refusals keep the exact two-key shape.
        assert_eq!(
            call(&b, "vm_snapshot_delete", delete.clone()).await,
            json!({"ok":false,"reason":"not_found"})
        );
        assert_eq!(
            call(&a, "vm_snapshot_delete", delete).await,
            json!({"ok":false,"reason":"static_backup_delete_forbidden"})
        );
        assert_eq!(call(&a, "vm_snapshot_restore", restore).await, restored);
        assert_eq!(phase(&a, &vm).await["phase"], "unpublished");
        assert_eq!((fake.posts.load(SeqCst), fake.deletes.load(SeqCst)), (1, 1));
        let started = call(&a, "vm_start", args(1809)).await;
        assert_eq!(
            terminal_static_start(&a, &started).await["state"],
            "succeeded"
        );
        let republished = call(&a, "publish_site", args(1810)).await;
        assert_eq!(terminal(&a, &republished).await["state"], "succeeded");
        drop(a);
        drop(b);
        h.shutdown().await.unwrap();
    }

    #[test]
    fn only_the_retiring_snapshot_delete_fault_carries_a_retry_hint() {
        assert_eq!(REAPER_RETRY_AFTER_MS, 5000);
        assert_eq!(
            u128::from(REAPER_RETRY_AFTER_MS),
            crate::vm::reaper::PERIOD.as_millis()
        );
        // Same wire reason and audit code as every other cleanup_unconfirmed.
        assert_eq!(
            SNAPSHOT_DELETE_RETIRING.0,
            refusal_wire(Refusal::CleanupUnconfirmed)
        );
        assert_eq!(
            refusal_from_wire(SNAPSHOT_DELETE_RETIRING.0),
            Some(Refusal::CleanupUnconfirmed)
        );
        assert_eq!(
            refusal_value(SNAPSHOT_DELETE_RETIRING),
            json!({"ok":false,"reason":"cleanup_unconfirmed","retryable":true,"retry_after_ms":5000})
        );
        let faults = REFUSAL_CODES.into_iter().map(Fault::from).chain([
            INVALID,
            INVALID_CURSOR,
            SHUTDOWN,
            NOT_FOUND,
            STATE_UNAVAILABLE,
            NOT_READY,
            HOST_EXECUTION_DISABLED,
        ]);
        for fault in faults {
            assert_eq!(fault.1, Retry::Never, "{}", fault.0);
            assert_eq!(
                refusal_value(fault),
                json!({"ok":false,"reason":fault.0}),
                "{}",
                fault.0
            );
        }
    }

    #[tokio::test]
    async fn test_snapshot_delete_after_restore_hints_one_reaper_period() {
        let mut h = Harness::start(false, false).await;
        h.fixture(|fixture| fixture.snapshot_test_media = true);
        let a = h.client(0).await;
        let build = call(
            &a,
            "vm_template_build",
            json!({"recipe_id":hex::encode(RECIPE.0),"request_id":key(1860)}),
        )
        .await;
        h.hold_build.notify_one();
        let template = terminal(&a, &build).await["template_id"].clone();
        let clone = call(
            &a,
            "vm_create",
            json!({"template_id":template,"request_id":key(1861),"ttl_s":60}),
        )
        .await;
        let vm = terminal(&a, &clone).await["vm_id"].clone();
        let created = call(
            &a,
            "vm_snapshot_create",
            json!({"vm_id":vm,"request_id":key(1862)}),
        )
        .await;
        let captured = terminal(&a, &created).await;
        assert_eq!(captured["state"], "succeeded", "{captured}");
        let point = captured["snapshot_id"].clone();
        let start = call(&a, "vm_start", json!({"vm_id":vm,"request_id":key(1863)})).await;
        assert_eq!(terminal(&a, &start).await["state"], "succeeded");
        let stop = call(&a, "vm_stop", json!({"vm_id":vm,"request_id":key(1864)})).await;
        assert_eq!(terminal(&a, &stop).await["state"], "succeeded");
        let restore = call(
            &a,
            "vm_snapshot_restore",
            json!({"vm_id":vm,"snapshot_id":point,"request_id":key(1865)}),
        )
        .await;
        let restored = terminal(&a, &restore).await;
        assert_eq!(restored["state"], "succeeded", "{restored}");
        assert_eq!(restored["cleanup"], "cleanup_unconfirmed");
        let unknown = json!({"vm_id":vm,"snapshot_id":key(1866),"request_id":key(1867)});
        assert_eq!(
            call(&a, "vm_snapshot_delete", unknown).await,
            json!({"ok":false,"reason":"not_found"})
        );
        // Before the reaper sweeps the retired generation, the refusal says
        // when to retry: one production reaper period.
        let delete = json!({"vm_id":vm,"snapshot_id":point,"request_id":key(1868)});
        let refused = call(&a, "vm_snapshot_delete", delete.clone()).await;
        assert_eq!(
            refused,
            json!({"ok":false,"reason":"cleanup_unconfirmed","retryable":true,
                "retry_after_ms":crate::vm::reaper::PERIOD.as_millis() as u64})
        );
        assert_eq!(refused["retry_after_ms"], 5000);
        // The same reason from another tool keeps the two-key shape.
        assert_eq!(
            call(
                &a,
                "vm_snapshot_create",
                json!({"vm_id":vm,"request_id":key(1869)})
            )
            .await,
            json!({"ok":false,"reason":"cleanup_unconfirmed"})
        );
        h.sweep().await;
        // The refusal bound nothing: the same request now deletes the point.
        let deleted = call(&a, "vm_snapshot_delete", delete.clone()).await;
        assert_eq!(terminal(&a, &deleted).await["state"], "succeeded");
        assert_eq!(call(&a, "vm_snapshot_delete", delete).await, deleted);
        drop(a);
        h.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_snapshot_delete_behind_quarantined_copy_has_no_retry_hint() {
        let mut h = Harness::start(false, false).await;
        h.fixture(|fixture| fixture.snapshot_test_media = true);
        let a = h.client(0).await;
        let build = call(
            &a,
            "vm_template_build",
            json!({"recipe_id":hex::encode(RECIPE.0),"request_id":key(1870)}),
        )
        .await;
        h.hold_build.notify_one();
        let template = terminal(&a, &build).await["template_id"].clone();
        let clone = call(
            &a,
            "vm_create",
            json!({"template_id":template,"request_id":key(1871),"ttl_s":600}),
        )
        .await;
        let vm = terminal(&a, &clone).await["vm_id"].clone();
        let vm_id = parse_id(vm.as_str().unwrap()).unwrap();
        let created = call(
            &a,
            "vm_snapshot_create",
            json!({"vm_id":vm,"request_id":key(1872)}),
        )
        .await;
        let captured = terminal(&a, &created).await;
        assert_eq!(captured["state"], "succeeded", "{captured}");
        let point = captured["snapshot_id"].clone();
        // A later copy attempts its disk create without recording an identity:
        // the abandoned leaf is quarantined while the older point stays ready.
        h.fixture(|f| f.snapshot_fault = Some(SnapshotFault::FailAfterDiskAttemptNoId));
        let failed = call(
            &a,
            "vm_snapshot_create",
            json!({"vm_id":vm,"request_id":key(1873)}),
        )
        .await;
        assert_eq!(terminal(&a, &failed).await["state"], "failed");
        h.fixture(|f| f.snapshot_fault = None);
        let delete = json!({"vm_id":vm,"snapshot_id":point,"request_id":key(1874)});
        let held = json!({"ok":false,"reason":"cleanup_unconfirmed"});
        assert_eq!(call(&a, "vm_snapshot_delete", delete.clone()).await, held);
        h.sweep().await;
        h.sweep().await;
        let quarantined = |db: &rusqlite::Connection| {
            crate::store::vm::count(
                db,
                "SELECT count(*) FROM vm_snapshot_work w
                 JOIN vm_artifacts a ON a.generation_id=w.candidate
                 WHERE w.vm=?1 AND w.phase='abandoned' AND w.quarantine_audited=1
                   AND a.state='quarantined'",
                [vm_id],
            )
        };
        let store = h.supervisor().controller.store_for_test();
        assert_eq!(store.transact(|db| quarantined(db)).unwrap(), 1);
        // Sweeps do not clear quarantine, so the refusal never hints a retry.
        assert_eq!(call(&a, "vm_snapshot_delete", delete.clone()).await, held);
        h.sweep().await;
        assert_eq!(store.transact(|db| quarantined(db)).unwrap(), 1);
        assert_eq!(call(&a, "vm_snapshot_delete", delete).await, held);
        drop(a);
        // The held quarantine is exactly what keeps the drain unconfirmed.
        assert_eq!(
            h.shutdown().await.unwrap_err().to_string(),
            "server-drain-unconfirmed"
        );
    }

    #[tokio::test]
    async fn test_snapshot_replay_survives_delete_and_original_lease_expiry() {
        let mut h = Harness::start(false, false).await;
        h.fixture(|fixture| fixture.snapshot_test_media = true);
        let a = h.client(0).await;
        let build = call(
            &a,
            "vm_template_build",
            json!({"recipe_id":hex::encode(RECIPE.0),"request_id":key(1850)}),
        )
        .await;
        h.hold_build.notify_one();
        let template = terminal(&a, &build).await["template_id"].clone();
        let clone = call(
            &a,
            "vm_create",
            json!({"template_id":template,"request_id":key(1851),"ttl_s":60}),
        )
        .await;
        let vm = terminal(&a, &clone).await["vm_id"].clone();
        let capture = json!({"vm_id":vm,"request_id":key(1852)});
        let created = call(&a, "vm_snapshot_create", capture.clone()).await;
        let captured = terminal(&a, &created).await;
        assert_eq!(captured["state"], "succeeded", "{captured}");
        let point = captured["snapshot_id"].clone();
        let start = call(&a, "vm_start", json!({"vm_id":vm,"request_id":key(1855)})).await;
        assert_eq!(terminal(&a, &start).await["state"], "succeeded");
        let original_paths = fixture_pair_paths(&h, &vm, true);
        let saved = simulate_pair_write(&original_paths);
        let stop = call(&a, "vm_stop", json!({"vm_id":vm,"request_id":key(1856)})).await;
        assert_eq!(terminal(&a, &stop).await["state"], "succeeded");
        let restore_args = json!({"vm_id":vm,"snapshot_id":point,"request_id":key(1857)});
        let restore = call(&a, "vm_snapshot_restore", restore_args.clone()).await;
        let restored = terminal(&a, &restore).await;
        assert_eq!(restored["state"], "succeeded", "{restored}");
        assert_eq!(restored["cleanup"], "cleanup_unconfirmed");
        assert_eq!(
            fixture_pair_paths(&h, &vm, true).map(|path| fs::read(path).unwrap()),
            saved
        );
        assert_eq!(fixture_pair_paths(&h, &vm, false), original_paths);
        h.supervisor().sweep().unwrap();
        assert!(original_paths.iter().all(|path| !path.exists()));
        let cold = call(&a, "vm_start", json!({"vm_id":vm,"request_id":key(1858)})).await;
        assert_eq!(terminal(&a, &cold).await["state"], "succeeded");
        assert_eq!(call(&a, "vm_snapshot_restore", restore_args).await, restore);
        let stop = call(&a, "vm_stop", json!({"vm_id":vm,"request_id":key(1859)})).await;
        assert_eq!(terminal(&a, &stop).await["state"], "succeeded");
        let delete = json!({"vm_id":vm,"snapshot_id":point,"request_id":key(1853)});
        let deleted = call(&a, "vm_snapshot_delete", delete.clone()).await;
        assert_eq!(terminal(&a, &deleted).await["state"], "succeeded");
        assert_eq!(
            call(&a, "vm_snapshot_delete", delete.clone()).await,
            deleted
        );
        let surviving = call(
            &a,
            "vm_snapshot_create",
            json!({"vm_id":vm,"request_id":key(1854)}),
        )
        .await;
        assert_eq!(terminal(&a, &surviving).await["state"], "succeeded");
        let deadline = clone["expires_at_ms"].as_i64().unwrap();
        h.now.store(deadline, Ordering::Relaxed);
        // The reaper proves and records each pair separately. Drive the exact
        // two turns synchronously so work can never be skipped by scheduling.
        h.supervisor().sweep().unwrap();
        h.supervisor().sweep().unwrap();
        assert_eq!(
            call(&a, "vm_get", json!({"vm_id":vm})).await["state"],
            "expired"
        );
        assert_eq!(call(&a, "vm_snapshot_delete", delete).await, deleted);
        assert_eq!(call(&a, "vm_snapshot_create", capture).await, created);
        assert_eq!(operation(&a, &created).await["snapshot_id"], point);
        drop(a);
        h.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn snapshot_sdk_validation_cursor_replay_legacy_collision_and_pruning() {
        let mut h = Harness::start_configured(false, false, false, |fixture| {
            fixture.snapshot_test_media = true;
        })
        .await;
        let a = h.client(0).await;
        let b = h.client(1).await;
        let build = call(
            &a,
            "vm_template_build",
            json!({"recipe_id":hex::encode(RECIPE.0),"request_id":key(2100)}),
        )
        .await;
        h.hold_build.notify_one();
        let template = terminal(&a, &build).await["template_id"].clone();
        let vm1 = terminal(
            &a,
            &call(
                &a,
                "vm_create",
                json!({"template_id":template,"request_id":key(2101),"ttl_s":600}),
            )
            .await,
        )
        .await["vm_id"]
            .clone();
        let vm2 = terminal(
            &a,
            &call(
                &a,
                "vm_create",
                json!({"template_id":template,"request_id":key(2102),"ttl_s":600}),
            )
            .await,
        )
        .await["vm_id"]
            .clone();

        let create1 = json!({"vm_id":vm1,"request_id":key(2103),"name":"Alpha-1"});
        let created1 = call(&a, "vm_snapshot_create", create1.clone()).await;
        let snap1 = terminal(&a, &created1).await["snapshot_id"].clone();
        let created2 = call(
            &a,
            "vm_snapshot_create",
            json!({"vm_id":vm1,"request_id":key(2104)}),
        )
        .await;
        let snap2 = terminal(&a, &created2).await["snapshot_id"].clone();

        // Strict input validation across all four snapshot tools.
        for (tool, args, expected) in [
            (
                "vm_snapshot_create",
                json!({"vm_id":vm1,"request_id":key(2110),"extra":1}),
                "invalid",
            ),
            (
                "vm_snapshot_create",
                json!({"vm_id":vm1,"request_id":key(2111),"name":null}),
                "invalid",
            ),
            (
                "vm_snapshot_create",
                json!({"vm_id":vm1,"request_id":key(2112),"name":"-bad"}),
                "invalid",
            ),
            (
                "vm_snapshot_create",
                json!({"vm_id":vm1,"request_id":key(2113),"name":"bad-"}),
                "invalid",
            ),
            (
                "vm_snapshot_create",
                json!({"vm_id":vm1,"request_id":key(2114),"name":"a--b"}),
                "invalid",
            ),
            (
                "vm_snapshot_create",
                json!({"vm_id":vm1,"request_id":key(2115),"name":"a_b"}),
                "invalid",
            ),
            (
                "vm_snapshot_create",
                json!({"vm_id":vm1,"request_id":key(2116),"name":"a".repeat(49)}),
                "invalid",
            ),
            (
                "vm_snapshot_create",
                json!({"vm_id":"ZZ","request_id":key(2117)}),
                "invalid",
            ),
            (
                "vm_snapshot_list",
                json!({"vm_id":vm1,"extra":true}),
                "invalid",
            ),
            (
                "vm_snapshot_list",
                json!({"vm_id":vm1,"limit":null}),
                "invalid",
            ),
            (
                "vm_snapshot_list",
                json!({"vm_id":vm1,"limit":0}),
                "invalid",
            ),
            (
                "vm_snapshot_list",
                json!({"vm_id":vm1,"limit":5}),
                "invalid",
            ),
            (
                "vm_snapshot_list",
                json!({"vm_id":vm1,"cursor":null}),
                "invalid",
            ),
            (
                "vm_snapshot_list",
                json!({"vm_id":vm1,"cursor":"not-a-valid-cursor"}),
                "invalid_cursor",
            ),
            (
                "vm_snapshot_list",
                json!({"vm_id":vm1,"cursor":"a".repeat(97)}),
                "invalid_cursor",
            ),
            (
                "vm_snapshot_restore",
                json!({"vm_id":vm1,"snapshot_id":snap1,"request_id":key(2118),"extra":1}),
                "invalid",
            ),
            (
                "vm_snapshot_restore",
                json!({"vm_id":vm1,"snapshot_id":null,"request_id":key(2119)}),
                "invalid",
            ),
            (
                "vm_snapshot_delete",
                json!({"vm_id":vm1,"snapshot_id":snap1,"request_id":key(2120),"extra":1}),
                "invalid",
            ),
            (
                "vm_snapshot_delete",
                json!({"vm_id":vm1,"snapshot_id":"bad","request_id":key(2121)}),
                "invalid",
            ),
        ] {
            let reply = call(&a, tool, args).await;
            assert_eq!(reply["reason"], expected, "tool={tool} reply={reply}");
        }

        // Cursor isolation across VM, principal and tool.
        let page1 = call(&a, "vm_snapshot_list", json!({"vm_id":vm1,"limit":1})).await;
        assert_eq!(page1["entries"].as_array().unwrap().len(), 1);
        let cursor = page1["next_cursor"].as_str().unwrap().to_owned();
        assert_eq!(
            call(&a, "vm_snapshot_list", json!({"vm_id":vm2,"cursor":cursor})).await["reason"],
            "invalid_cursor"
        );
        assert_eq!(
            call(&b, "vm_snapshot_list", json!({"vm_id":vm1,"cursor":cursor})).await["reason"],
            "invalid_cursor"
        );
        assert_eq!(
            call(&a, "vm_list", json!({"cursor":cursor})).await["reason"],
            "invalid_cursor"
        );
        let page2 = call(&a, "vm_snapshot_list", json!({"vm_id":vm1,"cursor":cursor})).await;
        assert_eq!(page2["entries"].as_array().unwrap().len(), 1);
        assert!(page2["next_cursor"].is_null());

        // Populated legacy action 5/6 keys collide with typed create/restore
        // without promoting legacy vm_snapshots or altering legacy charges.
        let vm1_id = parse_id(vm1.as_str().unwrap()).unwrap();
        let legacy_snap: Id = [0x91; 16];
        let legacy_create_key = key(2150);
        let legacy_restore_key = key(2151);
        let store = h.supervisor().controller.store_for_test();
        let charge_with_legacy = store
            .transact(|db| {
                db.execute(
                    "INSERT INTO vm_snapshots(vm,id,ready,dashboard_created_ms) VALUES(?1,?2,1,?3)",
                    rusqlite::params![vm1_id, legacy_snap, UTC],
                )?;
                let k5 = RequestId::parse(&legacy_create_key).unwrap();
                let op5 = crate::store::vm::Binding {
                    principal: "alpha",
                    key: k5,
                    subject: crate::vm::Subject::Vm(vm1_id),
                    action: Action::Snapshot as i64,
                }
                .record(db, UTC, Some(vm1_id), None)?;
                db.execute(
                    "INSERT INTO vm_disk_claims(activity_id,vm,snapshot,persistent,staging)
                     VALUES(?1,?2,?3,4096,0)",
                    rusqlite::params![op5.receipt.activity_id, vm1_id, legacy_snap],
                )?;
                crate::store::vm::finish(
                    db,
                    op5.receipt.activity_id,
                    Status::Succeeded,
                    UTC,
                    Some(true),
                )?;
                let k6 = RequestId::parse(&legacy_restore_key).unwrap();
                let op6 = crate::store::vm::Binding {
                    principal: "alpha",
                    key: k6,
                    subject: crate::vm::Subject::Vm(vm1_id),
                    action: Action::Restore as i64,
                }
                .record(db, UTC, Some(vm1_id), None)?;
                crate::store::vm::finish(
                    db,
                    op6.receipt.activity_id,
                    Status::Succeeded,
                    UTC,
                    Some(true),
                )?;
                crate::store::vm::disk_bytes(db)
            })
            .unwrap();
        assert_eq!(
            call(&a, "vm_snapshot_list", json!({"vm_id":vm1})).await["entries"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            call(
                &a,
                "vm_snapshot_create",
                json!({"vm_id":vm1,"request_id":legacy_create_key})
            )
            .await["reason"],
            "key_collision"
        );
        assert_eq!(
            call(
                &a,
                "vm_snapshot_restore",
                json!({"vm_id":vm1,"snapshot_id":snap1,"request_id":legacy_restore_key})
            )
            .await["reason"],
            "key_collision"
        );
        assert_eq!(
            store
                .transact(|db| crate::store::vm::disk_bytes(db))
                .unwrap(),
            charge_with_legacy
        );

        // Independent changed VM, action and target collisions on typed keys.
        assert_eq!(
            call(
                &a,
                "vm_snapshot_create",
                json!({"vm_id":vm2,"request_id":key(2103),"name":"alpha-1"})
            )
            .await["reason"],
            "key_collision"
        );
        assert_eq!(
            call(
                &a,
                "vm_snapshot_create",
                json!({"vm_id":vm1,"request_id":key(2103),"name":"other"})
            )
            .await["reason"],
            "key_collision"
        );
        assert_eq!(
            call(
                &a,
                "vm_snapshot_restore",
                json!({"vm_id":vm1,"snapshot_id":snap1,"request_id":key(2103)})
            )
            .await["reason"],
            "key_collision"
        );
        let restored = call(
            &a,
            "vm_snapshot_restore",
            json!({"vm_id":vm1,"snapshot_id":snap1,"request_id":key(2105)}),
        )
        .await;
        assert_eq!(terminal(&a, &restored).await["state"], "succeeded");
        h.sweep().await;
        assert_eq!(
            call(
                &a,
                "vm_snapshot_restore",
                json!({"vm_id":vm1,"snapshot_id":snap2,"request_id":key(2105)})
            )
            .await["reason"],
            "key_collision"
        );
        assert_eq!(
            call(
                &a,
                "vm_snapshot_restore",
                json!({"vm_id":vm2,"snapshot_id":snap1,"request_id":key(2105)})
            )
            .await["reason"],
            "key_collision"
        );
        assert_eq!(
            call(
                &a,
                "vm_snapshot_delete",
                json!({"vm_id":vm1,"snapshot_id":snap1,"request_id":key(2105)})
            )
            .await["reason"],
            "key_collision"
        );

        // Prune >128 audit rows, reopen, and verify accepted replay survives.
        let created1_op = parse_id(created1["operation_id"].as_str().unwrap()).unwrap();
        let created1_activity: i64 = store
            .transact(|db| {
                let activity_id: i64 = db.query_row(
                    "SELECT activity_id FROM vm_operations WHERE operation_id=?1",
                    [created1_op],
                    |row| row.get(0),
                )?;
                for i in 0..135 {
                    db.execute(
                        "INSERT INTO activity(principal,actor,vm,action,started_ms,status,reason,finished_ms,terminal_order)
                         VALUES('alpha','agent',?1,13,?2,4,1,?2,(SELECT coalesce(max(terminal_order),0)+1 FROM activity))",
                        rusqlite::params![vm1_id, UTC + i],
                    )?;
                }
                crate::store::vm::prune(db)?;
                let remaining: i64 = db.query_row(
                    "SELECT count(*) FROM activity WHERE id=?1",
                    [activity_id],
                    |row| row.get(0),
                )?;
                assert_eq!(remaining, 0);
                Ok(activity_id)
            })
            .unwrap();
        assert!(created1_activity > 0);
        drop(a);
        drop(b);
        h.shutdown().await.unwrap();
        let mut h = h.reopen(|fixture| fixture.snapshot_test_media = true).await;
        let a = h.client(0).await;
        assert_eq!(call(&a, "vm_snapshot_create", create1).await, created1);
        let op_after = operation(&a, &created1).await;
        assert_eq!(op_after["state"], "succeeded");
        assert_eq!(op_after["snapshot_id"], snap1);
        assert_eq!(
            call(&a, "vm_snapshot_list", json!({"vm_id":vm1})).await["entries"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        drop(a);
        h.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_windows_and_ubuntu_snapshot_lifecycle_reopen_custody_and_stale_epoch() {
        let mut h = Harness::start_configured(false, false, true, |fixture| {
            fixture.windows_recipe = true;
            fixture.snapshot_test_media = true;
        })
        .await;
        let a = h.client(0).await;
        let ubuntu_build = call(
            &a,
            "vm_template_build",
            json!({"recipe_id":hex::encode(RECIPE.0),"request_id":key(2200)}),
        )
        .await;
        h.hold_build.notify_one();
        let ubuntu_tpl = terminal(&a, &ubuntu_build).await["template_id"].clone();
        let win_build = call(
            &a,
            "vm_template_build",
            json!({"recipe_id":hex::encode(Controller::windows_recipe_id()),"request_id":key(2201)}),
        )
        .await;
        let win_tpl = terminal(&a, &win_build).await["template_id"].clone();
        let web_build = call(
            &a,
            "vm_template_build",
            json!({"recipe_id":hex::encode(WEB_RECIPE_ID),"request_id":key(2202)}),
        )
        .await;
        let web_tpl = terminal(&a, &web_build).await["template_id"].clone();

        let win_clone = call(
            &a,
            "vm_create",
            json!({"template_id":win_tpl,"request_id":key(2203),"ttl_s":600}),
        )
        .await;
        let win_vm = terminal(&a, &win_clone).await["vm_id"].clone();
        let ubu_clone = call(
            &a,
            "vm_create",
            json!({"template_id":ubuntu_tpl,"request_id":key(2204),"ttl_s":600}),
        )
        .await;
        let ubu_vm = terminal(&a, &ubu_clone).await["vm_id"].clone();
        let sib_clone = call(
            &a,
            "vm_create",
            json!({"template_id":ubuntu_tpl,"request_id":key(2205),"ttl_s":600}),
        )
        .await;
        let sib_vm = terminal(&a, &sib_clone).await["vm_id"].clone();
        let stat_vm = terminal(
            &a,
            &call(
                &a,
                "static_vm_create",
                json!({"name":"sib-static","template_id":web_tpl,"request_id":key(2206)}),
            )
            .await,
        )
        .await["vm_id"]
            .clone();

        let golden_paths = |tpl: &Value| -> [std::path::PathBuf; 2] {
            let tpl_id = parse_id(tpl.as_str().unwrap()).unwrap();
            h.supervisor()
                .controller
                .store_for_test()
                .transact(|db| {
                    let mut stmt = db.prepare(
                        "SELECT a.role,a.object_key FROM vm_build_intents b
                         JOIN vm_generations g ON g.build_id=b.id AND g.active=1
                         JOIN vm_artifacts a ON a.generation_id=g.id
                         WHERE b.template_id=?1 ORDER BY a.role",
                    )?;
                    let files = stmt
                        .query_map([tpl_id], |row| {
                            Ok(h.media.join(format!(
                                "{}-{}",
                                row.get::<_, String>(0)?,
                                hex::encode(row.get::<_, Id>(1)?)
                            )))
                        })?
                        .collect::<rusqlite::Result<Vec<_>>>()?;
                    Ok(files.try_into().expect("golden pair"))
                })
                .unwrap()
        };
        let win_golden_paths = golden_paths(&win_tpl);
        let ubu_golden_paths = golden_paths(&ubuntu_tpl);
        let win_golden_bytes = win_golden_paths.each_ref().map(|p| fs::read(p).unwrap());
        let ubu_golden_bytes = ubu_golden_paths.each_ref().map(|p| fs::read(p).unwrap());
        let sib_paths = fixture_pair_paths(&h, &sib_vm, true);
        let sib_bytes = sib_paths.each_ref().map(|p| fs::read(p).unwrap());
        let stat_paths = fixture_pair_paths(&h, &stat_vm, true);
        let stat_bytes = stat_paths.each_ref().map(|p| fs::read(p).unwrap());

        // Paired marker capture -> mutate -> restore -> charged retirement -> cold boot
        // for both Windows and Ubuntu TEST VMs, preserving original leases.
        let mut nonce = 2210u64;
        for (vm, orig_clone) in [(&win_vm, &win_clone), (&ubu_vm, &ubu_clone)] {
            let orig_deadline = orig_clone["expires_at_ms"].as_i64().unwrap();
            let p1 = terminal(
                &a,
                &call(
                    &a,
                    "vm_snapshot_create",
                    json!({"vm_id":vm,"request_id":key(nonce)}),
                )
                .await,
            )
            .await["snapshot_id"]
                .clone();
            nonce += 1;
            assert_eq!(
                terminal(
                    &a,
                    &call(&a, "vm_start", json!({"vm_id":vm,"request_id":key(nonce)})).await,
                )
                .await["state"],
                "succeeded"
            );
            nonce += 1;
            let active_before = fixture_pair_paths(&h, vm, true);
            let saved = simulate_pair_write(&active_before);
            assert_eq!(
                terminal(
                    &a,
                    &call(&a, "vm_stop", json!({"vm_id":vm,"request_id":key(nonce)})).await,
                )
                .await["state"],
                "succeeded"
            );
            nonce += 1;
            let restored = terminal(
                &a,
                &call(
                    &a,
                    "vm_snapshot_restore",
                    json!({"vm_id":vm,"snapshot_id":p1,"request_id":key(nonce)}),
                )
                .await,
            )
            .await;
            nonce += 1;
            assert_eq!(restored["state"], "succeeded", "{restored}");
            assert_eq!(restored["cleanup"], "cleanup_unconfirmed");
            assert_eq!(
                fixture_pair_paths(&h, vm, true).map(|p| fs::read(p).unwrap()),
                saved
            );
            assert_eq!(fixture_pair_paths(&h, vm, false), active_before);
            let charge_retired = h
                .supervisor()
                .controller
                .store_for_test()
                .transact(|db| crate::store::vm::disk_bytes(db))
                .unwrap()
                .unwrap();
            h.sweep().await;
            assert!(active_before.iter().all(|p| !p.exists()));
            let charge_after = h
                .supervisor()
                .controller
                .store_for_test()
                .transact(|db| crate::store::vm::disk_bytes(db))
                .unwrap()
                .unwrap();
            assert!(charge_after < charge_retired);
            assert_eq!(
                call(&a, "vm_get", json!({"vm_id":vm})).await["expires_at_ms"],
                orig_deadline
            );
            assert_eq!(
                terminal(
                    &a,
                    &call(&a, "vm_start", json!({"vm_id":vm,"request_id":key(nonce)})).await,
                )
                .await["state"],
                "succeeded"
            );
            nonce += 1;
            assert_eq!(
                terminal(
                    &a,
                    &call(&a, "vm_stop", json!({"vm_id":vm,"request_id":key(nonce)})).await,
                )
                .await["state"],
                "succeeded"
            );
            nonce += 1;
        }

        // Crash-after-commit before in-memory fixture map update still boots
        // via durable SQLite pair rehydration.
        let p2 = terminal(
            &a,
            &call(
                &a,
                "vm_snapshot_create",
                json!({"vm_id":win_vm,"request_id":key(nonce)}),
            )
            .await,
        )
        .await["snapshot_id"]
            .clone();
        nonce += 1;
        h.fixture(|fixture| fixture.skip_restore_map_update = true);
        assert_eq!(
            terminal(
                &a,
                &call(
                    &a,
                    "vm_snapshot_restore",
                    json!({"vm_id":win_vm,"snapshot_id":p2,"request_id":key(nonce)}),
                )
                .await,
            )
            .await["state"],
            "succeeded"
        );
        nonce += 1;
        h.fixture(|fixture| fixture.skip_restore_map_update = false);
        h.sweep().await;
        assert_eq!(
            terminal(
                &a,
                &call(
                    &a,
                    "vm_start",
                    json!({"vm_id":win_vm,"request_id":key(nonce)}),
                )
                .await,
            )
            .await["state"],
            "succeeded"
        );
        nonce += 1;
        assert_eq!(
            terminal(
                &a,
                &call(
                    &a,
                    "vm_stop",
                    json!({"vm_id":win_vm,"request_id":key(nonce)})
                )
                .await,
            )
            .await["state"],
            "succeeded"
        );
        nonce += 1;

        // Reopen: committed TEST pair rehydrates and verifies on boot;
        // replacing a leaf refuses boot; stale writer epoch fails and rolls back.
        drop(a);
        h.shutdown().await.unwrap();
        let mut h = h
            .reopen(|fixture| {
                fixture.windows_recipe = true;
                fixture.snapshot_test_media = true;
            })
            .await;
        let a = h.client(0).await;
        assert_eq!(
            terminal(
                &a,
                &call(
                    &a,
                    "vm_start",
                    json!({"vm_id":win_vm,"request_id":key(nonce)}),
                )
                .await,
            )
            .await["state"],
            "succeeded"
        );
        nonce += 1;
        assert_eq!(
            terminal(
                &a,
                &call(
                    &a,
                    "vm_stop",
                    json!({"vm_id":win_vm,"request_id":key(nonce)})
                )
                .await,
            )
            .await["state"],
            "succeeded"
        );
        nonce += 1;

        // Tamper one leaf on ubu_vm: boot refuses before start.
        let ubu_active = fixture_pair_paths(&h, &ubu_vm, true);
        let backup_vars = h.media.join("tamper-backup-vars");
        fs::rename(&ubu_active[1], &backup_vars).unwrap();
        fs::copy(&backup_vars, &ubu_active[1]).unwrap();
        let bad_boot = terminal(
            &a,
            &call(
                &a,
                "vm_start",
                json!({"vm_id":ubu_vm,"request_id":key(nonce)}),
            )
            .await,
        )
        .await;
        assert_eq!(bad_boot["state"], "interrupted", "{bad_boot}");
        assert_eq!(bad_boot["reason"], "boot_failed");
        assert_eq!(
            call(&a, "vm_get", json!({"vm_id":ubu_vm})).await["ready"],
            false
        );
        fs::remove_file(&ubu_active[1]).unwrap();
        fs::rename(&backup_vars, &ubu_active[1]).unwrap();
        h.sweep().await;

        // Stale writer epoch on prove_stop rejects and rolls back digest updates.
        let win_id = parse_id(win_vm.as_str().unwrap()).unwrap();
        let store = h.supervisor().controller.store_for_test();
        let mut fake_pair = store
            .transact(|db| {
                Ok(crate::store::vm::snapshot::active(db, win_id, false)?
                    .unwrap()
                    .pair)
            })
            .unwrap();
        let orig_digest = fake_pair.1[0].pin.sha256;
        fake_pair.1[0].pin.sha256 = [0xAA; 32];
        assert_eq!(
            store
                .transact(|db| {
                    // Model the next writer before its stop settles, so only the
                    // epoch check rejects the displaced writer's digest.
                    crate::store::vm::snapshot::invalidate_release(db, win_id, [0xEF; 16])?;
                    crate::store::vm::snapshot::prove_stop(
                        db,
                        win_id,
                        [0xEE; 16],
                        fake_pair,
                        runtime::StopDisposition::CleanRelease,
                    )
                })
                .unwrap_err()
                .to_string(),
            "snapshot stop writer epoch changed"
        );
        let after_digest = store
            .transact(|db| {
                Ok(crate::store::vm::snapshot::active(db, win_id, false)?
                    .unwrap()
                    .pair
                    .1[0]
                    .pin
                    .sha256)
            })
            .unwrap();
        assert_eq!(after_digest, orig_digest);

        // Golden, TEST sibling and static sibling bytes are untouched.
        assert_eq!(
            win_golden_paths.each_ref().map(|p| fs::read(p).unwrap()),
            win_golden_bytes
        );
        assert_eq!(
            ubu_golden_paths.each_ref().map(|p| fs::read(p).unwrap()),
            ubu_golden_bytes
        );
        assert_eq!(
            sib_paths.each_ref().map(|p| fs::read(p).unwrap()),
            sib_bytes
        );
        assert_eq!(
            stat_paths.each_ref().map(|p| fs::read(p).unwrap()),
            stat_bytes
        );
        drop(a);
        h.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn snapshot_copy_delete_failure_graph_reopen_orphan_expiry_and_sql_retry() {
        let mut h = Harness::start_configured(false, false, false, |fixture| {
            fixture.snapshot_test_media = true;
        })
        .await;
        let a = h.client(0).await;
        let build = call(
            &a,
            "vm_template_build",
            json!({"recipe_id":hex::encode(RECIPE.0),"request_id":key(2300)}),
        )
        .await;
        h.hold_build.notify_one();
        let template = terminal(&a, &build).await["template_id"].clone();
        let mut nonce = 2301u64;
        let make_vm = async |client: &Client, ttl: u64, n: u64| {
            let c = call(
                client,
                "vm_create",
                json!({"template_id":template,"request_id":key(n),"ttl_s":ttl}),
            )
            .await;
            let res = terminal(client, &c).await;
            (res["vm_id"].clone(), c["expires_at_ms"].as_i64().unwrap())
        };
        let (before_create_vm, before_deadline) = make_vm(&a, 60, 2370).await;
        let (after_create_vm, after_deadline) = make_vm(&a, 120, 2371).await;
        let (vm_o1, _) = make_vm(&a, 600, 2372).await;
        let (vm1, _) = make_vm(&a, 600, nonce).await;
        nonce += 1;

        // 1. Both untouched + reopen: abandon_inactive must NOT rewrite planned -> quarantined.
        h.fixture(|f| f.snapshot_fault = Some(SnapshotFault::FailBeforeCreate));
        let fail_untouched = terminal(
            &a,
            &call(
                &a,
                "vm_snapshot_create",
                json!({"vm_id":vm1,"request_id":key(nonce)}),
            )
            .await,
        )
        .await;
        nonce += 1;
        assert_eq!(fail_untouched["state"], "failed");
        drop(a);
        h.shutdown().await.unwrap();
        let mut h = h.reopen(|f| f.snapshot_test_media = true).await;
        let a = h.client(0).await;
        let vm1_id = parse_id(vm1.as_str().unwrap()).unwrap();
        h.supervisor()
            .controller
            .store_for_test()
            .transact(|db| {
                assert_eq!(
                    crate::store::vm::count(db, "SELECT count(*) FROM vm_snapshot_work WHERE vm=?1", [vm1_id])?,
                    0
                );
                assert_eq!(
                    crate::store::vm::count(
                        db,
                        "SELECT count(*) FROM vm_snapshot_reconciliation WHERE vm=?1 AND cause='abandoned'",
                        [vm1_id]
                    )?,
                    1
                );
                Ok(())
            })
            .unwrap();

        // 2. Identified disk + untouched VARS + reopen: disposes disk, accepts planned VARS.
        h.fixture(|f| f.snapshot_fault = Some(SnapshotFault::FailAfterDiskIdentified));
        let fail_half = terminal(
            &a,
            &call(
                &a,
                "vm_snapshot_create",
                json!({"vm_id":vm1,"request_id":key(nonce)}),
            )
            .await,
        )
        .await;
        nonce += 1;
        assert_eq!(fail_half["state"], "failed");
        drop(a);
        h.shutdown().await.unwrap();
        let mut h = h.reopen(|f| f.snapshot_test_media = true).await;
        let a = h.client(0).await;
        h.supervisor()
            .controller
            .store_for_test()
            .transact(|db| {
                assert_eq!(
                    crate::store::vm::count(db, "SELECT count(*) FROM vm_snapshot_work WHERE vm=?1", [vm1_id])?,
                    0
                );
                assert_eq!(
                    crate::store::vm::count(
                        db,
                        "SELECT count(*) FROM vm_snapshot_reconciliation WHERE vm=?1 AND cause='abandoned'",
                        [vm1_id]
                    )?,
                    2
                );
                Ok(())
            })
            .unwrap();

        // 3. Fail after flush before commit + reopen: old active pointer stays, no auto-promotion.
        let active_gen_before: Id = h
            .supervisor()
            .controller
            .store_for_test()
            .transact(|db| {
                Ok(db.query_row(
                    "SELECT id FROM vm_generations WHERE vm_id=?1 AND active=1",
                    [vm1_id],
                    |row| row.get(0),
                )?)
            })
            .unwrap();
        h.fixture(|f| f.snapshot_fault = Some(SnapshotFault::FailAfterFlushBeforeCommit));
        let fail_flush = terminal(
            &a,
            &call(
                &a,
                "vm_snapshot_create",
                json!({"vm_id":vm1,"request_id":key(nonce)}),
            )
            .await,
        )
        .await;
        nonce += 1;
        assert_eq!(fail_flush["state"], "failed");
        drop(a);
        h.shutdown().await.unwrap();
        let mut h = h.reopen(|f| f.snapshot_test_media = true).await;
        let a = h.client(0).await;
        h.supervisor()
            .controller
            .store_for_test()
            .transact(|db| {
                let gen_after: Id = db.query_row(
                    "SELECT id FROM vm_generations WHERE vm_id=?1 AND active=1",
                    [vm1_id],
                    |row| row.get(0),
                )?;
                assert_eq!(gen_after, active_gen_before);
                assert_eq!(
                    crate::store::vm::count(
                        db,
                        "SELECT count(*) FROM vm_restore_points WHERE vm=?1",
                        [vm1_id]
                    )?,
                    0
                );
                assert_eq!(
                    crate::store::vm::count(
                        db,
                        "SELECT count(*) FROM vm_snapshot_work WHERE vm=?1",
                        [vm1_id]
                    )?,
                    0
                );
                Ok(())
            })
            .unwrap();

        // 4. Loss of host disk reserve during final recheck/settlement refuses commit.
        let controller = h.supervisor().controller.clone();
        h.fixture(|f| {
            f.host_observer = Some(Arc::new(move || {
                let both_identified = controller.store_for_test()
                    .transact(|db| {
                        crate::store::vm::count(
                            db,
                            "SELECT count(*) FROM vm_artifacts a JOIN vm_snapshot_work w ON w.candidate=a.generation_id WHERE w.vm=?1 AND w.phase='copy' AND a.state='identified'",
                            [vm1_id],
                        )
                    })
                    .unwrap_or(0)
                    >= 2;
                if both_identified {
                    HostObservation {
                        capacity: Some(Capacity {
                            free_bytes: 0,
                            managed_bytes: 0,
                        }),
                        ..HOST
                    }
                } else {
                    HOST
                }
            }));
        });
        let cap_fail = terminal(
            &a,
            &call(
                &a,
                "vm_snapshot_create",
                json!({"vm_id":vm1,"request_id":key(nonce)}),
            )
            .await,
        )
        .await;
        nonce += 1;
        assert_eq!(cap_fail["state"], "failed");
        h.fixture(|f| f.host_observer = None);
        h.sweep().await;
        assert_eq!(
            call(&a, "vm_snapshot_list", json!({"vm_id":vm1})).await["entries"]
                .as_array()
                .unwrap()
                .len(),
            0
        );

        // 5. Reopen mid-TEST-delete (one leaf already removed, other remains).
        let snap_del = terminal(
            &a,
            &call(
                &a,
                "vm_snapshot_create",
                json!({"vm_id":vm1,"request_id":key(nonce)}),
            )
            .await,
        )
        .await["snapshot_id"]
            .clone();
        nonce += 1;
        let snap_del_id = parse_id(snap_del.as_str().unwrap()).unwrap();
        let (del_work, _, _) = h
            .supervisor()
            .controller
            .admit_snapshot(
                "alpha",
                RequestId::parse(&key(nonce)).unwrap(),
                vm1_id,
                SNAPSHOT_DELETE_ACTION,
                &snapshot::Request::Delete(snap_del_id),
                None,
                HOST,
                RESOURCES,
            )
            .unwrap()
            .unwrap();
        nonce += 1;
        // Remove disk leaf manually to simulate crash between leaf removals.
        let del_disk = h.media.join(format!(
            "{}-{}",
            del_work.source.0.disk.role,
            hex::encode(del_work.source.0.disk.key)
        ));
        let del_vars = h.media.join(format!(
            "{}-{}",
            del_work.source.0.vars.role,
            hex::encode(del_work.source.0.vars.key)
        ));
        fs::remove_file(&del_disk).unwrap();
        assert!(del_vars.exists());
        drop(a);
        h.shutdown().await.unwrap();
        let mut h = h.reopen(|f| f.snapshot_test_media = true).await;
        let a = h.client(0).await;
        assert!(!del_vars.exists());
        assert_eq!(
            call(&a, "vm_snapshot_list", json!({"vm_id":vm1})).await["entries"]
                .as_array()
                .unwrap()
                .len(),
            0
        );

        // 6. Held copy + lease expiry cancellation and handoff:
        //    (a) held before create (untouched) and (b) held after both leaves identified.
        for (vm_exp, exp_deadline, hold_before_create, label) in [
            (before_create_vm, before_deadline, true, "before-create"),
            (after_create_vm, after_deadline, false, "after-identified"),
        ] {
            let vm_exp_id = parse_id(vm_exp.as_str().unwrap()).unwrap();
            let entered = Arc::new(Notify::new());
            let release = Arc::new(Notify::new());
            h.fixture(|f| {
                f.snapshot_hold = Some((entered.clone(), release.clone()));
                if hold_before_create {
                    f.snapshot_fault = Some(SnapshotFault::HoldBeforeCreate);
                }
            });
            let op = call(
                &a,
                "vm_snapshot_create",
                json!({"vm_id":vm_exp,"request_id":key(nonce)}),
            )
            .await;
            nonce += 1;
            tokio::time::timeout(Duration::from_secs(2), entered.notified())
                .await
                .unwrap();
            h.now.store(exp_deadline, Ordering::Relaxed);
            // Sweep 1 cancels the scoped snapshot worker, joins it, and settles
            // the candidate (untouched or identified) correlated to expiry.
            h.sweep().await;
            // Sweep 2 disposes the active pair last and writes the tombstone.
            h.sweep().await;
            assert_eq!(
                call(&a, "vm_get", json!({"vm_id":vm_exp})).await["state"],
                "expired",
                "case={label}"
            );
            assert_eq!(operation(&a, &op).await["state"], "interrupted");
            h.supervisor()
                .controller
                .store_for_test()
                .transact(|db| {
                    assert_eq!(
                        crate::store::vm::count(
                            db,
                            "SELECT count(*) FROM vm_snapshot_reconciliation
                             WHERE vm=?1 AND cause='abandoned' AND expiry_activity IS NOT NULL",
                            [vm_exp_id]
                        )?,
                        1
                    );
                    Ok(())
                })
                .unwrap();
        }

        // 7. Orphan live copy worker panic reconciliation + same-process foreground SDK recovery
        //    + shutdown failure preservation.
        let vm_o1_id = parse_id(vm_o1.as_str().unwrap()).unwrap();
        for fault in [
            SnapshotFault::OrphanPanicAfterDiskOnly,
            SnapshotFault::OrphanPanicAfterIdentified,
        ] {
            h.fixture(|f| f.snapshot_fault = Some(fault));
            let op = call(
                &a,
                "vm_snapshot_create",
                json!({"vm_id":vm_o1,"request_id":key(nonce)}),
            )
            .await;
            nonce += 1;
            until(|| !h.fixture(|f| f.live.lock().unwrap().working.contains(&vm_o1_id))).await;
            assert_eq!(operation(&a, &op).await["state"], "pending");
            h.sweep().await;
            let settled = operation(&a, &op).await;
            assert_eq!(settled["state"], "interrupted", "{settled}");
            assert_eq!(settled["cleanup"], "cleanup_unconfirmed");
        }
        h.supervisor()
            .controller
            .store_for_test()
            .transact(|db| {
                assert_eq!(
                    crate::store::vm::count(
                        db,
                        "SELECT count(*) FROM vm_snapshot_reconciliation WHERE vm=?1 AND cause='orphan_copy'",
                        [vm_o1_id]
                    )?,
                    2
                );
                Ok(())
            })
            .unwrap();
        // Same-process foreground SDK recovers once the reaper reconciled the orphan intent.
        let recovered = terminal(
            &a,
            &call(
                &a,
                "vm_snapshot_create",
                json!({"vm_id":vm_o1,"request_id":key(nonce)}),
            )
            .await,
        )
        .await;
        assert_eq!(recovered["state"], "succeeded", "{recovered}");
        drop(a);
        // Shutdown still preserves failure evidence from the panicked snapshot worker.
        assert!(h.shutdown().await.is_err());

        // 8. Quarantine cases (attempted-create without recorded identity, both
        //    disk-only and vars-after-disk, plus staging file rename/replacement):
        //    keep charges quarantined, never delete replaced identities, and emit
        //    ONE correlated daemon quarantine receipt without duplicates across sweeps.
        let mut h2 = Harness::start_configured(false, false, false, |fixture| {
            fixture.snapshot_test_media = true;
        })
        .await;
        let a2 = h2.client(0).await;
        let b2 = call(
            &a2,
            "vm_template_build",
            json!({"recipe_id":hex::encode(RECIPE.0),"request_id":key(2350)}),
        )
        .await;
        h2.hold_build.notify_one();
        let tpl2 = terminal(&a2, &b2).await["template_id"].clone();
        let vm_q1 = terminal(
            &a2,
            &call(
                &a2,
                "vm_create",
                json!({"template_id":tpl2,"request_id":key(2351),"ttl_s":600}),
            )
            .await,
        )
        .await["vm_id"]
            .clone();
        let vm_q1_id = parse_id(vm_q1.as_str().unwrap()).unwrap();
        let vm_q2 = terminal(
            &a2,
            &call(
                &a2,
                "vm_create",
                json!({"template_id":tpl2,"request_id":key(2352),"ttl_s":600}),
            )
            .await,
        )
        .await["vm_id"]
            .clone();
        let vm_q2_id = parse_id(vm_q2.as_str().unwrap()).unwrap();
        let vm_rep = terminal(
            &a2,
            &call(
                &a2,
                "vm_create",
                json!({"template_id":tpl2,"request_id":key(2353),"ttl_s":600}),
            )
            .await,
        )
        .await["vm_id"]
            .clone();
        let vm_rep_id = parse_id(vm_rep.as_str().unwrap()).unwrap();

        h2.fixture(|f| f.snapshot_fault = Some(SnapshotFault::FailAfterDiskAttemptNoId));
        assert_eq!(
            terminal(
                &a2,
                &call(
                    &a2,
                    "vm_snapshot_create",
                    json!({"vm_id":vm_q1,"request_id":key(2354)}),
                )
                .await,
            )
            .await["state"],
            "failed"
        );
        h2.fixture(|f| f.snapshot_fault = Some(SnapshotFault::FailAfterVarsAttemptNoId));
        assert_eq!(
            terminal(
                &a2,
                &call(
                    &a2,
                    "vm_snapshot_create",
                    json!({"vm_id":vm_q2,"request_id":key(2355)}),
                )
                .await,
            )
            .await["state"],
            "failed"
        );
        h2.fixture(|f| f.snapshot_fault = Some(SnapshotFault::FailAfterDiskIdentified));
        assert_eq!(
            terminal(
                &a2,
                &call(
                    &a2,
                    "vm_snapshot_create",
                    json!({"vm_id":vm_rep,"request_id":key(2356)}),
                )
                .await,
            )
            .await["state"],
            "failed"
        );
        let staged_disk_path = h2
            .supervisor()
            .controller
            .store_for_test()
            .transact(|db| {
                let (role, obj): (String, Id) = db.query_row(
                    "SELECT a.role,a.object_key FROM vm_snapshot_work w
                     JOIN vm_artifacts a ON a.generation_id=w.candidate
                     WHERE w.vm=?1 AND a.role='disk'",
                    [vm_rep_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )?;
                Ok(h2.media.join(format!("{role}-{}", hex::encode(obj))))
            })
            .unwrap();
        let moved_orig = h2.media.join("moved-staged-disk");
        let staged_bytes = fs::read(&staged_disk_path).unwrap();
        fs::rename(&staged_disk_path, &moved_orig).unwrap();
        fs::write(&staged_disk_path, &staged_bytes).unwrap();

        h2.sweep().await;
        h2.sweep().await;
        assert!(staged_disk_path.exists());
        h2.supervisor()
            .controller
            .store_for_test()
            .transact(|db| {
                for vm_id in [vm_q1_id, vm_q2_id] {
                    assert_eq!(
                        crate::store::vm::count(
                            db,
                            "SELECT count(*) FROM vm_snapshot_work WHERE vm=?1 AND quarantine_audited=1",
                            [vm_id]
                        )?,
                        1
                    );
                    assert_eq!(
                        crate::store::vm::count(
                            db,
                            "SELECT count(*) FROM vm_snapshot_reconciliation r
                             JOIN activity a ON a.id=r.activity_id
                             WHERE r.vm=?1 AND r.cause='quarantined' AND a.status=2 AND a.cleanup_confirmed=0",
                            [vm_id]
                        )?,
                        1
                    );
                }
                assert_eq!(
                    crate::store::vm::count(
                        db,
                        "SELECT count(*) FROM vm_snapshot_work WHERE vm=?1",
                        [vm_rep_id]
                    )?,
                    1
                );
                Ok(())
            })
            .unwrap();
        drop(a2);
        let _ = h2.shutdown().await;
    }

    #[tokio::test]
    async fn static_snapshot_protection_caps_dirty_displacement_and_negative_trust() {
        let mut h = Harness::start_configured(false, false, true, |fixture| {
            fixture.snapshot_test_media = true;
        })
        .await;
        let a = h.client(0).await;
        let web_build = call(
            &a,
            "vm_template_build",
            json!({"recipe_id":hex::encode(WEB_RECIPE_ID),"request_id":key(2400)}),
        )
        .await;
        h.hold_build.notify_one();
        let web_tpl = terminal(&a, &web_build).await["template_id"].clone();
        let ubu_tpl = terminal(
            &a,
            &call(
                &a,
                "vm_template_build",
                json!({"recipe_id":hex::encode(RECIPE.0),"request_id":key(2401)}),
            )
            .await,
        )
        .await["template_id"]
            .clone();

        let mut nonce = 2402u64;
        let make_static = |name: &str, n: u64| {
            let client = &a;
            let tpl = web_tpl.clone();
            let name = name.to_owned();
            async move {
                terminal(
                    client,
                    &call(
                        client,
                        "static_vm_create",
                        json!({"name":name,"template_id":tpl,"request_id":key(n)}),
                    )
                    .await,
                )
                .await["vm_id"]
                    .clone()
            }
        };
        let stat1 = make_static("cap-site-1", nonce).await;
        nonce += 1;
        let stat2 = make_static("cap-site-2", nonce).await;
        nonce += 1;
        let stat3 = make_static("cap-site-3", nonce).await;
        nonce += 1;

        // 1. Static per-VM cap: 3 manual points + 1 reserved 4th slot for restore displacement.
        let mut s1_points = Vec::new();
        for _ in 0..3 {
            let r = terminal(
                &a,
                &call(
                    &a,
                    "vm_snapshot_create",
                    json!({"vm_id":stat1,"request_id":key(nonce)}),
                )
                .await,
            )
            .await;
            nonce += 1;
            assert_eq!(r["state"], "succeeded");
            s1_points.push(r["snapshot_id"].clone());
        }
        assert_eq!(
            call(
                &a,
                "vm_snapshot_create",
                json!({"vm_id":stat1,"request_id":key(nonce)})
            )
            .await["reason"],
            "snapshot_limit"
        );
        nonce += 1;
        assert_eq!(
            terminal(
                &a,
                &call(
                    &a,
                    "vm_snapshot_restore",
                    json!({"vm_id":stat1,"snapshot_id":s1_points[0],"request_id":key(nonce)}),
                )
                .await,
            )
            .await["state"],
            "succeeded"
        );
        nonce += 1;
        assert_eq!(
            call(&a, "vm_snapshot_list", json!({"vm_id":stat1})).await["entries"]
                .as_array()
                .unwrap()
                .len(),
            4
        );
        assert_eq!(
            call(
                &a,
                "vm_snapshot_restore",
                json!({"vm_id":stat1,"snapshot_id":s1_points[1],"request_id":key(nonce)})
            )
            .await["reason"],
            "snapshot_limit"
        );
        nonce += 1;

        // 2. Dirty stop refuses capture; after static recovery, restore displaces
        //    the dirty active pair into protected, restore-ineligible dirty_release.
        let stat2_id = parse_id(stat2.as_str().unwrap()).unwrap();
        let orig2: Id = h
            .supervisor()
            .controller
            .store_for_test()
            .transact(|db| {
                Ok(db.query_row(
                    "SELECT origin_generation FROM vm_static_instances WHERE id=?1",
                    [stat2_id],
                    |row| row.get(0),
                )?)
            })
            .unwrap();
        let c1 = terminal(
            &a,
            &call(
                &a,
                "vm_snapshot_create",
                json!({"vm_id":stat2,"request_id":key(nonce)}),
            )
            .await,
        )
        .await["snapshot_id"]
            .clone();
        nonce += 1;
        assert_eq!(
            terminal_static_start(
                &a,
                &call(
                    &a,
                    "vm_start",
                    json!({"vm_id":stat2,"request_id":key(nonce)})
                )
                .await,
            )
            .await["state"],
            "succeeded"
        );
        nonce += 1;
        h.fixture(|f| {
            f.live.lock().unwrap().dirty_stop.insert(stat2_id);
        });
        let dirty_stop = terminal(
            &a,
            &call(
                &a,
                "vm_stop",
                json!({"vm_id":stat2,"request_id":key(nonce)}),
            )
            .await,
        )
        .await;
        nonce += 1;
        assert_eq!(dirty_stop["state"], "failed");
        assert_eq!(dirty_stop["reason"], "dirty_stop");
        // Recover static VM from Recovering -> Stopped while preserving dirty disposition.
        h.sweep().await;
        assert_eq!(
            call(
                &a,
                "vm_snapshot_create",
                json!({"vm_id":stat2,"request_id":key(nonce)})
            )
            .await["reason"],
            "snapshot_offline_required"
        );
        nonce += 1;
        assert_eq!(
            terminal(
                &a,
                &call(
                    &a,
                    "vm_snapshot_restore",
                    json!({"vm_id":stat2,"snapshot_id":c1,"request_id":key(nonce)}),
                )
                .await,
            )
            .await["state"],
            "succeeded"
        );
        nonce += 1;
        let list2 = call(&a, "vm_snapshot_list", json!({"vm_id":stat2})).await;
        let entries2 = list2["entries"].as_array().unwrap();
        assert_eq!(entries2.len(), 2);
        let dirty_entry = entries2
            .iter()
            .find(|e| e["restore_eligible"] == false)
            .expect("protected dirty_release point");
        assert_eq!(dirty_entry["protected"], true);
        let dirty_id = dirty_entry["snapshot_id"].clone();
        assert_eq!(
            call(
                &a,
                "vm_snapshot_restore",
                json!({"vm_id":stat2,"snapshot_id":dirty_id,"request_id":key(nonce)})
            )
            .await["reason"],
            "not_found"
        );
        nonce += 1;
        for target in [&c1, &dirty_id] {
            assert_eq!(
                call(
                    &a,
                    "vm_snapshot_delete",
                    json!({"vm_id":stat2,"snapshot_id":target,"request_id":key(nonce)})
                )
                .await["reason"],
                "static_backup_delete_forbidden"
            );
            nonce += 1;
        }
        h.supervisor()
            .controller
            .store_for_test()
            .transact(|db| {
                assert!(
                    db.execute(
                        "DELETE FROM vm_restore_points WHERE static_vm=?1",
                        [stat2_id]
                    )
                    .is_err()
                );
                let (active_gen, origin_gen): (Id, Id) = db.query_row(
                    "SELECT g.id,s.origin_generation FROM vm_static_instances s
                     JOIN vm_generations g ON g.static_id=s.id AND g.active=1 WHERE s.id=?1",
                    [stat2_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )?;
                assert_eq!(origin_gen, orig2);
                assert_ne!(active_gen, orig2);
                Ok(())
            })
            .unwrap();

        // 3. Same-process negative TLS trust (expired cert marker 'E') under unchanged host trust.
        assert_eq!(
            terminal_static_start(
                &a,
                &call(
                    &a,
                    "vm_start",
                    json!({"vm_id":stat2,"request_id":key(nonce)})
                )
                .await,
            )
            .await["state"],
            "succeeded"
        );
        nonce += 1;
        let boot_origin = h.fixture(|f| f.publish_origin(stat2_id)).unwrap();
        assert_eq!(boot_origin.origin_generation, orig2);
        assert_ne!(boot_origin.generation, orig2);
        {
            use std::io::{Seek, SeekFrom, Write};
            let paths = fixture_pair_paths(&h, &stat2, true);
            let mut file = fs::OpenOptions::new().write(true).open(&paths[0]).unwrap();
            file.seek(SeekFrom::Start(FIXTURE_CERT_MARKER_OFFSET))
                .unwrap();
            file.write_all(b"E").unwrap();
            file.sync_all().unwrap();
        }
        assert_eq!(
            terminal(
                &a,
                &call(
                    &a,
                    "vm_stop",
                    json!({"vm_id":stat2,"request_id":key(nonce)})
                )
                .await,
            )
            .await["state"],
            "succeeded"
        );
        nonce += 1;
        assert_eq!(
            terminal(
                &a,
                &call(
                    &a,
                    "vm_snapshot_create",
                    json!({"vm_id":stat2,"request_id":key(nonce)}),
                )
                .await,
            )
            .await["state"],
            "succeeded"
        );
        nonce += 1;
        let expired_boot = terminal(
            &a,
            &call(
                &a,
                "vm_start",
                json!({"vm_id":stat2,"request_id":key(nonce)}),
            )
            .await,
        )
        .await;
        nonce += 1;
        assert_eq!(expired_boot["state"], "interrupted");
        assert_eq!(expired_boot["reason"], "boot_failed");
        h.sweep().await;

        // 4. Prepare stat3 with untrusted CA marker 'U' (2 points: clean + untrusted).
        assert_eq!(
            terminal(
                &a,
                &call(
                    &a,
                    "vm_snapshot_create",
                    json!({"vm_id":stat3,"request_id":key(nonce)}),
                )
                .await,
            )
            .await["state"],
            "succeeded"
        );
        nonce += 1;
        assert_eq!(
            terminal_static_start(
                &a,
                &call(
                    &a,
                    "vm_start",
                    json!({"vm_id":stat3,"request_id":key(nonce)})
                )
                .await,
            )
            .await["state"],
            "succeeded"
        );
        nonce += 1;
        {
            use std::io::{Seek, SeekFrom, Write};
            let paths = fixture_pair_paths(&h, &stat3, true);
            let mut file = fs::OpenOptions::new().write(true).open(&paths[0]).unwrap();
            file.seek(SeekFrom::Start(FIXTURE_CERT_MARKER_OFFSET))
                .unwrap();
            file.write_all(b"U").unwrap();
            file.sync_all().unwrap();
        }
        assert_eq!(
            terminal(
                &a,
                &call(
                    &a,
                    "vm_stop",
                    json!({"vm_id":stat3,"request_id":key(nonce)})
                )
                .await,
            )
            .await["state"],
            "succeeded"
        );
        nonce += 1;
        let p_untrusted = terminal(
            &a,
            &call(
                &a,
                "vm_snapshot_create",
                json!({"vm_id":stat3,"request_id":key(nonce)}),
            )
            .await,
        )
        .await["snapshot_id"]
            .clone();
        nonce += 1;

        // 5. Shared 16-point host cap across static (4+3+2=9) and TEST (4+3=7) VMs.
        let t1 = terminal(
            &a,
            &call(
                &a,
                "vm_create",
                json!({"template_id":ubu_tpl,"request_id":key(nonce),"ttl_s":600}),
            )
            .await,
        )
        .await["vm_id"]
            .clone();
        nonce += 1;
        let t2 = terminal(
            &a,
            &call(
                &a,
                "vm_create",
                json!({"template_id":ubu_tpl,"request_id":key(nonce),"ttl_s":600}),
            )
            .await,
        )
        .await["vm_id"]
            .clone();
        nonce += 1;
        for (vm, count) in [(&t1, 4usize), (&t2, 3usize)] {
            for _ in 0..count {
                assert_eq!(
                    terminal(
                        &a,
                        &call(
                            &a,
                            "vm_snapshot_create",
                            json!({"vm_id":vm,"request_id":key(nonce)}),
                        )
                        .await,
                    )
                    .await["state"],
                    "succeeded"
                );
                nonce += 1;
            }
        }
        // Host now holds exactly 16 points (4+3+2+4+3).
        assert_eq!(
            call(
                &a,
                "vm_snapshot_create",
                json!({"vm_id":t2,"request_id":key(nonce)})
            )
            .await["reason"],
            "snapshot_limit"
        );
        nonce += 1;
        assert_eq!(
            call(
                &a,
                "vm_snapshot_create",
                json!({"vm_id":stat3,"request_id":key(nonce)})
            )
            .await["reason"],
            "snapshot_limit"
        );
        nonce += 1;
        assert_eq!(
            call(
                &a,
                "vm_snapshot_restore",
                json!({"vm_id":stat3,"snapshot_id":p_untrusted,"request_id":key(nonce)})
            )
            .await["reason"],
            "snapshot_limit"
        );
        nonce += 1;

        // 6. Reopen retaining the exact same host-side FixtureOrigin trust root
        //    and assert untrusted guest cert ('U') is refused across restart.
        drop(a);
        h.shutdown().await.unwrap();
        let mut h = h.reopen(|f| f.snapshot_test_media = true).await;
        let a = h.client(0).await;
        let untrusted_boot = terminal(
            &a,
            &call(
                &a,
                "vm_start",
                json!({"vm_id":stat3,"request_id":key(nonce)}),
            )
            .await,
        )
        .await;
        assert_eq!(untrusted_boot["state"], "interrupted");
        assert_eq!(untrusted_boot["reason"], "boot_failed");
        h.sweep().await;
        drop(a);
        h.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn b2_snapshot_barrier_races_busy_unpublish_pending_removal_and_old_origin() {
        use std::sync::atomic::Ordering::SeqCst;
        let mut h = Harness::start_with_web(false, false, true).await;
        let a = h.client(0).await;
        let (vm, fake) = enrolled(&h, &a, 2500).await;
        let vm_id = parse_id(vm.as_str().unwrap()).unwrap();
        let args = |n| json!({"vm_id":vm,"request_id":key(n)});

        // Create a clean baseline snapshot first.
        assert_eq!(
            terminal(&a, &call(&a, "vm_stop", args(2505)).await).await["state"],
            "succeeded"
        );
        let p1 =
            terminal(&a, &call(&a, "vm_snapshot_create", args(2506)).await).await["snapshot_id"]
                .clone();
        let started = terminal_static_start(&a, &call(&a, "vm_start", args(2507)).await).await;
        assert_settled_static_start(&h, &vm, &started);

        // 1. Publish-first: held DNS POST blocks snapshot create and restore.
        let (entered, in_post) = tokio::sync::oneshot::channel();
        let (release, hold) = tokio::sync::oneshot::channel();
        *fake.post_gate.lock().unwrap() = Some((entered, hold));
        let pub1 = call(&a, "publish_site", args(2508)).await;
        tokio::time::timeout(Duration::from_secs(2), in_post)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            call(&a, "vm_snapshot_create", args(2509)).await["reason"],
            "snapshot_publish_not_quiesced"
        );
        assert_eq!(
            call(
                &a,
                "vm_snapshot_restore",
                json!({"vm_id":vm,"snapshot_id":p1,"request_id":key(2510)})
            )
            .await["reason"],
            "snapshot_publish_not_quiesced"
        );
        release.send(()).unwrap();
        assert_eq!(terminal(&a, &pub1).await["state"], "succeeded");
        // Colliding-key owner unpublish still triggers early local ingress revocation.
        assert_eq!(
            call(&a, "unpublish_site", args(2508)).await["reason"],
            "key_collision"
        );
        until(|| !fake.local.load(SeqCst)).await;
        let unpub1 = call(&a, "unpublish_site", args(2511)).await;
        assert_eq!(terminal(&a, &unpub1).await["state"], "succeeded");
        assert_eq!(call(&a, "unpublish_site", args(2511)).await, unpub1);
        assert_eq!(
            terminal(&a, &call(&a, "vm_stop", args(2512)).await).await["state"],
            "succeeded"
        );

        // 2. Restore-first: held snapshot copy holds effects slot; publish and fresh
        //    same-VM unpublish (with no durable intent) both refuse Busy with no action 16,
        //    and restore commits cleanly once released.
        let snap_entered = Arc::new(Notify::new());
        let snap_release = Arc::new(Notify::new());
        h.fixture(|f| f.snapshot_hold = Some((snap_entered.clone(), snap_release.clone())));
        let restoring = call(
            &a,
            "vm_snapshot_restore",
            json!({"vm_id":vm,"snapshot_id":p1,"request_id":key(2513)}),
        )
        .await;
        tokio::time::timeout(Duration::from_secs(2), snap_entered.notified())
            .await
            .unwrap();
        assert_eq!(call(&a, "publish_site", args(2514)).await["reason"], "busy");
        assert_eq!(
            call(&a, "unpublish_site", args(2515)).await["reason"],
            "busy"
        );
        assert_eq!(
            h.supervisor()
                .controller
                .store_for_test()
                .transact(|db| crate::store::vm::count(
                    db,
                    "SELECT count(*) FROM activity WHERE vm=?1 AND status=0 AND action=16",
                    [vm_id]
                ))
                .unwrap(),
            0
        );
        snap_release.notify_one();
        assert_eq!(terminal(&a, &restoring).await["state"], "succeeded");

        // 3. Pending DNS removal across restart still fences snapshots until settled.
        assert_eq!(
            terminal_static_start(&a, &call(&a, "vm_start", args(2516)).await).await["state"],
            "succeeded"
        );
        assert_eq!(
            terminal(&a, &call(&a, "publish_site", args(2517)).await).await["state"],
            "succeeded"
        );
        fake.credential.store(false, SeqCst);
        let unpub_fail = call(&a, "unpublish_site", args(2518)).await;
        assert_eq!(terminal(&a, &unpub_fail).await["state"], "failed");
        assert_eq!(phase(&a, &vm).await["phase"], "cleanup_pending");
        assert_eq!(
            terminal(&a, &call(&a, "vm_stop", args(2519)).await).await["state"],
            "succeeded"
        );
        drop(a);
        assert_eq!(
            h.shutdown().await.unwrap_err().to_string(),
            "server-drain-unconfirmed"
        );
        let mut h = h.reopen(|_| {}).await;
        enroll(
            &h,
            &fake,
            true,
            Profile::synthetic(h.supervisor().host_id, "tunnel-site"),
        );
        let a = h.client(0).await;
        assert_eq!(
            call(&a, "vm_snapshot_create", args(2520)).await["reason"],
            "snapshot_publish_not_quiesced"
        );
        assert_eq!(
            call(
                &a,
                "vm_snapshot_restore",
                json!({"vm_id":vm,"snapshot_id":p1,"request_id":key(2521)})
            )
            .await["reason"],
            "snapshot_publish_not_quiesced"
        );
        fake.credential.store(true, SeqCst);
        settle(&h, &a, &vm).await;
        let old_gen: Id = h
            .supervisor()
            .controller
            .store_for_test()
            .transact(|db| {
                Ok(db.query_row(
                    "SELECT id FROM vm_generations WHERE static_id=?1 AND active=1",
                    [vm_id],
                    |row| row.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(
            terminal(
                &a,
                &call(
                    &a,
                    "vm_snapshot_restore",
                    json!({"vm_id":vm,"snapshot_id":p1,"request_id":key(2522)}),
                )
                .await,
            )
            .await["state"],
            "succeeded"
        );
        assert_eq!(
            terminal_static_start(&a, &call(&a, "vm_start", args(2523)).await).await["state"],
            "succeeded"
        );

        // 4. Publish intent carrying the displaced old generation is rejected.
        let mut stale_origin = h.fixture(|f| f.publish_origin(vm_id)).unwrap();
        assert_ne!(stale_origin.generation, old_gen);
        stale_origin.generation = old_gen;
        let profile = Profile::synthetic(h.supervisor().host_id, "tunnel-site");
        let stale_intent = profile.intent(vm_id, &stale_origin).unwrap();
        let stale_name = crate::vm::static_web::StaticName::parse("tunnel-site").unwrap();
        assert!(matches!(
            h.supervisor()
                .controller
                .admit_publication(
                    "alpha",
                    RequestId::parse(&key(2524)).unwrap(),
                    vm_id,
                    Some((&stale_intent, &stale_name)),
                )
                .unwrap(),
            Err(Refusal::NotReady)
        ));
        drop(a);
        h.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn stale_copy_cleanup_preserves_committed_static_point_and_active_media() {
        let mut h =
            Harness::start_configured(false, false, true, |f| f.snapshot_test_media = true).await;
        let a = h.client(0).await;
        let build = call(
            &a,
            "vm_template_build",
            json!({"recipe_id":hex::encode(WEB_RECIPE_ID),"request_id":key(2700)}),
        )
        .await;
        h.hold_build.notify_one();
        let template = terminal(&a, &build).await["template_id"].clone();
        let vm = terminal(
            &a,
            &call(
                &a,
                "static_vm_create",
                json!({
                    "template_id":template,"name":"stale-cleanup","request_id":key(2701)
                }),
            )
            .await,
        )
        .await["vm_id"]
            .clone();
        let vm_id = parse_id(vm.as_str().unwrap()).unwrap();
        let mut point = Value::Null;
        for restore in [false, true] {
            let entered = Arc::new(Notify::new());
            let resume = Arc::new(Notify::new());
            h.fixture(|f| f.snapshot_hold = Some((entered.clone(), resume.clone())));
            let op = if restore {
                call(
                    &a,
                    "vm_snapshot_restore",
                    json!({"vm_id":vm,"snapshot_id":point,"request_id":key(2703)}),
                )
                .await
            } else {
                call(
                    &a,
                    "vm_snapshot_create",
                    json!({"vm_id":vm,"request_id":key(2702)}),
                )
                .await
            };
            assert_eq!(op["ok"], true, "{op}");
            tokio::time::timeout(Duration::from_secs(2), entered.notified())
                .await
                .unwrap();

            // Both candidate identities exist. Suspend quiescence after this
            // sweep collected them, then publish those exact objects.
            let reaper = h.fixture(|f| f.reaper());
            let mut stall = Stall::install(&reaper.after_selection);
            let ctl = h.supervisor().controller.clone();
            let sweep =
                tokio::task::spawn_blocking(move || ctl.sweep_with(&reaper, &*reaper.custody));
            stall.entered().await;
            let candidate: Id = h
                .supervisor()
                .controller
                .store_for_test()
                .transact(|db| {
                    Ok(db.query_row(
                        "SELECT candidate FROM vm_snapshot_work WHERE static_vm=?1",
                        [vm_id],
                        |row| row.get(0),
                    )?)
                })
                .unwrap();
            resume.notify_one();
            let result = terminal(&a, &op).await;
            assert_eq!(result["state"], "succeeded", "{result}");
            if !restore {
                point = result["snapshot_id"].clone();
            }
            let paths = h
                .supervisor()
                .controller
                .store_for_test()
                .transact(|db| {
                    let published: bool = db.query_row(
                        "SELECT active FROM vm_generations WHERE id=?1 AND static_id=?2",
                        rusqlite::params![candidate, vm_id],
                        |row| row.get(0),
                    )?;
                    assert_eq!(published, restore);
                    if !restore {
                        let ready = crate::store::vm::count(
                            db,
                            "SELECT count(*) FROM vm_restore_points
                             WHERE static_vm=?1 AND generation=?2 AND state='ready'",
                            rusqlite::params![vm_id, candidate],
                        )?;
                        assert_eq!(ready, 1);
                    }
                    let mut stmt = db.prepare(
                    "SELECT role,object_key FROM vm_artifacts WHERE generation_id=?1 ORDER BY role",
                )?;
                    let paths = stmt.query_map([candidate], |row| {
                        let role: String = row.get(0)?;
                        let key: Id = row.get(1)?;
                        Ok(h.media.join(format!("{role}-{}", hex::encode(key))))
                    })?;
                    Ok(paths.collect::<rusqlite::Result<Vec<_>>>()?)
                })
                .unwrap();
            assert_eq!(paths.len(), 2);
            let bytes = paths
                .iter()
                .map(fs::read)
                .collect::<std::io::Result<Vec<_>>>()
                .unwrap();
            stall.release_and_join().await;
            assert!(sweep.await.unwrap().unwrap());
            for (path, original) in paths.iter().zip(bytes) {
                assert!(
                    fs::read(path).is_ok_and(|bytes| bytes == original),
                    "stale reaper removed committed media"
                );
            }
            h.sweep().await;
        }
        drop(a);
        h.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn stalled_boot_join_keeps_later_expiry_turns_unconfirmed() {
        use crate::vm::{reaper::CleanupKind, runtime::ReaperRuntime};
        for force in [false, true] {
            let root = crate::service::application::tests::private_temp();
            let fixture = Fixture::new(root.path(), Default::default());
            let vm = [7; 16];
            let mut boot = Boot::start(&Shutdown::default()).await.unwrap();
            // A running blocking task cannot be aborted. Hold this join across
            // the stop deadline, even on the forced path.
            let (entered, started) = tokio::sync::oneshot::channel();
            let (release, hold) = std::sync::mpsc::channel();
            let (finished, joined) = tokio::sync::oneshot::channel();
            let original = std::mem::replace(
                &mut boot.guest,
                tokio::task::spawn_blocking(move || {
                    entered.send(()).unwrap();
                    let _ = hold.recv();
                    let _ = finished.send(());
                    Ok(())
                }),
            );
            started.await.unwrap();
            fixture
                .live
                .lock()
                .unwrap()
                .boots
                .insert(vm, Slot::Idle(Box::new(boot)));
            let reaper = fixture.reaper();
            tokio::task::spawn_blocking(move || {
                let work = CleanupWork {
                    kind: CleanupKind::Expiry { vm, activity_id: 1 },
                    pending_worker: None,
                    snapshot_phase: None,
                    artifacts: Vec::new(),
                    accounted: false,
                    release_only: true,
                };
                let deadline = Instant::now() + Duration::from_millis(20);
                let result = if force {
                    reaper.force_reap(&work, deadline)
                } else {
                    reaper.graceful_stop(&work, deadline)
                };
                assert_eq!(result, runtime::StopDisposition::ChargedQuarantine);
                let later = Instant::now() + Duration::from_secs(1);
                assert!(matches!(
                    reaper.quiesce(&work, later),
                    runtime::Quiesced::Unconfirmed
                ));
                assert_eq!(
                    reaper.force_reap(&work, later),
                    runtime::StopDisposition::ChargedQuarantine
                );
            })
            .await
            .unwrap();
            release.send(()).unwrap();
            joined.await.unwrap();
            original.await.unwrap().unwrap();
        }
    }

    #[tokio::test]
    async fn running_test_expiry_force_reaps_without_disposing_media_then_reconciles_pairs_active_last()
     {
        let mut h = Harness::start_configured(false, false, true, |fixture| {
            fixture.snapshot_test_media = true;
        })
        .await;
        let a = h.client(0).await;
        let ubu_build = call(
            &a,
            "vm_template_build",
            json!({"recipe_id":hex::encode(RECIPE.0),"request_id":key(2600)}),
        )
        .await;
        h.hold_build.notify_one();
        let ubu_tpl = terminal(&a, &ubu_build).await["template_id"].clone();
        let web_tpl = terminal(
            &a,
            &call(
                &a,
                "vm_template_build",
                json!({"recipe_id":hex::encode(WEB_RECIPE_ID),"request_id":key(2601)}),
            )
            .await,
        )
        .await["template_id"]
            .clone();

        let run_clone = call(
            &a,
            "vm_create",
            json!({"template_id":ubu_tpl,"request_id":key(2602),"ttl_s":60}),
        )
        .await;
        let vm_run = terminal(&a, &run_clone).await["vm_id"].clone();
        let vm_run_id = parse_id(vm_run.as_str().unwrap()).unwrap();
        let sib_test = terminal(
            &a,
            &call(
                &a,
                "vm_create",
                json!({"template_id":ubu_tpl,"request_id":key(2603),"ttl_s":600}),
            )
            .await,
        )
        .await["vm_id"]
            .clone();
        let sib_static = terminal(
            &a,
            &call(
                &a,
                "static_vm_create",
                json!({"name":"exp-sib-static","template_id":web_tpl,"request_id":key(2604)}),
            )
            .await,
        )
        .await["vm_id"]
            .clone();
        assert_eq!(
            terminal(
                &a,
                &call(
                    &a,
                    "vm_snapshot_create",
                    json!({"vm_id":sib_static,"request_id":key(2605)}),
                )
                .await,
            )
            .await["state"],
            "succeeded"
        );

        let snap1 = terminal(
            &a,
            &call(
                &a,
                "vm_snapshot_create",
                json!({"vm_id":vm_run,"request_id":key(2606)}),
            )
            .await,
        )
        .await["snapshot_id"]
            .clone();
        let snap2 = terminal(
            &a,
            &call(
                &a,
                "vm_snapshot_create",
                json!({"vm_id":vm_run,"request_id":key(2607)}),
            )
            .await,
        )
        .await["snapshot_id"]
            .clone();

        // Start vm_run and force expiry through force_reap (joining peer+guest).
        assert_eq!(
            terminal(
                &a,
                &call(
                    &a,
                    "vm_start",
                    json!({"vm_id":vm_run,"request_id":key(2608)})
                )
                .await,
            )
            .await["state"],
            "succeeded"
        );
        h.fixture(|f| {
            f.live.lock().unwrap().force_reap_only.insert(vm_run_id);
        });

        let active_paths = fixture_pair_paths(&h, &vm_run, true);
        let sib_test_paths = fixture_pair_paths(&h, &sib_test, true);
        let sib_static_paths = fixture_pair_paths(&h, &sib_static, true);
        let snap_paths = |snap: &Value| -> [std::path::PathBuf; 2] {
            let sid = parse_id(snap.as_str().unwrap()).unwrap();
            h.supervisor()
                .controller
                .store_for_test()
                .transact(|db| {
                    let mut stmt = db.prepare(
                        "SELECT a.role,a.object_key FROM vm_restore_points p
                         JOIN vm_artifacts a ON a.generation_id=p.generation
                         WHERE p.id=?1 ORDER BY a.role",
                    )?;
                    let files = stmt
                        .query_map([sid], |row| {
                            Ok(h.media.join(format!(
                                "{}-{}",
                                row.get::<_, String>(0)?,
                                hex::encode(row.get::<_, Id>(1)?)
                            )))
                        })?
                        .collect::<rusqlite::Result<Vec<_>>>()?;
                    Ok(files.try_into().expect("snapshot pair"))
                })
                .unwrap()
        };
        let snap1_paths = snap_paths(&snap1);
        let snap2_paths = snap_paths(&snap2);
        let golden_paths: [std::path::PathBuf; 2] = {
            let tpl_id = parse_id(ubu_tpl.as_str().unwrap()).unwrap();
            h.supervisor()
                .controller
                .store_for_test()
                .transact(|db| {
                    let mut stmt = db.prepare(
                        "SELECT a.role,a.object_key FROM vm_build_intents b
                         JOIN vm_generations g ON g.build_id=b.id AND g.active=1
                         JOIN vm_artifacts a ON a.generation_id=g.id
                         WHERE b.template_id=?1 ORDER BY a.role",
                    )?;
                    let files = stmt
                        .query_map([tpl_id], |row| {
                            Ok(h.media.join(format!(
                                "{}-{}",
                                row.get::<_, String>(0)?,
                                hex::encode(row.get::<_, Id>(1)?)
                            )))
                        })?
                        .collect::<rusqlite::Result<Vec<_>>>()?;
                    Ok(files.try_into().expect("golden pair"))
                })
                .unwrap()
        };
        let deadline = run_clone["expires_at_ms"].as_i64().unwrap();
        h.now.store(deadline, Ordering::Relaxed);

        // Sweep 1: release-only force_reap joins boot without disposing active media,
        // then disposes exactly ONE non-active snapshot pair.
        h.sweep().await;
        assert!(!h.fixture(|f| f.ready(vm_run_id)));
        assert!(active_paths.iter().all(|p| p.exists()));
        let s1_alive = snap1_paths.iter().all(|p| p.exists());
        let s2_alive = snap2_paths.iter().all(|p| p.exists());
        assert_ne!(s1_alive, s2_alive, "exactly one snapshot pair disposed");
        assert_eq!(
            h.supervisor()
                .controller
                .store_for_test()
                .transact(|db| crate::store::vm::count(
                    db,
                    "SELECT count(*) FROM vm_tombstones",
                    []
                ))
                .unwrap(),
            0
        );

        // Sweep 2: disposes the second non-active snapshot pair; active still kept.
        h.sweep().await;
        assert!(active_paths.iter().all(|p| p.exists()));
        assert!(snap1_paths.iter().all(|p| !p.exists()));
        assert!(snap2_paths.iter().all(|p| !p.exists()));
        assert_eq!(
            h.supervisor()
                .controller
                .store_for_test()
                .transact(|db| crate::store::vm::count(
                    db,
                    "SELECT count(*) FROM vm_tombstones",
                    []
                ))
                .unwrap(),
            0
        );

        // Sweep 3: disposes active pair LAST and writes the tombstone.
        h.sweep().await;
        assert!(active_paths.iter().all(|p| !p.exists()));
        assert_eq!(
            call(&a, "vm_get", json!({"vm_id":vm_run})).await["state"],
            "expired"
        );
        h.supervisor()
            .controller
            .store_for_test()
            .transact(|db| {
                assert_eq!(
                    crate::store::vm::count(
                        db,
                        "SELECT count(*) FROM vm_tombstones WHERE id=?1",
                        [vm_run_id]
                    )?,
                    1
                );
                assert_eq!(
                    crate::store::vm::count(
                        db,
                        "SELECT count(*) FROM vm_snapshot_reconciliation
                         WHERE vm=?1 AND cause='expiry_point' AND expiry_activity IS NOT NULL",
                        [vm_run_id]
                    )?,
                    2
                );
                Ok(())
            })
            .unwrap();

        // Sibling TEST, sibling static (+ backup), and golden remain untouched.
        assert!(golden_paths.iter().all(|p| p.exists()));
        assert!(sib_test_paths.iter().all(|p| p.exists()));
        assert!(sib_static_paths.iter().all(|p| p.exists()));
        assert_eq!(
            call(&a, "vm_snapshot_list", json!({"vm_id":sib_static})).await["entries"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        drop(a);
        h.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn compose_sdk_lost_reply_is_queried_and_settled_once() {
        use crate::store::vm::compose as db;
        use arx_guest::compose::{self as guest, ComposeRequest as Req, ComposeResponse as Resp};

        struct FakeDpapi;
        const CANARY: &[u8] = b"fixture-compose-custody-canary";
        impl crate::vm::compose_secrets::SecretSource for FakeDpapi {
            fn open(&self, reference: Id) -> Result<zeroize::Zeroizing<Vec<u8>>> {
                anyhow::ensure!(reference == [0x44; 16]);
                Ok(zeroize::Zeroizing::new(CANARY.to_vec()))
            }
        }

        let mut h = Harness::start_with_web(false, false, true).await;
        let a = h.client(0).await;
        let b = h.client(1).await;
        let (vm, _) = enrolled(&h, &a, 120).await;
        let vm_id = parse_id(vm.as_str().unwrap()).unwrap();
        let project = "fixture-compose";
        let release_id = hex::encode([0x42; 16]);
        let mut second_release = json!({
            "vm_id":vm, "release_id":release_id,
            "manifest":{
                "schema":guest::MANIFEST_SCHEMA,"project_id":project,
                "volumes":[{"name":"data","uid":10001,"gid":10001}],"secrets":["password"],
                "services":[{"name":"api","image":format!("example.invalid/api@sha256:{}", "a".repeat(64)),
                    "platform":guest::PLATFORM,"uid":10001,"gid":10001,"read_only_rootfs":true,"http_port":8080,"health_path":"/health",
                    "environment":[],"mounts":[{"volume":"data","target":"/data","read_only":false}],
                    "secrets":["password"],"limits":{"cpu_millis":500,"memory_mib":512,"pids":32}}]
            },
            "secrets":[{"slot":"password","epoch":7,"reference":hex::encode([0x44;16])}]
        });
        let enrollment: compose::ReleaseInput =
            serde_json::from_value(second_release.clone()).unwrap();
        second_release["release_id"] = json!(hex::encode([0x43; 16]));
        second_release["secrets"][0]["epoch"] = json!(8);
        second_release["secrets"][0]["reference"] = json!(hex::encode([0x45; 16]));
        second_release["manifest"]["services"][0]["gid"] = json!(10002);
        second_release["manifest"]["services"][0]["mounts"] = json!([]);
        second_release["manifest"]["volumes"] = json!([]);
        let other: compose::ReleaseInput = serde_json::from_value(second_release).unwrap();
        let enrollment = enrollment.enroll().unwrap();
        let digest = enrollment.digest.clone();
        let target = enrollment.target.clone();
        let store = h.supervisor().controller.compose_store();
        db::enroll(store, &[enrollment, other.enroll().unwrap()]).unwrap();
        let generation = store
            .transact(|tx| db::boot(tx, "alpha", &target))
            .unwrap()
            .unwrap();
        *h.supervisor().compose.secrets.lock().unwrap() = Some(Arc::new(FakeDpapi));
        let boot = 19;
        let args =
            json!({"vm_id":vm,"project_id":project,"release_id":release_id,"request_id":key(125)});
        assert_eq!(
            call(&a, "compose_deploy", args.clone()).await["reason"],
            "not_ready"
        );
        let (host, mut peer) = tokio::io::duplex(64 * 1024);
        let (effect, effect_seen) = tokio::sync::oneshot::channel();
        let (close, allow_close) = tokio::sync::oneshot::channel();
        let expected_digest = digest.clone();
        let first = tokio::spawn(async move {
            fake_compose_hello(&mut peer, boot).await;
            let Req::ComposeOperation { id, .. } = fake_compose_read(&mut peer).await else {
                panic!("operation query must precede submission")
            };
            fake_compose_reply(
                &mut peer,
                Resp::ComposeRejected {
                    id,
                    code: guest::RejectCode::UnknownOperation,
                    rule: None,
                },
            )
            .await;
            let Req::ComposeStatus { id, project_id } = fake_compose_read(&mut peer).await else {
                panic!("status query before submission")
            };
            assert_eq!(project_id, project);
            fake_compose_reply(
                &mut peer,
                Resp::ComposeRejected {
                    id,
                    code: guest::RejectCode::UnknownProject,
                    rule: None,
                },
            )
            .await;
            let Req::ComposeStageBegin {
                id,
                digest,
                total_bytes,
                chunks,
            } = fake_compose_read(&mut peer).await
            else {
                panic!("stage begin")
            };
            assert_eq!(digest, expected_digest);
            assert_eq!(chunks, 1);
            fake_compose_reply(
                &mut peer,
                Resp::ComposeStageProgress {
                    id,
                    digest: digest.clone(),
                    received_bytes: 0,
                    next_index: 0,
                },
            )
            .await;
            let Req::ComposeStageChunk {
                id,
                index: 0,
                digest: chunk_digest,
                ..
            } = fake_compose_read(&mut peer).await
            else {
                panic!("stage chunk")
            };
            assert_eq!(chunk_digest, digest);
            fake_compose_reply(
                &mut peer,
                Resp::ComposeStageProgress {
                    id,
                    digest: digest.clone(),
                    received_bytes: total_bytes,
                    next_index: 1,
                },
            )
            .await;
            let Req::ComposeStageCommit {
                id,
                digest: commit_digest,
            } = fake_compose_read(&mut peer).await
            else {
                panic!("stage commit")
            };
            assert_eq!(commit_digest, digest);
            fake_compose_reply(
                &mut peer,
                Resp::ComposeReleaseStaged {
                    id,
                    digest: digest.clone(),
                    project_id: project.into(),
                },
            )
            .await;
            let Req::ComposeSecretPut {
                id,
                generation,
                project_id,
                digest: bound_digest,
                slot,
                epoch,
                data,
                ..
            } = fake_compose_read(&mut peer).await
            else {
                panic!("private secret delivery")
            };
            let reference = guest::SecretText::new(guest::encode_base64(CANARY));
            assert!(data == reference);
            assert_eq!(
                (
                    generation,
                    project_id.as_str(),
                    bound_digest.as_str(),
                    slot.as_str(),
                    epoch
                ),
                (boot, project, digest.as_str(), "password", 7)
            );
            fake_compose_reply(
                &mut peer,
                Resp::ComposeSecretAccepted {
                    id,
                    project_id,
                    digest: digest.clone(),
                    slot,
                    epoch,
                    recreate: None,
                },
            )
            .await;
            let Req::ComposeDeploy {
                operation_id,
                project_id,
                revision,
                digest: released,
                ..
            } = fake_compose_read(&mut peer).await
            else {
                panic!("one deploy")
            };
            assert_eq!(
                (project_id.as_str(), revision, released.as_str()),
                (project, 1, digest.as_str())
            );
            effect.send(operation_id).unwrap();
            allow_close.await.unwrap();
            // The fake effect is durable. Drop the reply and close the stream.
        });
        let channel =
            crate::vm::guest::ComposeChannel::negotiate(host, boot, &h.supervisor().cancel)
                .await
                .unwrap();
        h.supervisor()
            .compose
            .attach(store, vm_id, compose::Session::new(generation, channel))
            .unwrap();
        let mut extras = args.clone();
        extras["extra"] = json!(true);
        let invalid = call(&a, "compose_deploy", extras).await;
        assert!(invalid["operation_id"].is_null());
        let mut legacy_key = args.clone();
        legacy_key["request_id"] = json!(key(122));
        assert_eq!(
            call(&a, "compose_deploy", legacy_key).await["reason"],
            "key_collision"
        );
        let accepted = call(&a, "compose_deploy", args.clone()).await;
        assert_eq!(accepted["state"], "pending");
        for (field, value) in [
            ("vm_id", json!(key(141))),
            ("project_id", json!("another")),
            ("release_id", json!(hex::encode([0x43; 16]))),
        ] {
            let mut changed = args.clone();
            changed[field] = value;
            assert_eq!(
                call(&a, "compose_deploy", changed).await["reason"],
                "key_collision"
            );
        }
        assert_eq!(
            call(&a, "vm_start", json!({"vm_id":vm,"request_id":key(125)})).await["reason"],
            "key_collision"
        );
        let pinned = store
            .transact(|tx| db::replay(tx, "alpha", parse_id(key(125).as_str()).unwrap()))
            .unwrap()
            .unwrap();
        assert_eq!(pinned.secrets.len(), 1);
        assert_eq!(pinned.secrets[0].reference, [0x44; 16]);
        assert_eq!(pinned.secrets[0].epoch, 7);
        let operation_id = effect_seen.await.unwrap();
        assert_eq!(accepted["operation_id"], operation_id);
        assert_eq!(
            call(&b, "compose_deploy", args.clone()).await["reason"],
            "not_found"
        );
        assert_eq!(
            call(
                &a,
                "compose_start",
                json!({
                    "vm_id":vm,"project_id":project,"request_id":key(125)
                })
            )
            .await["reason"],
            "key_collision"
        );
        assert_eq!(
            call(
                &a,
                "compose_down",
                json!({
                    "vm_id":vm,"project_id":project,"request_id":key(126)
                })
            )
            .await["reason"],
            "busy"
        );
        a.cancel().await.unwrap();
        close.send(()).unwrap();
        first.await.unwrap();
        until(|| {
            h.supervisor()
                .gate
                .lock()
                .unwrap()
                .compose_active
                .lock()
                .unwrap()
                .is_empty()
        })
        .await;
        let a = h.client(0).await;
        assert_eq!(
            call(
                &a,
                "compose_status",
                json!({"vm_id":vm,"project_id":project})
            )
            .await["desired"],
            "none"
        );

        let (host, mut peer) = tokio::io::duplex(64 * 1024);
        let recovered_operation = operation_id.clone();
        let revision = accepted["revision"].as_u64().unwrap();
        let second = tokio::spawn(async move {
            fake_compose_hello(&mut peer, boot).await;
            for case in [
                "unknown",
                "wrong_operation",
                "wrong_stage",
                "wrong_revision",
                "recorded",
                "unhealthy",
                "proved",
            ] {
                let committed_revision = Some(if case == "wrong_revision" {
                    revision + 1
                } else {
                    revision
                });
                let Req::ComposeOperation { id, operation_id } = fake_compose_read(&mut peer).await
                else {
                    panic!("replay must query")
                };
                assert_eq!(operation_id, recovered_operation);
                if case == "unknown" {
                    fake_compose_reply(
                        &mut peer,
                        Resp::ComposeRejected {
                            id,
                            code: guest::RejectCode::UnknownOperation,
                            rule: None,
                        },
                    )
                    .await;
                    continue;
                }
                fake_compose_reply(
                    &mut peer,
                    Resp::ComposeOperationState {
                        id,
                        operation: guest::OperationView {
                            operation_id: if case == "wrong_operation" {
                                hex::encode([0xff; 16])
                            } else {
                                operation_id
                            },
                            kind: guest::OperationKind::Deploy,
                            project_id: project.into(),
                            revision: 1,
                            state: guest::OperationState::Succeeded,
                            stage: if case == "wrong_stage" {
                                guest::OperationStage::Applying
                            } else {
                                guest::OperationStage::Settled
                            },
                            failure: None,
                            restored_previous: false,
                        },
                    },
                )
                .await;
                if matches!(case, "wrong_operation" | "wrong_stage") {
                    continue;
                }
                let Req::ComposeStatus { id, .. } = fake_compose_read(&mut peer).await else {
                    panic!("attributed state required")
                };
                fake_compose_reply(
                    &mut peer,
                    Resp::ComposeProjectStatus {
                        id,
                        project: guest::ProjectView {
                            project_id: project.into(),
                            desired: guest::Desired::Started,
                            committed_revision,
                            previous_revision: None,
                            recovery_required: false,
                            running_operation: None,
                            observation: if case == "recorded" {
                                guest::Observation::Recorded
                            } else {
                                guest::Observation::Live
                            },
                            services: vec![guest::ServiceView {
                                name: "api".into(),
                                state: guest::ServiceState::Running,
                                health: if case == "unhealthy" {
                                    guest::HealthState::Unhealthy
                                } else {
                                    guest::HealthState::None
                                },
                                revision: Some(1),
                            }],
                        },
                    },
                )
                .await;
            }
            let Req::ComposeLogs {
                id,
                cursor: 0,
                limit: 1,
                ..
            } = fake_compose_read(&mut peer).await
            else {
                panic!("bounded log read")
            };
            fake_compose_reply(
                &mut peer,
                Resp::ComposeLogPage {
                    id,
                    next_cursor: 2,
                    entries: vec![guest::LogEntry {
                        seq: 1,
                        unix_s: 4,
                        project_id: project.into(),
                        event: guest::LogEvent::Promoted,
                        service: Some("canary".into()),
                        failure: None,
                    }],
                },
            )
            .await;
            let mut desired = guest::Desired::Started;
            let mut partial_down = false;
            let mut rejected_secret = false;
            for (kind, refusal) in [
                (guest::OperationKind::Stop, true),
                (guest::OperationKind::Down, true),
                (guest::OperationKind::Stop, false),
                (guest::OperationKind::Start, true),
                (guest::OperationKind::Start, true),
                (guest::OperationKind::Start, false),
                (guest::OperationKind::Down, true),
                (guest::OperationKind::Down, false),
                (guest::OperationKind::Stop, true),
            ] {
                let Req::ComposeOperation { id, operation_id } = fake_compose_read(&mut peer).await
                else {
                    panic!("initial lookup")
                };
                fake_compose_reply(
                    &mut peer,
                    Resp::ComposeRejected {
                        id,
                        code: guest::RejectCode::UnknownOperation,
                        rule: None,
                    },
                )
                .await;
                let status = |desired| guest::ProjectView {
                    project_id: project.into(),
                    desired,
                    committed_revision: Some(revision),
                    previous_revision: None,
                    recovery_required: false,
                    running_operation: None,
                    observation: guest::Observation::Live,
                    services: if desired == guest::Desired::Down {
                        vec![]
                    } else {
                        vec![guest::ServiceView {
                            name: "api".into(),
                            revision: Some(revision),
                            state: if desired == guest::Desired::Started {
                                guest::ServiceState::Running
                            } else {
                                guest::ServiceState::Exited
                            },
                            health: guest::HealthState::None,
                        }]
                    },
                };
                let Req::ComposeStatus { id, .. } = fake_compose_read(&mut peer).await else {
                    panic!("preflight")
                };
                fake_compose_reply(
                    &mut peer,
                    Resp::ComposeProjectStatus {
                        id,
                        project: {
                            let mut view = status(desired);
                            view.recovery_required = partial_down;
                            view
                        },
                    },
                )
                .await;
                if kind == guest::OperationKind::Start {
                    let Req::ComposeSecretPut {
                        operation_id: secret_operation,
                        id,
                        project_id,
                        digest,
                        slot,
                        epoch,
                        ..
                    } = fake_compose_read(&mut peer).await
                    else {
                        panic!("start redelivery")
                    };
                    if refusal && rejected_secret {
                        fake_compose_reply(
                            &mut peer,
                            Resp::ComposeRejected {
                                id,
                                code: guest::RejectCode::StaleEpoch,
                                rule: None,
                            },
                        )
                        .await;
                        continue;
                    }
                    if refusal {
                        rejected_secret = true;
                        fake_compose_reply(
                            &mut peer,
                            Resp::ComposeOperationState {
                                id,
                                operation: guest::OperationView {
                                    project_id,
                                    kind: guest::OperationKind::SecretPut,
                                    operation_id: secret_operation,
                                    revision,
                                    stage: guest::OperationStage::Settled,
                                    state: guest::OperationState::Failed,
                                    failure: Some(guest::FailureCode::NotApplied),
                                    restored_previous: false,
                                },
                            },
                        )
                        .await;
                        continue;
                    }
                    fake_compose_reply(
                        &mut peer,
                        Resp::ComposeSecretAccepted {
                            id,
                            project_id,
                            digest,
                            slot,
                            epoch,
                            recreate: None,
                        },
                    )
                    .await;
                }
                let (id, echo, next) = match fake_compose_read(&mut peer).await {
                    Req::ComposeStart {
                        id, operation_id, ..
                    } if kind == guest::OperationKind::Start => {
                        (id, operation_id, guest::Desired::Started)
                    }
                    Req::ComposeStop {
                        id, operation_id, ..
                    } if kind == guest::OperationKind::Stop => {
                        (id, operation_id, guest::Desired::Stopped)
                    }
                    Req::ComposeDown {
                        id, operation_id, ..
                    } if kind == guest::OperationKind::Down => {
                        (id, operation_id, guest::Desired::Down)
                    }
                    _ => panic!("lifecycle attribution"),
                };
                assert_eq!(echo, operation_id);
                if refusal && kind == guest::OperationKind::Down {
                    partial_down = true;
                    fake_compose_reply(
                        &mut peer,
                        Resp::ComposeOperationState {
                            id,
                            operation: guest::OperationView {
                                operation_id,
                                kind,
                                project_id: project.into(),
                                revision,
                                state: guest::OperationState::Failed,
                                stage: guest::OperationStage::Settled,
                                failure: Some(guest::FailureCode::DownFailed),
                                restored_previous: false,
                            },
                        },
                    )
                    .await;
                    continue;
                }
                if refusal {
                    fake_compose_reply(
                        &mut peer,
                        Resp::ComposeRejected {
                            id,
                            code: if desired == guest::Desired::Down {
                                guest::RejectCode::StoreFailure
                            } else {
                                guest::RejectCode::JournalFull
                            },
                            rule: None,
                        },
                    )
                    .await;
                    continue;
                }
                let mut id = id;
                if kind == guest::OperationKind::Down {
                    // The container view may already be empty while the generated network
                    // still exists. Only the guest can verify both inventories.
                    for state in [
                        guest::OperationState::Running,
                        guest::OperationState::Ambiguous,
                    ] {
                        fake_compose_reply(
                            &mut peer,
                            Resp::ComposeOperationState {
                                id,
                                operation: guest::OperationView {
                                    operation_id: operation_id.clone(),
                                    project_id: project.into(),
                                    revision,
                                    kind,
                                    stage: guest::OperationStage::Removing,
                                    state,
                                    failure: None,
                                    restored_previous: false,
                                },
                            },
                        )
                        .await;
                        let (next_id, lookup) = match fake_compose_read(&mut peer).await {
                            Req::ComposeOperation { id, operation_id }
                                if state == guest::OperationState::Running =>
                            {
                                (id, operation_id)
                            }
                            Req::ComposeReconcile { id, operation_id }
                                if state == guest::OperationState::Ambiguous =>
                            {
                                (id, operation_id)
                            }
                            _ => panic!("Down requires durable network and container absence"),
                        };
                        assert_eq!(lookup, operation_id);
                        id = next_id;
                    }
                }
                partial_down = false;
                desired = next;
                fake_compose_reply(
                    &mut peer,
                    Resp::ComposeOperationState {
                        id,
                        operation: guest::OperationView {
                            operation_id,
                            kind,
                            project_id: project.into(),
                            revision,
                            state: guest::OperationState::Succeeded,
                            stage: guest::OperationStage::Settled,
                            failure: None,
                            restored_previous: false,
                        },
                    },
                )
                .await;
                let Req::ComposeStatus { id, .. } = fake_compose_read(&mut peer).await else {
                    panic!("settlement observation")
                };
                fake_compose_reply(
                    &mut peer,
                    Resp::ComposeProjectStatus {
                        id,
                        project: status(desired),
                    },
                )
                .await;
            }
        });
        let channel =
            crate::vm::guest::ComposeChannel::negotiate(host, boot, &h.supervisor().cancel)
                .await
                .unwrap();
        h.supervisor()
            .compose
            .attach(store, vm_id, compose::Session::new(generation, channel))
            .unwrap();
        for _ in 0..7 {
            let replay = call(&a, "compose_deploy", args.clone()).await;
            assert_eq!(replay["operation_id"], accepted["operation_id"]);
            assert_eq!(replay["state"], "unresolved");
            until(|| {
                h.supervisor()
                    .gate
                    .lock()
                    .unwrap()
                    .compose_active
                    .lock()
                    .unwrap()
                    .is_empty()
            })
            .await;
        }
        // Fill the normal admission budget with retained terminal receipts. Roll back
        // this pressure fixture after checking the Stop reserve and expiry maintenance.
        store.transact(|tx| {
            tx.execute_batch("SAVEPOINT pressure")?;
            tx.execute("WITH RECURSIVE seq(i) AS (VALUES(1) UNION ALL SELECT i+1 FROM seq WHERE i<239)
                INSERT INTO vm_compose_operations(id,principal,request,vm,project,generation,boot,kind,release,revision,valid_through_ms,secret_refs,closed)
                SELECT randomblob(16),principal,randomblob(16),vm,project,generation,boot,kind,release,revision,valid_through_ms,secret_refs,'refused'
                FROM vm_compose_operations,seq WHERE id=?1",[pinned.id])?;
            let key=RequestId::parse(&key(181)).unwrap();
            let mut request=compose::Mutation {key,kind:compose::Kind::Start,release:None,target:target.clone()};
            assert!(matches!(db::admit(tx,"alpha",&request,generation,boot,UTC)?,Err(compose::Fault::ReplayFull)));
            request.kind=compose::Kind::Stop;
            // Consume every ordinary cleanup slot, including repeated terminal refusals.
            for nonce in 181..197 {
                request.key=RequestId::parse(&format!("{UTC:016x}{nonce:016x}")).unwrap();
                let cleanup=db::admit(tx,"alpha",&request,generation,boot,UTC)?.unwrap();
                assert!(!tx.query_row("SELECT emergency FROM vm_compose_operations WHERE id=?1",[cleanup.id],|r|r.get::<_,bool>(0))?);
                db::close(tx,&cleanup,"refused",UTC)?;
            }
            let rollback=RequestId::parse(&format!("{:016x}{:016x}",UTC-1,900)).unwrap();
            let legacy=crate::store::vm::Binding {principal:"alpha",key:rollback,subject:crate::vm::Subject::Vm(vm_id),action:crate::vm::Action::Stop as i64};
            assert!(matches!(legacy.replay(tx,UTC-1,false)?,Ok(None)));
            let first_spare=RequestId::parse(&format!("{UTC:016x}{:016x}",197)).unwrap();
            request.key=first_spare;
            let emergency=db::admit(tx,"alpha",&request,generation,boot,UTC)?.unwrap();
            assert_eq!(db::replay(tx,"alpha",first_spare.key)?.unwrap().id,emergency.id);
            db::close(tx,&emergency,"refused",UTC)?;
            request.key=RequestId::parse(&format!("{:016x}{:016x}",UTC-1,198)).unwrap();
            let replacement=db::admit(tx,"alpha",&request,generation,boot,UTC-1)?.unwrap();
            assert!(matches!(legacy.replay(tx,UTC-1,false)?,Ok(None)));
            assert_ne!(replacement.id,emergency.id);
            assert!(db::replay(tx,"alpha",key.key)?.is_some());
            assert_eq!(tx.query_row("SELECT count(*) FROM vm_compose_operations WHERE emergency=1",[],|r|r.get::<_,i64>(0))?,1);
            assert!(db::replay(tx,"alpha",first_spare.key)?.is_none());
            db::close(tx,&replacement,"refused",UTC)?;
            request.key=first_spare;
            assert!(matches!(db::admit(tx,"alpha",&request,generation,boot,UTC)?,Err(compose::Fault::StaleKey)));
            let later=UTC+crate::vm::RETRY_WINDOW_MS+1;
            let fresh=RequestId::parse(&format!("{later:016x}{:016x}",182)).unwrap();
            // Positive boot retirement releases the lease; simulate completed stop/start.
            tx.execute("UPDATE vm_static_instances SET state=1 WHERE id=?1",[vm_id])?;
            tx.execute("UPDATE vm_static_instances SET state=2 WHERE id=?1",[vm_id])?;
            db::register_boot(tx,vm_id,generation,boot+1)?;
            request.key=fresh;
            request.kind=compose::Kind::Deploy;
            request.release=Some(pinned.release);
            let next=db::admit(tx,"alpha",&request,generation,boot+1,later)?.unwrap();
            assert_eq!(next.revision,pinned.revision+1);
            db::close(tx,&next,"refused",later)?;
            request.key=key;
            assert!(matches!(db::admit(tx,"alpha",&request,generation,boot+1,UTC)?,Err(compose::Fault::StaleKey)));
            tx.execute_batch("ROLLBACK TO pressure; RELEASE pressure")?;
            Ok(())
        }).unwrap();
        let settled = call(&a, "compose_deploy", args.clone()).await;
        assert_eq!(settled["operation_id"], operation_id);
        assert_eq!(settled["state"], "succeeded");
        let logs = call(
            &a,
            "compose_logs",
            json!({"vm_id":vm,"project_id":project,"limit":1}),
        )
        .await;
        assert_eq!(logs["reason"], "not_ready");
        assert!(!logs.to_string().contains("canary"));
        for (index, (tool, state)) in [
            ("compose_stop", "refused"),
            ("compose_down", "failed"),
            ("compose_stop", "succeeded"),
            ("compose_start", "secret_failed"),
            ("compose_start", "secret_failed"),
            ("compose_start", "succeeded"),
            ("compose_down", "failed"),
            ("compose_down", "succeeded"),
            ("compose_stop", "unresolved"),
        ]
        .into_iter()
        .enumerate()
        {
            let args = json!({"vm_id":vm,"project_id":project,"request_id":key(130+index as u64)});
            let begun = call(&a, tool, args.clone()).await;
            assert_eq!(begun["state"], "pending");
            until(|| {
                h.supervisor()
                    .gate
                    .lock()
                    .unwrap()
                    .compose_active
                    .lock()
                    .unwrap()
                    .is_empty()
            })
            .await;
            let outcome = call(&a, tool, args).await;
            assert_eq!(outcome["operation_id"], begun["operation_id"]);
            assert_eq!(outcome["state"], state);
        }
        second.await.unwrap();
        let stopped = call(&a, "vm_stop", json!({"vm_id":vm,"request_id":key(199)})).await;
        assert_eq!(terminal(&a, &stopped).await["state"], "succeeded");
        let retired = call(
            &a,
            "compose_stop",
            json!({"vm_id":vm,"project_id":project,"request_id":key(138)}),
        )
        .await;
        let audit = call(&a, "list_activity", json!({"limit":50})).await;
        let rows = audit["activities"].as_array().unwrap();
        let matching = rows
            .iter()
            .filter(|row| row["operation_id"] == accepted["operation_id"])
            .collect::<Vec<_>>();
        assert_eq!(matching.len(), 1);
        assert_eq!(matching[0]["action"], "compose_deploy");
        assert_eq!(matching[0]["state"], "succeeded");
        assert!(
            rows.iter()
                .any(|r| r["action"] == "compose_deploy" && r["reason"] == "invalid")
        );
        assert!(
            rows.iter().any(
                |r| r["operation_id"] == retired["operation_id"] && r["state"] == "interrupted"
            )
        );
        let foreign = call(&b, "list_activity", json!({"limit":50})).await;
        assert!(
            foreign["activities"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["action"] == "compose_deploy"
                    && r["reason"] == "not_found"
                    && r["vm_id"].is_null())
        );
        assert_eq!(retired["state"], "retired_boot");
        assert!(retired["receipt"].is_null());
        let restarted = call(&a, "vm_start", json!({"vm_id":vm,"request_id":key(200)})).await;
        assert_eq!(
            terminal_static_start(&a, &restarted).await["state"],
            "succeeded"
        );
        store
            .transact(|tx| {
                assert!(db::register_boot(tx, vm_id, generation, boot).is_err());
                db::register_boot(tx, vm_id, generation, boot + 1)?;
                assert!(db::register_boot(tx, vm_id, generation, boot + 2).is_err());
                let old = db::replay(tx, "alpha", parse_id(key(138).as_str()).unwrap())?.unwrap();
                assert!(db::mark_sent(tx, &old).is_err());
                Ok(())
            })
            .unwrap();
        let status = call(
            &a,
            "compose_status",
            json!({"vm_id":vm,"project_id":project}),
        )
        .await;
        assert_eq!(status["desired"], "down");
        assert_eq!(status["current_boot"], false);
        assert_eq!(status["observed"]["observation"], "recorded");
        assert!(status["operation_id"].is_null());
        store
            .transact(|tx| {
                assert_eq!(
                    crate::store::vm::count(tx, "SELECT sum(charge) FROM vm_compose_volumes", [])?,
                    1
                );
                assert_eq!(
                    crate::store::vm::count(tx, "SELECT count(*) FROM vm_compose_operations", [])?,
                    10
                );
                Ok(())
            })
            .unwrap();
        drop(a);
        drop(b);
        h.shutdown().await.unwrap();
        let mut h = h.reopen(|_| {}).await;
        let a = h.client(0).await;
        assert_eq!(call(&a, "compose_deploy", args).await, settled);
        drop(a);
        h.shutdown().await.unwrap();
        let mut bytes = Vec::new();
        for entry in fs::read_dir(h.temp.path()).unwrap().flatten() {
            if entry
                .file_name()
                .to_string_lossy()
                .starts_with("activity.sqlite3")
                && entry.path().is_file()
            {
                bytes.extend(fs::read(entry.path()).unwrap());
            }
        }
        for projection in [&accepted, &settled, &status, &logs] {
            bytes.extend(serde_json::to_vec(projection).unwrap());
        }
        let encoded = guest::encode_base64(CANARY);
        assert!(!bytes.windows(CANARY.len()).any(|window| window == CANARY));
        assert!(
            !bytes
                .windows(encoded.len())
                .any(|window| window == encoded.as_bytes())
        );
    }

    #[tokio::test]
    async fn compose_lost_deploy_reconciles_after_verified_restart() {
        compose_retirement(None).await;
    }

    #[tokio::test]
    async fn compose_missing_retired_deploy_allows_stop_and_down_after_reopen() {
        for down in [false, true] {
            compose_retirement(Some((false, down))).await;
        }
    }

    #[tokio::test]
    async fn compose_ambiguous_retired_deploy_allows_stop_and_down_after_reopen() {
        for down in [false, true] {
            compose_retirement(Some((true, down))).await;
        }
    }

    async fn compose_retirement(cleanup: Option<(bool, bool)>) {
        use crate::store::vm::compose as db;
        use arx_guest::compose::{self as g, ComposeRequest as Req, ComposeResponse as Resp};
        const PROJECT: &str = "reboot-test";
        fn view(revision: u64) -> g::ProjectView {
            g::ProjectView {
                project_id: PROJECT.into(),
                desired: g::Desired::Started,
                committed_revision: Some(revision),
                previous_revision: revision.checked_sub(1).filter(|r| *r > 0),
                recovery_required: false,
                running_operation: None,
                observation: g::Observation::Live,
                services: vec![g::ServiceView {
                    name: "api".into(),
                    state: g::ServiceState::Running,
                    health: g::HealthState::None,
                    revision: Some(revision),
                }],
            }
        }
        async fn status(peer: &mut tokio::io::DuplexStream, revision: Option<u64>) {
            let Req::ComposeStatus { id, project_id } = fake_compose_read(peer).await else {
                panic!("project observation required")
            };
            assert_eq!(project_id, PROJECT);
            let reply = match revision {
                None => Resp::ComposeRejected {
                    id,
                    code: g::RejectCode::UnknownProject,
                    rule: None,
                },
                Some(r) => Resp::ComposeProjectStatus {
                    id,
                    project: view(r),
                },
            };
            fake_compose_reply(peer, reply).await;
        }
        async fn unknown(peer: &mut tokio::io::DuplexStream) -> String {
            let Req::ComposeOperation { id, operation_id } = fake_compose_read(peer).await else {
                panic!("query before effect")
            };
            fake_compose_reply(
                peer,
                Resp::ComposeRejected {
                    id,
                    code: g::RejectCode::UnknownOperation,
                    rule: None,
                },
            )
            .await;
            operation_id
        }
        async fn idle(h: &Harness) {
            until(|| {
                h.supervisor()
                    .gate
                    .lock()
                    .unwrap()
                    .compose_active
                    .lock()
                    .unwrap()
                    .is_empty()
            })
            .await;
        }
        let mut h = Harness::start_with_web(false, false, true).await;
        let a = h.client(0).await;
        let (vm, _) = enrolled(&h, &a, 3000).await;
        let vm_id = parse_id(vm.as_str().unwrap()).unwrap();
        let store = h.supervisor().controller.compose_store();
        let mut releases = Vec::new();
        for tag in [1_u8, 2] {
            let release:compose::ReleaseInput=serde_json::from_value(json!({"vm_id":vm,"release_id":hex::encode([tag;16]),
                "manifest":{"schema":g::MANIFEST_SCHEMA,"project_id":PROJECT,"volumes":[],"secrets":[],
                    "services":[{"name":"api","image":format!("example.invalid/api@sha256:{}",hex::encode([tag;32])),
                        "platform":g::PLATFORM,"uid":10001,"gid":10001,"read_only_rootfs":true,"environment":[],"mounts":[],
                        "secrets":[],"limits":{"cpu_millis":500,"memory_mib":512,"pids":32}}]}})).unwrap();
            releases.push(release.enroll().unwrap());
        }
        let target = releases[0].target.clone();
        db::enroll(store, &releases).unwrap();
        let generation = store
            .transact(|tx| db::boot(tx, "alpha", &target))
            .unwrap()
            .unwrap();
        let boot = 41;
        let (host, mut peer) = tokio::io::duplex(65536);
        let first = tokio::spawn(async move {
            fake_compose_hello(&mut peer, boot).await;
            for revision in [1, 2] {
                let queried = unknown(&mut peer).await;
                status(&mut peer, if revision == 1 { None } else { Some(1) }).await;
                let Req::ComposeStageBegin {
                    id,
                    digest,
                    total_bytes,
                    chunks,
                } = fake_compose_read(&mut peer).await
                else {
                    panic!("stage")
                };
                fake_compose_reply(
                    &mut peer,
                    Resp::ComposeStageProgress {
                        id,
                        digest: digest.clone(),
                        received_bytes: total_bytes,
                        next_index: chunks,
                    },
                )
                .await;
                let Req::ComposeStageCommit { id, .. } = fake_compose_read(&mut peer).await else {
                    panic!("stage commit")
                };
                fake_compose_reply(
                    &mut peer,
                    Resp::ComposeReleaseStaged {
                        id,
                        digest: digest.clone(),
                        project_id: PROJECT.into(),
                    },
                )
                .await;
                let Req::ComposeDeploy {
                    id,
                    operation_id,
                    project_id,
                    revision: actual,
                    digest: applied,
                } = fake_compose_read(&mut peer).await
                else {
                    panic!("deploy")
                };
                assert_eq!(
                    (actual, applied, operation_id.as_str()),
                    (revision, digest, queried.as_str())
                );
                let receipt = g::OperationView {
                    operation_id,
                    kind: g::OperationKind::Deploy,
                    project_id,
                    revision,
                    state: g::OperationState::Succeeded,
                    stage: g::OperationStage::Settled,
                    failure: None,
                    restored_previous: false,
                };
                if revision == 2 {
                    return receipt;
                } // Guest journal survives the dropped transport.
                fake_compose_reply(
                    &mut peer,
                    Resp::ComposeOperationState {
                        id,
                        operation: receipt,
                    },
                )
                .await;
                status(&mut peer, Some(revision)).await;
            }
            unreachable!()
        });
        let channel =
            crate::vm::guest::ComposeChannel::negotiate(host, boot, &h.supervisor().cancel)
                .await
                .unwrap();
        h.supervisor()
            .compose
            .attach(store, vm_id, compose::Session::new(generation, channel))
            .unwrap();
        let args = |i: u8| json!({"vm_id":vm,"project_id":PROJECT,"release_id":hex::encode([i;16]),"request_id":key(3010+u64::from(i))});
        for i in [1, 2] {
            assert_eq!(
                call(&a, "compose_deploy", args(i)).await["state"],
                "pending"
            );
            idle(&h).await;
        }
        let journal = first.await.unwrap();
        let revision = journal.revision;
        assert_eq!(db_status_revision(store, &target), Some(1));
        let stopped = call(&a, "vm_stop", json!({"vm_id":vm,"request_id":key(3021)})).await;
        assert_eq!(terminal(&a, &stopped).await["state"], "succeeded");
        drop(a);
        h.shutdown().await.unwrap();
        let mut h = h.reopen(|_| {}).await;
        let a = h.client(0).await;
        let store = h.supervisor().controller.compose_store();
        let started = call(&a, "vm_start", json!({"vm_id":vm,"request_id":key(3022)})).await;
        assert_eq!(
            terminal_static_start(&a, &started).await["state"],
            "succeeded"
        );
        let (host, mut peer) = tokio::io::duplex(65536);
        let second = tokio::spawn(async move {
            fake_compose_hello(&mut peer, boot + 1).await;
            if let Some((ambiguous, down)) = cleanup {
                let Req::ComposeOperation { id, operation_id } = fake_compose_read(&mut peer).await
                else {
                    panic!("retired operation lookup")
                };
                assert_eq!(operation_id, journal.operation_id);
                if ambiguous {
                    let mut queued = journal.clone();
                    queued.stage = g::OperationStage::Queued;
                    queued.state = g::OperationState::Ambiguous;
                    fake_compose_reply(
                        &mut peer,
                        Resp::ComposeOperationState {
                            id,
                            operation: queued,
                        },
                    )
                    .await;
                    let Req::ComposeReconcile { id, operation_id } =
                        fake_compose_read(&mut peer).await
                    else {
                        panic!("exact retired intent reconciliation")
                    };
                    assert_eq!(operation_id, journal.operation_id);
                    let mut failed = journal.clone();
                    failed.state = g::OperationState::Failed;
                    failed.failure = Some(g::FailureCode::NotApplied);
                    fake_compose_reply(
                        &mut peer,
                        Resp::ComposeOperationState {
                            id,
                            operation: failed,
                        },
                    )
                    .await;
                } else {
                    fake_compose_reply(
                        &mut peer,
                        Resp::ComposeRejected {
                            id,
                            code: g::RejectCode::UnknownOperation,
                            rule: None,
                        },
                    )
                    .await;
                }
                status(&mut peer, Some(1)).await;
                let queried = unknown(&mut peer).await;
                status(&mut peer, Some(1)).await;
                let (id, op, kind, desired) = match fake_compose_read(&mut peer).await {
                    Req::ComposeDown {
                        id, operation_id, ..
                    } if down => (id, operation_id, g::OperationKind::Down, g::Desired::Down),
                    Req::ComposeStop {
                        id, operation_id, ..
                    } if !down => (
                        id,
                        operation_id,
                        g::OperationKind::Stop,
                        g::Desired::Stopped,
                    ),
                    _ => panic!("fresh cleanup"),
                };
                assert_eq!(op, queried);
                fake_compose_reply(
                    &mut peer,
                    Resp::ComposeOperationState {
                        id,
                        operation: g::OperationView {
                            operation_id: op,
                            kind,
                            revision: 1,
                            ..journal
                        },
                    },
                )
                .await;
                let Req::ComposeStatus { id, project_id } = fake_compose_read(&mut peer).await
                else {
                    panic!("cleanup settlement")
                };
                assert_eq!(project_id, PROJECT);
                fake_compose_reply(
                    &mut peer,
                    Resp::ComposeProjectStatus {
                        id,
                        project: g::ProjectView {
                            desired,
                            services: Vec::new(),
                            ..view(1)
                        },
                    },
                )
                .await;
                return;
            }
            for proof in [false, true] {
                let Req::ComposeOperation { id, operation_id } = fake_compose_read(&mut peer).await
                else {
                    panic!("retired deploy lookup")
                };
                assert_eq!(operation_id, journal.operation_id);
                let reply = if proof {
                    Resp::ComposeOperationState {
                        id,
                        operation: journal.clone(),
                    }
                } else {
                    Resp::ComposeRejected {
                        id,
                        code: g::RejectCode::StoreFailure,
                        rule: None,
                    }
                };
                fake_compose_reply(&mut peer, reply).await;
                if proof {
                    status(&mut peer, Some(revision)).await;
                }
            }
            let queried = unknown(&mut peer).await;
            status(&mut peer, Some(revision)).await;
            let Req::ComposeStart {
                id,
                operation_id,
                project_id,
            } = fake_compose_read(&mut peer).await
            else {
                panic!("fresh Start after repair")
            };
            assert_eq!(operation_id, queried);
            fake_compose_reply(
                &mut peer,
                Resp::ComposeOperationState {
                    id,
                    operation: g::OperationView {
                        operation_id,
                        project_id,
                        kind: g::OperationKind::Start,
                        revision,
                        ..journal
                    },
                },
            )
            .await;
            status(&mut peer, Some(revision)).await;
        });
        let channel =
            crate::vm::guest::ComposeChannel::negotiate(host, boot + 1, &h.supervisor().cancel)
                .await
                .unwrap();
        h.supervisor()
            .compose
            .attach(store, vm_id, compose::Session::new(generation, channel))
            .unwrap();
        let fresh = json!({"vm_id":vm,"project_id":PROJECT,"request_id":key(3030)});
        if let Some((ambiguous, down)) = cleanup {
            let tool = if down { "compose_down" } else { "compose_stop" };
            assert_eq!(call(&a, tool, fresh.clone()).await["state"], "pending");
            idle(&h).await;
            assert_eq!(call(&a, tool, fresh).await["state"], "succeeded");
            let retired = call(&a, "compose_deploy", args(2)).await;
            assert_eq!(
                retired["state"],
                if ambiguous {
                    "retired_boot"
                } else {
                    "not_applied"
                }
            );
            second.await.unwrap();
            drop(a);
            h.shutdown().await.unwrap();
            return;
        }
        for expected in [1, revision] {
            assert_eq!(
                call(&a, "compose_start", fresh.clone()).await["reason"],
                "compose_recovery_required"
            );
            idle(&h).await;
            assert_eq!(db_status_revision(store, &target), Some(expected));
        }
        assert_eq!(
            call(&a, "compose_deploy", args(2)).await["state"],
            "retired_boot"
        );
        assert_eq!(
            call(&a, "compose_start", fresh.clone()).await["state"],
            "pending"
        );
        idle(&h).await;
        assert_eq!(call(&a, "compose_start", fresh).await["state"], "succeeded");
        second.await.unwrap();
        drop(a);
        h.shutdown().await.unwrap();
        fn db_status_revision(
            store: &crate::store::vm::VmStore,
            target: &compose::Target,
        ) -> Option<u64> {
            store
                .transact(|db| db::status(db, "alpha", target))
                .unwrap()
                .unwrap()
                .revision
        }
    }

    #[tokio::test]
    async fn compose_first_deploy_residue_cleanup() {
        for restart in [None, Some(false), Some(true)] {
            for down in [false, true] {
                first_deploy_residue(restart, down).await;
            }
        }
    }

    // None is a same-boot terminal failure. After restart a journal may be
    // trimmed (false) or ambiguous (true). Each cleanup is a fresh SDK call.
    async fn first_deploy_residue(restart: Option<bool>, down: bool) {
        use crate::store::vm::compose as db;
        use arx_guest::compose::{self as g, ComposeRequest as Req, ComposeResponse as Resp};
        use tokio::io::DuplexStream;
        const PROJECT: &str = "first-same-boot";
        async fn idle(h: &Harness) {
            until(|| {
                h.supervisor()
                    .gate
                    .lock()
                    .unwrap()
                    .compose_active
                    .lock()
                    .unwrap()
                    .is_empty()
            })
            .await;
        }
        let mut h = Harness::start_with_web(false, false, true).await;
        let a = h.client(0).await;
        let (vm, _) = enrolled(&h, &a, 4000).await;
        let vm_id = parse_id(vm.as_str().unwrap()).unwrap();
        let store = h.supervisor().controller.compose_store();
        let rel_id = [0x61_u8; 16];
        let release: compose::ReleaseInput = serde_json::from_value(json!({
            "vm_id": vm,
            "release_id": hex::encode(rel_id),
            "manifest": {
                "schema": g::MANIFEST_SCHEMA,
                "project_id": PROJECT,
                "volumes": [],
                "secrets": [],
                "services": [{
                    "name": "api",
                    "image": format!("example.invalid/api@sha256:{}", hex::encode([0x61_u8; 32])),
                    "platform": g::PLATFORM,
                    "uid": 10001,
                    "gid": 10001,
                    "read_only_rootfs": true,
                    "environment": [],
                    "mounts": [],
                    "secrets": [],
                    "limits": {"cpu_millis": 500, "memory_mib": 512, "pids": 32}
                }]
            }
        }))
        .unwrap();
        let enrolled_rel = release.enroll().unwrap();
        let target = enrolled_rel.target.clone();
        db::enroll(store, &[enrolled_rel]).unwrap();
        let generation = store
            .transact(|tx| db::boot(tx, "alpha", &target))
            .unwrap()
            .unwrap();
        let boot = 11_u64;
        let (host, mut peer) = tokio::io::duplex(65536);

        fn project(desired: g::Desired, residue: bool, revision: Option<u64>) -> g::ProjectView {
            g::ProjectView {
                project_id: PROJECT.into(),
                desired,
                committed_revision: revision,
                previous_revision: None,
                observation: g::Observation::Live,
                recovery_required: residue,
                running_operation: None,
                services: if residue || revision.is_some() {
                    vec![g::ServiceView {
                        name: "api".into(),
                        revision: revision.or(Some(1)),
                        state: g::ServiceState::Running,
                        health: g::HealthState::None,
                    }]
                } else {
                    vec![]
                },
            }
        }
        async fn status(peer: &mut DuplexStream, view: Option<g::ProjectView>) {
            let Req::ComposeStatus { id, project_id } = fake_compose_read(peer).await else {
                panic!("status")
            };
            assert_eq!(project_id, PROJECT);
            fake_compose_reply(
                peer,
                match view {
                    Some(project) => Resp::ComposeProjectStatus { id, project },
                    None => Resp::ComposeRejected {
                        id,
                        code: g::RejectCode::UnknownProject,
                        rule: None,
                    },
                },
            )
            .await;
        }
        async fn unknown(peer: &mut DuplexStream) -> String {
            let Req::ComposeOperation { id, operation_id } = fake_compose_read(peer).await else {
                panic!("operation lookup")
            };
            fake_compose_reply(
                peer,
                Resp::ComposeRejected {
                    id,
                    code: g::RejectCode::UnknownOperation,
                    rule: None,
                },
            )
            .await;
            operation_id
        }
        async fn deploy(peer: &mut DuplexStream) -> (u32, g::OperationView) {
            let Req::ComposeStageBegin {
                id,
                digest,
                total_bytes,
                chunks,
            } = fake_compose_read(peer).await
            else {
                panic!("stage begin")
            };
            fake_compose_reply(
                peer,
                Resp::ComposeStageProgress {
                    id,
                    digest: digest.clone(),
                    received_bytes: total_bytes,
                    next_index: chunks,
                },
            )
            .await;
            let Req::ComposeStageCommit { id, digest } = fake_compose_read(peer).await else {
                panic!("stage commit")
            };
            fake_compose_reply(
                peer,
                Resp::ComposeReleaseStaged {
                    id,
                    digest,
                    project_id: PROJECT.into(),
                },
            )
            .await;
            let Req::ComposeDeploy {
                id,
                operation_id,
                revision,
                ..
            } = fake_compose_read(peer).await
            else {
                panic!("deploy")
            };

            (
                id,
                g::OperationView {
                    operation_id,
                    kind: g::OperationKind::Deploy,
                    project_id: PROJECT.into(),
                    revision,
                    state: g::OperationState::Succeeded,
                    stage: g::OperationStage::Settled,
                    failure: None,
                    restored_previous: false,
                },
            )
        }
        let first = tokio::spawn(async move {
            fake_compose_hello(&mut peer, boot).await;
            let key = unknown(&mut peer).await;
            status(&mut peer, None).await;
            let (id, mut op) = deploy(&mut peer).await;
            assert_eq!(op.operation_id, key);
            assert_eq!(op.revision, 1);
            op.state = g::OperationState::Failed;
            op.failure = Some(g::FailureCode::UpFailed);
            if restart.is_some() {
                return (None, op);
            }
            fake_compose_reply(
                &mut peer,
                Resp::ComposeOperationState {
                    id,
                    operation: op.clone(),
                },
            )
            .await;
            (Some(peer), op)
        });
        let channel =
            crate::vm::guest::ComposeChannel::negotiate(host, boot, &h.supervisor().cancel)
                .await
                .unwrap();
        h.supervisor()
            .compose
            .attach(store, vm_id, compose::Session::new(generation, channel))
            .unwrap();
        let deploy_args = |n| json!({"vm_id":vm,"project_id":PROJECT,"release_id":hex::encode(rel_id),"request_id":key(n)});
        assert_eq!(
            call(&a, "compose_deploy", deploy_args(4010)).await["state"],
            "pending"
        );
        let (peer, op) = first.await.unwrap();
        idle(&h).await;
        let (a, mut peer) = if let Some(ambiguous) = restart {
            let stopped = call(&a, "vm_stop", json!({"vm_id":vm,"request_id":key(4011)})).await;
            assert_eq!(terminal(&a, &stopped).await["state"], "succeeded");
            drop(a);
            h.shutdown().await.unwrap();
            h = h.reopen(|_| {}).await;
            let a = h.client(0).await;
            let started = call(&a, "vm_start", json!({"vm_id":vm,"request_id":key(4012)})).await;
            assert_eq!(
                terminal_static_start(&a, &started).await["state"],
                "succeeded"
            );
            let (host, mut peer) = tokio::io::duplex(65536);
            let hello = tokio::spawn(async move {
                fake_compose_hello(&mut peer, boot + 1).await;
                peer
            });
            let channel =
                crate::vm::guest::ComposeChannel::negotiate(host, boot + 1, &h.supervisor().cancel)
                    .await
                    .unwrap();
            h.supervisor()
                .compose
                .attach(
                    h.supervisor().controller.compose_store(),
                    vm_id,
                    compose::Session::new(generation, channel),
                )
                .unwrap();
            let mut peer = hello.await.unwrap();
            let recovery = tokio::spawn(async move {
                if ambiguous {
                    let Req::ComposeOperation { id, operation_id } =
                        fake_compose_read(&mut peer).await
                    else {
                        panic!("retired lookup")
                    };
                    assert_eq!(operation_id, op.operation_id);
                    let queued = g::OperationView {
                        state: g::OperationState::Ambiguous,
                        stage: g::OperationStage::Queued,
                        failure: None,
                        ..op.clone()
                    };
                    fake_compose_reply(
                        &mut peer,
                        Resp::ComposeOperationState {
                            id,
                            operation: queued,
                        },
                    )
                    .await;
                    let Req::ComposeReconcile { id, operation_id } =
                        fake_compose_read(&mut peer).await
                    else {
                        panic!("retired reconcile")
                    };
                    assert_eq!(operation_id, op.operation_id);
                    fake_compose_reply(
                        &mut peer,
                        Resp::ComposeOperationState {
                            id,
                            operation: g::OperationView {
                                failure: Some(g::FailureCode::NotApplied),
                                ..op
                            },
                        },
                    )
                    .await;
                } else {
                    assert_eq!(unknown(&mut peer).await, op.operation_id);
                }
                status(&mut peer, Some(project(g::Desired::None, true, None))).await;
                peer
            });
            (a, recovery)
        } else {
            (a, tokio::spawn(async move { peer.unwrap() }))
        };
        let cleanup = if down { "compose_down" } else { "compose_stop" };
        let args = json!({"vm_id":vm,"project_id":PROJECT,"request_id":key(4013)});
        assert_eq!(call(&a, cleanup, args.clone()).await["state"], "pending");
        let mut peer = (&mut peer).await.unwrap();
        let cleanup_id =
            tokio::time::timeout(std::time::Duration::from_secs(8), unknown(&mut peer))
                .await
                .expect("cleanup lookup");
        status(&mut peer, Some(project(g::Desired::None, true, None))).await;
        let (id, operation_id, kind, desired) = match fake_compose_read(&mut peer).await {
            Req::ComposeDown {
                id, operation_id, ..
            } if down => (id, operation_id, g::OperationKind::Down, g::Desired::Down),
            Req::ComposeStop {
                id, operation_id, ..
            } if !down => (
                id,
                operation_id,
                g::OperationKind::Stop,
                g::Desired::Stopped,
            ),
            _ => panic!("uncommitted cleanup"),
        };
        assert_eq!(operation_id, cleanup_id);
        let projection = json!({"vm_id":vm,"project_id":PROJECT});
        let pending = call(&a, "compose_status", projection.clone()).await;
        assert!(pending["recovery"]["operation_id"].is_string());
        fake_compose_reply(
            &mut peer,
            Resp::ComposeOperationState {
                id,
                operation: g::OperationView {
                    operation_id,
                    kind,
                    project_id: PROJECT.into(),
                    revision: 1,
                    state: g::OperationState::Succeeded,
                    stage: g::OperationStage::Settled,
                    failure: None,
                    restored_previous: false,
                },
            },
        )
        .await;
        status(&mut peer, Some(project(desired, false, None))).await;
        idle(&h).await;
        assert_eq!(call(&a, cleanup, args).await["state"], "succeeded");
        let clean = call(&a, "compose_status", projection).await;
        assert!(clean["revision"].is_null() && clean["recovery"].is_null());
        // Exercise admission after the failed Deploy and its cleanup have both
        // aged out. The project retains the rendered binding independently.
        h.supervisor()
            .controller
            .compose_store()
            .transact(|tx| {
                tx.execute_batch("SAVEPOINT after_expiry")?;
                let later = UTC + crate::vm::RETRY_WINDOW_MS + 1;
                let request = compose::Mutation {
                    kind: compose::Kind::Down,
                    release: None,
                    target: target.clone(),
                    key: RequestId::parse(&format!("{later:016x}{:016x}", 4020)).unwrap(),
                };
                let cleanup = db::admit(
                    tx,
                    "alpha",
                    &request,
                    generation,
                    if restart.is_some() { boot + 1 } else { boot },
                    later,
                )?
                .unwrap();
                assert_eq!((cleanup.release, cleanup.revision), (rel_id, 1));
                assert!(
                    db::replay(tx, "alpha", RequestId::parse(&key(4010)).unwrap().key)?.is_none()
                );
                db::mark_sent(tx, &cleanup)?;
                tx.execute(
                    "UPDATE vm_compose_operations SET revision=revision+1 WHERE id=?1",
                    [cleanup.id],
                )?;
                assert!(db::bind_previous_render(tx, &cleanup, cleanup.revision + 1).is_err());
                db::bind_previous_render(tx, &cleanup, cleanup.revision)?;
                assert_eq!(
                    db::replay(tx, "alpha", request.key.key)?.unwrap().revision,
                    cleanup.revision
                );
                tx.execute_batch("ROLLBACK TO after_expiry; RELEASE after_expiry")?;
                Ok(())
            })
            .unwrap();
        assert_eq!(
            call(&a, "compose_deploy", deploy_args(4010)).await["state"],
            if restart.is_some() {
                "retired_boot"
            } else {
                "failed"
            }
        );
        assert_eq!(
            call(&a, "compose_deploy", deploy_args(4014)).await["state"],
            "pending"
        );
        let fresh = unknown(&mut peer).await;
        status(&mut peer, Some(project(desired, false, None))).await;
        let (id, receipt) = deploy(&mut peer).await;
        assert_eq!(receipt.operation_id, fresh);
        assert_eq!(receipt.revision, 2);
        fake_compose_reply(
            &mut peer,
            Resp::ComposeOperationState {
                id,
                operation: receipt,
            },
        )
        .await;
        status(
            &mut peer,
            Some(project(g::Desired::Started, false, Some(2))),
        )
        .await;
        idle(&h).await;
        assert_eq!(
            call(&a, "compose_deploy", deploy_args(4014)).await["state"],
            "succeeded"
        );
        drop(a);
        h.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn compose_secret_replay_proves_only_known_exact_delivery() {
        use crate::vm::compose_secrets::{Delivery, FailedDelivery, SecretSource};
        use arx_guest::compose::{self as g, ComposeRequest as Req, ComposeResponse as Resp};
        use std::sync::atomic::{AtomicUsize, Ordering};
        struct Source(AtomicUsize);
        impl SecretSource for Source {
            fn open(&self, reference: Id) -> Result<zeroize::Zeroizing<Vec<u8>>> {
                assert_eq!(reference, [8; 16]);
                self.0.fetch_add(1, Ordering::SeqCst);
                Ok(zeroize::Zeroizing::new(vec![42]))
            }
        }
        let boot = 19;
        let intent = compose::Intent {
            id: [1; 16],
            target: compose::Target {
                vm: [2; 16],
                project: "api".into(),
            },
            generation: [3; 16],
            boot,
            kind: compose::Kind::Deploy,
            release: [4; 16],
            revision: 1,
            sent: false,
            closed: None,
            receipt: None,
            secrets: vec![],
            secret_sent: 1,
            digest: "a".repeat(64),
            bytes: vec![],
        };
        let secret = compose::SecretRef {
            slot: "password".into(),
            epoch: 7,
            reference: [8; 16],
        };
        let delivery = Delivery::new(&intent, &secret, None);
        for case in [
            "unknown",
            "wrong_project",
            "failed",
            "after_write",
            "after_settle",
            "missing_file",
        ] {
            let (host, mut peer) = tokio::io::duplex(65536);
            let cancel = crate::service::Shutdown::default();
            let task = tokio::spawn(async move {
                fake_compose_hello(&mut peer, boot).await;
                let Req::ComposeOperation { id, operation_id } = fake_compose_read(&mut peer).await
                else {
                    panic!("lookup first")
                };
                let mut operation = g::OperationView {
                    operation_id: operation_id.clone(),
                    project_id: if case == "wrong_project" {
                        "another"
                    } else {
                        "api"
                    }
                    .into(),
                    kind: g::OperationKind::SecretPut,
                    revision: 0,
                    stage: g::OperationStage::Applying,
                    state: g::OperationState::Ambiguous,
                    failure: None,
                    restored_previous: false,
                };
                if matches!(case, "failed" | "after_settle") {
                    operation.stage = g::OperationStage::Settled;
                    operation.state = if case == "failed" {
                        operation.failure = Some(g::FailureCode::NotApplied);
                        g::OperationState::Failed
                    } else {
                        g::OperationState::Succeeded
                    };
                }
                let reply = if case == "unknown" {
                    Resp::ComposeRejected {
                        id,
                        code: g::RejectCode::UnknownOperation,
                        rule: None,
                    }
                } else {
                    Resp::ComposeOperationState {
                        id,
                        operation: operation.clone(),
                    }
                };
                fake_compose_reply(&mut peer, reply).await;
                if matches!(case, "after_write" | "after_settle" | "missing_file") {
                    let Req::ComposeSecretPut {
                        id,
                        operation_id: echo,
                        project_id,
                        digest,
                        generation,
                        slot,
                        epoch,
                        data,
                    } = fake_compose_read(&mut peer).await
                    else {
                        panic!("exact compare")
                    };
                    assert_eq!(echo, operation_id);
                    assert_eq!(
                        (
                            project_id.as_str(),
                            digest.as_str(),
                            generation,
                            slot.as_str(),
                            epoch
                        ),
                        ("api", "a".repeat(64).as_str(), boot, "password", 7)
                    );
                    assert!(data == g::SecretText::new(g::encode_base64(&[42])));
                    let response = if case == "missing_file" {
                        operation.stage = g::OperationStage::Settled;
                        operation.state = g::OperationState::Failed;
                        operation.failure = Some(g::FailureCode::NotApplied);
                        Resp::ComposeOperationState { id, operation }
                    } else {
                        Resp::ComposeSecretAccepted {
                            id,
                            project_id,
                            digest,
                            slot,
                            epoch,
                            recreate: None,
                        }
                    };
                    fake_compose_reply(&mut peer, response).await;
                }
            });
            let mut channel = crate::vm::guest::ComposeChannel::negotiate(host, boot, &cancel)
                .await
                .unwrap();
            let source = Source(AtomicUsize::new(0));
            let result = delivery
                .deliver(&source, &mut channel, &cancel, true, || {
                    panic!("no new submission")
                })
                .await;
            task.await.unwrap();
            assert_eq!(
                result.is_ok(),
                matches!(case, "after_write" | "after_settle"),
                "{case}"
            );
            assert_eq!(
                result.err().is_some_and(|e| e.is::<FailedDelivery>()),
                matches!(case, "failed" | "missing_file")
            );
            assert_eq!(
                source.0.load(Ordering::SeqCst),
                usize::from(matches!(
                    case,
                    "after_write" | "after_settle" | "missing_file"
                ))
            );
        }
    }

    async fn fake_compose_hello(peer: &mut tokio::io::DuplexStream, boot: u64) {
        use arx_guest::compose::{ComposeRequest as Req, ComposeResponse as Resp};
        let Req::ComposeHello {
            version,
            challenge,
            generation,
            capabilities,
        } = fake_compose_read(peer).await
        else {
            panic!("Compose handshake required")
        };
        assert_eq!(generation, boot);
        fake_compose_reply(
            peer,
            Resp::ComposeHelloAck {
                version,
                challenge,
                generation,
                capabilities,
                cleared_staging: 0,
                ambiguous_operations: 0,
            },
        )
        .await;
    }

    async fn fake_compose_read(
        peer: &mut tokio::io::DuplexStream,
    ) -> arx_guest::compose::ComposeRequest {
        use tokio::io::AsyncReadExt;
        let size = peer.read_u32().await.unwrap() as usize;
        assert!((1..=arx_guest::protocol::MAX_FRAME).contains(&size));
        let mut frame = zeroize::Zeroizing::new(vec![0; 4 + size]);
        frame[..4].copy_from_slice(&(size as u32).to_be_bytes());
        peer.read_exact(&mut frame[4..]).await.unwrap();
        arx_guest::protocol::read_frame(&mut frame.as_slice())
            .unwrap()
            .unwrap()
    }

    async fn fake_compose_reply(
        peer: &mut tokio::io::DuplexStream,
        reply: arx_guest::compose::ComposeResponse,
    ) {
        use tokio::io::AsyncWriteExt;
        let mut bytes = Vec::new();
        arx_guest::protocol::write_frame(&mut bytes, &reply).unwrap();
        peer.write_all(&bytes).await.unwrap();
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
        assert_eq!(terminal_static_start(a, &start).await["state"], "succeeded");
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
        // Publication can report unpublished before the same turn finishes
        // fencing and reconciling VM custody.
        h.tick_and_wait().await;
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
        assert_eq!(
            terminal_static_start(&a, &start).await["state"],
            "succeeded"
        );
        let published = call(&a, "publish_site", args(812)).await;
        assert_eq!(terminal(&a, &published).await["state"], "succeeded");
        let before = intent(&h).unwrap();
        assert!(before.hosts[0].record.is_some());
        fake.credential.store(false, SeqCst);
        drop(a);
        // Local ingress is down, but the record stays: no clean drain.
        assert!(
            h.shutdown().await.is_err(),
            "a drain with DNS cleanup pending reported clean"
        );
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
                after.hosts[0].hostname.as_str(),
                &after.credential_ref,
                &after.hosts[0].record,
            ),
            (
                &before.nonce,
                before.generation,
                before.boot,
                before.revision,
                &before.zone,
                before.hosts[0].hostname.as_str(),
                &before.credential_ref,
                &before.hosts[0].record,
            )
        );
        assert_eq!(phase(&a, &vm).await["phase"], "cleanup_pending");
        let revoke = call(&a, "unpublish_site", args(813)).await;
        assert_eq!(terminal(&a, &revoke).await["state"], "failed");
        assert_eq!(phase(&a, &vm).await["phase"], "cleanup_pending");
        let start = call(&a, "vm_start", args(814)).await;
        assert_eq!(
            terminal_static_start(&a, &start).await["state"],
            "succeeded"
        );
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
        assert!(before.hosts[0].post_possible && before.hosts[0].record.is_none());
        let starts = fake.starts.load(SeqCst);
        drop(a);
        // The possible POST stays ambiguous: no clean drain.
        assert!(
            h.shutdown().await.is_err(),
            "a drain with an ambiguous POST reported clean"
        );
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
                intent.hosts[0].hostname.as_str().to_owned(),
                intent.tunnel.clone(),
                intent.credential_ref.clone(),
                intent.hosts[0].record.clone(),
                intent.hosts[0].post_possible,
            )
        };
        assert_eq!(pinned(&intent(&h).unwrap()), pinned(&before));
        let start = call(&a, "vm_start", args(817)).await;
        assert_eq!(
            terminal_static_start(&a, &start).await["state"],
            "succeeded"
        );
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
        stops.reap(&supervisor.publisher);
        stops.set.is_empty() && supervisor.publisher.stop_obligations() == 0
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
            "CREATE TRIGGER withdrawal_failure BEFORE UPDATE OF post_possible ON vm_publication_hosts
             WHEN OLD.post_possible=1 AND NEW.post_possible=0
             BEGIN SELECT RAISE(ABORT, 'synthetic withdrawal failure'); END;",
        );
        let (in_gate, release) = pause(&h);
        let pending = call(&a, "publish_site", args(1054)).await;
        entered(in_gate).await;
        let revoke = call(&a, "unpublish_site", args(1055)).await;
        release.send(()).unwrap();
        let failed = terminal(&a, &pending).await;
        // A failed withdrawal keeps the owner cause.
        assert_eq!(
            (&failed["state"], &failed["reason"], &failed["cleanup"]),
            (&json!("failed"), &json!("cancelled"), &json!("confirmed")),
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
            "CREATE TRIGGER withdrawal_failure BEFORE UPDATE OF post_possible ON vm_publication_hosts
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
        assert_eq!(h.supervisor().publisher.stop_obligations(), 1);
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
        assert_eq!(
            terminal_static_start(&a, &start).await["state"],
            "succeeded"
        );
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
    async fn owner_revocation_during_tls_setup_never_writes_the_post() {
        use std::sync::atomic::Ordering::SeqCst;
        let mut h = Harness::start_with_web(false, false, true).await;
        let a = h.client(0).await;
        let (vm, fake) = enrolled(&h, &a, 1400).await;
        let args = |n| json!({"vm_id":vm,"request_id":key(n)});
        let (tls, in_tls, _release) = gate();
        *fake.tls_gate.lock().unwrap() = Some(tls);
        let pending = call(&a, "publish_site", args(1410)).await;
        // The POST transport is setting up its connection; nothing is written.
        entered(in_tls).await;
        assert_eq!(
            call(&a, "unpublish_site", args(1410)).await["reason"],
            "key_collision"
        );
        let failed = terminal(&a, &pending).await;
        assert_eq!(
            (&failed["state"], &failed["reason"], &failed["cleanup"]),
            (&json!("failed"), &json!("cancelled"), &json!("confirmed")),
            "{failed}"
        );
        until(|| revocations_settled(h.supervisor())).await;
        assert!(!fake.local.load(SeqCst));
        assert_eq!(fake.posts.load(SeqCst), 0);
        assert_eq!(phase(&a, &vm).await["phase"], "unpublished");
        let published = call(&a, "publish_site", args(1411)).await;
        assert_eq!(terminal(&a, &published).await["state"], "succeeded");
        let revoke = call(&a, "unpublish_site", args(1412)).await;
        assert_eq!(terminal(&a, &revoke).await["state"], "succeeded");
        drop(a);
        h.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn vm_stop_during_tls_setup_releases_as_never_sent() {
        use std::sync::atomic::Ordering::SeqCst;
        let mut h = Harness::start_with_web(false, false, true).await;
        let a = h.client(0).await;
        let (vm, fake) = enrolled(&h, &a, 1420).await;
        let args = |n| json!({"vm_id":vm,"request_id":key(n)});
        let (tls, in_tls, _release) = gate();
        *fake.tls_gate.lock().unwrap() = Some(tls);
        let pending = call(&a, "publish_site", args(1430)).await;
        entered(in_tls).await;
        fake.routes.lock().unwrap().clear();
        let stop = call(&a, "vm_stop", args(1431)).await;
        assert_eq!(terminal(&a, &stop).await["state"], "succeeded");
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
        // Released as never sent: no POST and no lookup, and the slot is free.
        assert_eq!(fake.posts.load(SeqCst), 0);
        assert!(fake.routes.lock().unwrap().is_empty());
        assert_eq!(phase(&a, &vm).await["phase"], "unpublished");
        let start = call(&a, "vm_start", args(1432)).await;
        assert_eq!(
            terminal_static_start(&a, &start).await["state"],
            "succeeded"
        );
        let published = call(&a, "publish_site", args(1433)).await;
        assert_eq!(terminal(&a, &published).await["state"], "succeeded");
        let revoke = call(&a, "unpublish_site", args(1434)).await;
        assert_eq!(terminal(&a, &revoke).await["state"], "succeeded");
        drop(a);
        h.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn a_zero_byte_first_write_failure_releases_without_dns() {
        use std::sync::atomic::Ordering::SeqCst;
        let mut h = Harness::start_with_web(false, false, true).await;
        let a = h.client(0).await;
        let (vm, fake) = enrolled(&h, &a, 1480).await;
        let args = |n| json!({"vm_id":vm,"request_id":key(n)});
        fake.first_write_error.store(true, SeqCst);
        let failed = call(&a, "publish_site", args(1490)).await;
        let failed = terminal(&a, &failed).await;
        assert_eq!(
            (&failed["state"], &failed["reason"], &failed["cleanup"]),
            (
                &json!("failed"),
                &json!("dns_unavailable"),
                &json!("confirmed")
            ),
            "{failed}"
        );
        // Only the preflight lookup ran: no POST, no cleanup lookup, no delete.
        assert_eq!((fake.posts.load(SeqCst), fake.deletes.load(SeqCst)), (0, 0));
        assert_eq!(fake.routes.lock().unwrap().len(), 1);
        assert_eq!(phase(&a, &vm).await["phase"], "unpublished");
        let published = call(&a, "publish_site", args(1491)).await;
        assert_eq!(terminal(&a, &published).await["state"], "succeeded");
        let revoke = call(&a, "unpublish_site", args(1492)).await;
        assert_eq!(terminal(&a, &revoke).await["state"], "succeeded");
        drop(a);
        h.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn an_unremoved_ingress_residue_is_never_reported_as_confirmed_cleanup() {
        use std::sync::atomic::Ordering::SeqCst;
        let mut h = Harness::start_with_web(false, false, true).await;
        let a = h.client(0).await;
        let (vm, fake) = enrolled(&h, &a, 1500).await;
        let args = |n| json!({"vm_id":vm,"request_id":key(n)});
        // The connector start fails after a failed ingress-file write whose
        // removal could not be confirmed, before any process or DNS effect.
        fake.residue.store(true, SeqCst);
        let failed = call(&a, "publish_site", args(1510)).await;
        let failed = terminal(&a, &failed).await;
        assert_eq!(
            (&failed["state"], &failed["reason"], &failed["cleanup"]),
            (
                &json!("failed"),
                &json!("cleanup_unconfirmed"),
                &json!(proof_wire(false))
            ),
            "{failed}"
        );
        assert_eq!((fake.posts.load(SeqCst), fake.deletes.load(SeqCst)), (0, 0));
        assert!(!fake.local.load(SeqCst));
        // No DNS effect exists, so the intent is released. The private residue
        // is never handed to a connector, so later publication proceeds.
        assert_eq!(phase(&a, &vm).await["phase"], "unpublished");
        let published = call(&a, "publish_site", args(1511)).await;
        assert_eq!(terminal(&a, &published).await["state"], "succeeded");
        let revoke = call(&a, "unpublish_site", args(1512)).await;
        assert_eq!(terminal(&a, &revoke).await["state"], "succeeded");
        drop(a);
        assert!(
            h.shutdown().await.is_err(),
            "the drain claimed a clean settlement with an unremoved residue"
        );
    }

    #[tokio::test]
    async fn owner_revocation_is_authorized_generation_scoped_and_never_replayed() {
        use std::sync::atomic::Ordering::SeqCst;
        let mut h = Harness::start_with_web(false, false, true).await;
        let a = h.client(0).await;
        let b = h.client(1).await;
        let (vm, fake) = enrolled(&h, &a, 1700).await;
        let args = |n| json!({"vm_id":vm,"request_id":key(n)});
        let unpublishes = |h: &Harness| {
            h.supervisor()
                .controller
                .store_for_test()
                .transact(|db| {
                    Ok(db.query_row(
                        "SELECT count(*) FROM activity WHERE action=16",
                        [],
                        |row| row.get::<_, i64>(0),
                    )?)
                })
                .unwrap()
        };
        let runners_idle = |h: &Harness| {
            let supervisor = h.supervisor();
            let mut stops = supervisor.stops.lock().unwrap();
            stops.reap(&supervisor.publisher);
            stops.set.is_empty()
        };

        // A connector start parked before readiness holds the local slot.
        // Another principal neither sees nor revokes it; the owner's colliding
        // call aborts the start.
        let (start, in_start, _release) = gate();
        *fake.start_gate.lock().unwrap() = Some(start);
        let starts = fake.starts.load(SeqCst);
        let pending = call(&a, "publish_site", args(1710)).await;
        entered(in_start).await;
        assert_eq!(
            call(&b, "unpublish_site", args(1711)).await["reason"],
            "not_found"
        );
        // Recorded synchronously before the reply, so a revocation by another
        // principal would be visible here whatever its stop runner did.
        let publisher = &h.supervisor().publisher;
        assert_eq!(
            (publisher.owner_causes(), publisher.stop_obligations()),
            (0, 0),
            "another principal revoked the publication"
        );
        assert_eq!(
            call(&a, "unpublish_site", args(1710)).await["reason"],
            "key_collision"
        );
        let failed = terminal(&a, &pending).await;
        assert_eq!(
            (&failed["state"], &failed["reason"], &failed["cleanup"]),
            (&json!("failed"), &json!("cancelled"), &json!("confirmed")),
            "{failed}"
        );
        assert_eq!(fake.starts.load(SeqCst), starts + 1);
        assert!(!fake.local.load(SeqCst));
        assert_eq!(fake.posts.load(SeqCst), 0);
        until(|| revocations_settled(h.supervisor())).await;
        assert_eq!(phase(&a, &vm).await["phase"], "unpublished");

        // A failed stop stays retryable with one obligation per generation:
        // a duplicate request coalesces while it runs, and the next request
        // after the failure retries it.
        let published = call(&a, "publish_site", args(1712)).await;
        assert_eq!(terminal(&a, &published).await["state"], "succeeded");
        fake.stop_failures.store(1, SeqCst);
        let (stop, stop_entered, release_stop) = gate();
        *h.supervisor().publisher.stop_gate.lock().unwrap() = Some(stop);
        let spawned = stops_spawned(h.supervisor());
        assert_eq!(
            call(&a, "unpublish_site", args(1712)).await["reason"],
            "key_collision"
        );
        entered(stop_entered).await;
        assert_eq!(
            call(&a, "unpublish_site", args(1712)).await["reason"],
            "key_collision"
        );
        assert_eq!(
            (
                stops_spawned(h.supervisor()),
                h.supervisor().publisher.stop_obligations()
            ),
            (spawned + 1, 1)
        );
        release_stop.send(()).unwrap();
        until(|| h.supervisor().publisher.stop_unconfirmed() && runners_idle(&h)).await;
        assert!(fake.local.load(SeqCst));
        assert_eq!(h.supervisor().publisher.stop_obligations(), 1);
        assert_eq!(
            call(&a, "unpublish_site", args(1712)).await["reason"],
            "key_collision"
        );
        until(|| !fake.local.load(SeqCst)).await;
        until(|| revocations_settled(h.supervisor())).await;
        assert_eq!(stops_spawned(h.supervisor()), spawned + 2);
        assert!(!h.supervisor().publisher.stop_unconfirmed());
        assert_eq!(settle(&h, &a, &vm).await["reason"], "cancelled");

        // Generation n ends through an admitted owner unpublish.
        let old = call(&a, "publish_site", args(1713)).await;
        assert_eq!(terminal(&a, &old).await["state"], "succeeded");
        let first = call(&a, "unpublish_site", args(1714)).await;
        assert_eq!(terminal(&a, &first).await["state"], "succeeded");
        // Generation n+1 is live.
        let published = call(&a, "publish_site", args(1715)).await;
        assert_eq!(terminal(&a, &published).await["state"], "succeeded");
        assert!(fake.local.load(SeqCst));
        let (starts, receipts) = (fake.starts.load(SeqCst), unpublishes(&h));
        // Replaying the old unpublish returns its receipt and admits nothing.
        // The newly delivered authenticated owner call still revokes the live
        // generation as a safety revoke.
        let replayed = call(&a, "unpublish_site", args(1714)).await;
        assert_eq!(
            replayed["operation_id"], first["operation_id"],
            "{replayed}"
        );
        until(|| !fake.local.load(SeqCst)).await;
        assert_eq!(unpublishes(&h), receipts);
        assert_eq!(phase(&a, &vm).await["phase"], "published");
        assert_eq!(settle(&h, &a, &vm).await["reason"], "cancelled");
        // Replaying the old publish recreates no effect.
        let replayed = call(&a, "publish_site", args(1713)).await;
        assert_eq!(replayed["operation_id"], old["operation_id"], "{replayed}");
        assert_eq!(fake.starts.load(SeqCst), starts);
        assert!(!fake.local.load(SeqCst));
        assert_eq!(phase(&a, &vm).await["phase"], "unpublished");
        drop((a, b));
        h.shutdown().await.unwrap();
    }

    /// The latest release audit's publication cause.
    fn release_cause(h: &Harness) -> Option<i64> {
        h.supervisor()
            .controller
            .store_for_test()
            .transact(|db| {
                Ok(db.query_row(
                    "SELECT publication_failure FROM activity WHERE action=17
                     ORDER BY id DESC LIMIT 1",
                    [],
                    |row| row.get::<_, Option<i64>>(0),
                )?)
            })
            .unwrap()
    }

    #[tokio::test]
    async fn the_owner_cause_survives_post_errors_checkpoint_outages_and_shutdown() {
        use std::sync::atomic::Ordering::SeqCst;
        let cancelled = Some(crate::vm::FailureReason::Cancelled as i64);
        let mut h = Harness::start_with_web(false, false, true).await;
        let a = h.client(0).await;
        let (vm, fake) = enrolled(&h, &a, 1800).await;
        let args = |n| json!({"vm_id":vm,"request_id":key(n)});
        let host = h.supervisor().host_id;

        // The owner revokes after the POST's first byte and the POST then
        // fails. The receipt keeps the owner cause over the DNS failure, and
        // the attributed record is removed.
        let (post, in_post, release_post) = gate();
        *fake.post_gate.lock().unwrap() = Some(post);
        let pending = call(&a, "publish_site", args(1810)).await;
        entered(in_post).await;
        assert_eq!(
            call(&a, "unpublish_site", args(1810)).await["reason"],
            "key_collision"
        );
        until(|| !fake.local.load(SeqCst)).await;
        drop(release_post);
        let failed = terminal(&a, &pending).await;
        assert_eq!(
            (&failed["state"], &failed["reason"], &failed["cleanup"]),
            (&json!("failed"), &json!("cancelled"), &json!("confirmed")),
            "{failed}"
        );
        assert_eq!((fake.posts.load(SeqCst), fake.deletes.load(SeqCst)), (1, 1));
        let status = phase(&a, &vm).await;
        assert_eq!(
            (&status["phase"], &status["reason"]),
            (&json!("unpublished"), &json!("cancelled")),
            "{status}"
        );

        // The owner revokes before connector start and the `revoking`
        // checkpoint fails. SQLite recovers and shutdown runs before any
        // reaper turn: the final reconciliation still records the owner cause
        // on the release, its audit and the receipt.
        let (worker, in_worker, release_worker) = gate();
        *h.supervisor().publisher.activate_gate.lock().unwrap() = Some(worker);
        let pending = call(&a, "publish_site", args(1811)).await;
        entered(in_worker).await;
        assert_eq!(
            call(&a, "unpublish_site", args(1811)).await["reason"],
            "key_collision"
        );
        until(|| revocations_settled(h.supervisor())).await;
        trigger(
            &h,
            "CREATE TRIGGER refuse_revoking BEFORE UPDATE OF phase ON vm_publication
             WHEN NEW.phase='revoking'
             BEGIN SELECT RAISE(ABORT, 'synthetic checkpoint failure'); END;",
        );
        release_worker.send(()).unwrap();
        until(|| h.supervisor().publisher.workers().is_empty()).await;
        assert_eq!(phase(&a, &vm).await["phase"], "publishing");
        trigger(&h, "DROP TRIGGER refuse_revoking");
        drop(a);
        h.shutdown().await.unwrap();
        let mut h = h.reopen(|_| {}).await;
        enroll(&h, &fake, true, Profile::synthetic(host, "tunnel-site"));
        let a = h.client(0).await;
        let failed = terminal(&a, &pending).await;
        assert_eq!(
            (&failed["state"], &failed["reason"]),
            (&json!("failed"), &json!("cancelled")),
            "{failed}"
        );
        let status = phase(&a, &vm).await;
        assert_eq!(
            (&status["phase"], &status["reason"]),
            (&json!("unpublished"), &json!("cancelled")),
            "{status}"
        );
        assert_eq!(release_cause(&h), cancelled);

        // The owner revokes between the possible-POST marker and submission,
        // and the withdrawal and the release both fail. SQLite recovers and
        // shutdown runs before any reaper turn: the in-memory no-submission
        // proof releases the intent without DNS, with the owner cause.
        let start = call(&a, "vm_start", args(1812)).await;
        assert_eq!(
            terminal_static_start(&a, &start).await["state"],
            "succeeded"
        );
        let (marker, in_marker, release_marker) = gate();
        *h.supervisor().publisher.marker_gate.lock().unwrap() = Some(marker);
        let posts = fake.posts.load(SeqCst);
        let pending = call(&a, "publish_site", args(1813)).await;
        entered(in_marker).await;
        assert_eq!(
            call(&a, "unpublish_site", args(1813)).await["reason"],
            "key_collision"
        );
        trigger(
            &h,
            "CREATE TRIGGER withdrawal_failure BEFORE UPDATE OF post_possible ON vm_publication_hosts
             WHEN OLD.post_possible=1 AND NEW.post_possible=0
             BEGIN SELECT RAISE(ABORT, 'synthetic withdrawal failure'); END;
             CREATE TRIGGER release_failure BEFORE DELETE ON vm_publication
             BEGIN SELECT RAISE(ABORT, 'synthetic release failure'); END;",
        );
        release_marker.send(()).unwrap();
        until(|| h.supervisor().publisher.workers().is_empty()).await;
        assert_eq!(phase(&a, &vm).await["phase"], "revoking");
        trigger(
            &h,
            "DROP TRIGGER withdrawal_failure; DROP TRIGGER release_failure;",
        );
        fake.routes.lock().unwrap().clear();
        drop(a);
        h.shutdown().await.unwrap();
        assert!(fake.routes.lock().unwrap().is_empty(), "DNS consulted");
        let mut h = h.reopen(|_| {}).await;
        enroll(&h, &fake, true, Profile::synthetic(host, "tunnel-site"));
        let a = h.client(0).await;
        let failed = terminal(&a, &pending).await;
        assert_eq!(
            (&failed["state"], &failed["reason"]),
            (&json!("failed"), &json!("cancelled")),
            "{failed}"
        );
        let status = phase(&a, &vm).await;
        assert_eq!(
            (&status["phase"], &status["reason"]),
            (&json!("unpublished"), &json!("cancelled")),
            "{status}"
        );
        assert_eq!(release_cause(&h), cancelled);
        assert_eq!(fake.posts.load(SeqCst), posts);
        drop(a);
        h.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn a_later_admitted_unpublish_never_erases_a_durable_owner_cause() {
        use std::sync::atomic::Ordering::SeqCst;
        let cancelled = Some(crate::vm::FailureReason::Cancelled as i64);
        let mut h = Harness::start_with_web(false, false, true).await;
        let a = h.client(0).await;
        let (vm, fake) = enrolled(&h, &a, 2000).await;
        let args = |n| json!({"vm_id":vm,"request_id":key(n)});
        let host = h.supervisor().host_id;
        let published = call(&a, "publish_site", args(2010)).await;
        assert_eq!(terminal(&a, &published).await["state"], "succeeded");
        // The owner's admitted unpublish leaves DNS cleanup pending.
        fake.credential.store(false, SeqCst);
        let first = call(&a, "unpublish_site", args(2011)).await;
        assert_eq!(terminal(&a, &first).await["state"], "failed");
        let status = phase(&a, &vm).await;
        assert_eq!(
            (&status["phase"], &status["reason"]),
            (&json!("cleanup_pending"), &json!("cancelled")),
            "{status}"
        );
        drop(a);
        assert!(h.shutdown().await.is_err());
        // A new process holds no in-memory owner cause. A second admitted
        // unpublish writes a null reason and its cleanup a DNS failure; the
        // durable owner cause survives both.
        let mut h = h.reopen(|_| {}).await;
        enroll(&h, &fake, true, Profile::synthetic(host, "tunnel-site"));
        let a = h.client(0).await;
        let second = call(&a, "unpublish_site", args(2012)).await;
        assert_eq!(terminal(&a, &second).await["state"], "failed");
        let status = phase(&a, &vm).await;
        assert_eq!(
            (&status["phase"], &status["reason"]),
            (&json!("cleanup_pending"), &json!("cancelled")),
            "{status}"
        );
        fake.credential.store(true, SeqCst);
        assert_eq!(settle(&h, &a, &vm).await["reason"], "cancelled");
        assert_eq!(release_cause(&h), cancelled);
        assert_eq!(fake.deletes.load(SeqCst), 1);
        drop(a);
        h.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn a_clean_foreground_drain_requires_full_publication_settlement() {
        use std::sync::atomic::Ordering::SeqCst;
        let mut h = Harness::start_with_web(false, false, true).await;
        let a = h.client(0).await;
        let (vm, fake) = enrolled(&h, &a, 1900).await;
        let args = |n| json!({"vm_id":vm,"request_id":key(n)});
        let host = h.supervisor().host_id;
        let intent = |h: &Harness| h.supervisor().controller.publication_intent().unwrap();
        let refuse_commit = "CREATE TRIGGER refuse_commit BEFORE UPDATE OF phase ON vm_publication
             WHEN NEW.phase='published'
             BEGIN SELECT RAISE(ABORT, 'synthetic publish commit failure'); END;";

        // A handled worker error is superseded by a confirmed reconciliation:
        // the publish commit fails after DNS ID custody, and the final turn
        // removes the record.
        trigger(&h, refuse_commit);
        let pending = call(&a, "publish_site", args(1910)).await;
        assert_eq!(pending["ok"], true, "{pending}");
        until(|| fake.posts.load(SeqCst) == 1).await;
        until(|| h.supervisor().publisher.workers().is_empty()).await;
        assert!(intent(&h).unwrap().hosts[0].record.is_some());
        trigger(&h, "DROP TRIGGER refuse_commit");
        drop(a);
        h.shutdown().await.unwrap();
        assert_eq!(fake.deletes.load(SeqCst), 1);
        assert!(!fake.local.load(SeqCst));

        // The same failed terminal write with the DNS credential gone: local
        // ingress stops, the intent and its record stay, and the drain is
        // unconfirmed. Restart cleanup removes the record once the credential
        // returns.
        let mut h = h.reopen(|_| {}).await;
        enroll(&h, &fake, true, Profile::synthetic(host, "tunnel-site"));
        let a = h.client(0).await;
        let start = call(&a, "vm_start", args(1911)).await;
        assert_eq!(
            terminal_static_start(&a, &start).await["state"],
            "succeeded"
        );
        trigger(&h, refuse_commit);
        let pending = call(&a, "publish_site", args(1912)).await;
        assert_eq!(pending["ok"], true, "{pending}");
        until(|| fake.posts.load(SeqCst) == 2).await;
        until(|| h.supervisor().publisher.workers().is_empty()).await;
        trigger(&h, "DROP TRIGGER refuse_commit");
        let before = intent(&h).unwrap();
        assert!(before.hosts[0].record.is_some());
        fake.credential.store(false, SeqCst);
        drop(a);
        assert!(
            h.shutdown().await.is_err(),
            "a drain with DNS cleanup pending reported clean"
        );
        assert!(!fake.local.load(SeqCst));
        assert!(fake.record.load(SeqCst));
        let mut h = h.reopen(|_| {}).await;
        enroll(&h, &fake, true, Profile::synthetic(host, "tunnel-site"));
        let after = intent(&h).unwrap();
        assert_eq!(
            (&after.nonce, &after.hosts[0].record),
            (&before.nonce, &before.hosts[0].record)
        );
        fake.credential.store(true, SeqCst);
        let a = h.client(0).await;
        settle(&h, &a, &vm).await;
        assert_eq!(fake.deletes.load(SeqCst), 2);
        assert_ne!(terminal(&a, &pending).await["state"], "succeeded");

        // A final reconciliation parked past the drain deadline is unconfirmed
        // and keeps the intent for restart cleanup.
        let start = call(&a, "vm_start", args(1913)).await;
        assert_eq!(
            terminal_static_start(&a, &start).await["state"],
            "succeeded"
        );
        let published = call(&a, "publish_site", args(1914)).await;
        assert_eq!(terminal(&a, &published).await["state"], "succeeded");
        let (remove, _in_remove, _held) = gate();
        *fake.remove_gate.lock().unwrap() = Some(remove);
        drop(a);
        assert!(
            h.shutdown().await.is_err(),
            "a final reconciliation past its deadline reported clean"
        );
        assert!(!fake.local.load(SeqCst));
        assert!(fake.record.load(SeqCst));
        let mut h = h.reopen(|_| {}).await;
        enroll(&h, &fake, true, Profile::synthetic(host, "tunnel-site"));
        assert!(intent(&h).is_some());
        let a = h.client(0).await;
        settle(&h, &a, &vm).await;
        assert_eq!(fake.deletes.load(SeqCst), 3);

        // A worker panics after its POST's first byte. Another publication
        // call harvests the panic and its cleanup settles the publication,
        // yet the drain stays unconfirmed.
        let start = call(&a, "vm_start", args(1915)).await;
        let started = terminal_static_start(&a, &start).await;
        assert_settled_static_start(&h, &vm, &started);
        let posts = fake.posts.load(SeqCst);
        fake.panic_after_post.store(true, SeqCst);
        let pending = call(&a, "publish_site", args(1916)).await;
        assert_eq!(pending["ok"], true, "{pending}");
        until(|| h.supervisor().publisher.workers().is_empty()).await;
        assert_eq!(fake.posts.load(SeqCst), posts + 1);
        let revoke = call(&a, "unpublish_site", args(1917)).await;
        assert_eq!(terminal(&a, &revoke).await["state"], "succeeded");
        assert!(h.supervisor().publisher.lost());
        assert_eq!(phase(&a, &vm).await["phase"], "unpublished");
        assert!(!fake.record.load(SeqCst));
        drop(a);
        assert!(
            h.shutdown().await.is_err(),
            "a harvested worker panic disappeared from the drain"
        );

        // A deferred stop panics. Shutdown still contains the connector and
        // settles the publication, and the drain stays unconfirmed.
        let mut h = h.reopen(|_| {}).await;
        enroll(&h, &fake, true, Profile::synthetic(host, "tunnel-site"));
        let a = h.client(0).await;
        let start = call(&a, "vm_start", args(1918)).await;
        assert_eq!(
            terminal_static_start(&a, &start).await["state"],
            "succeeded"
        );
        let published = call(&a, "publish_site", args(1919)).await;
        assert_eq!(terminal(&a, &published).await["state"], "succeeded");
        fake.stop_panic.store(true, SeqCst);
        assert_eq!(
            call(&a, "unpublish_site", args(1919)).await["reason"],
            "key_collision"
        );
        until(|| h.supervisor().publisher.lost()).await;
        drop(a);
        assert!(
            h.shutdown().await.is_err(),
            "a lost stop completion disappeared from the drain"
        );
        assert!(!fake.local.load(SeqCst));
        assert!(!fake.record.load(SeqCst));
    }

    /// A start that fails after spawn hands back a process whose termination
    /// it could not confirm. The process stays in the local slot: cleanup
    /// reports it unconfirmed and closes publish admission, a retried stop
    /// settles it, and while it stays unconfirmed the drain is not quiescent
    /// and custody recovery never runs.
    #[tokio::test]
    async fn a_start_that_cannot_confirm_termination_keeps_process_custody() {
        use std::sync::atomic::Ordering::SeqCst;
        let mut h = Harness::start_with_web(false, false, true).await;
        let a = h.client(0).await;
        let (vm, fake) = enrolled(&h, &a, 2100).await;
        let args = |n| json!({"vm_id":vm,"request_id":key(n)});
        fake.start_held.store(true, SeqCst);
        fake.stop_failures.store(1, SeqCst);
        let failed = call(&a, "publish_site", args(2110)).await;
        let failed = terminal(&a, &failed).await;
        assert_eq!(
            (&failed["state"], &failed["reason"], &failed["cleanup"]),
            (
                &json!("failed"),
                &json!("cleanup_unconfirmed"),
                &json!(proof_wire(false))
            ),
            "{failed}"
        );
        assert!(fake.local.load(SeqCst), "the held process was not kept");
        assert!(h.supervisor().publisher.stop_unconfirmed());
        assert_eq!(phase(&a, &vm).await["phase"], "cleanup_pending");
        assert_eq!(
            call(&a, "publish_site", args(2111)).await["reason"],
            "state_unavailable"
        );
        // The next reaper turn retries the stop and settles the publication.
        let status = settle(&h, &a, &vm).await;
        assert_eq!(status["reason"], "cleanup_unconfirmed", "{status}");
        assert!(!fake.local.load(SeqCst));
        assert!(!h.supervisor().publisher.stop_unconfirmed());
        let published = call(&a, "publish_site", args(2112)).await;
        assert_eq!(terminal(&a, &published).await["state"], "succeeded");
        let revoke = call(&a, "unpublish_site", args(2113)).await;
        assert_eq!(terminal(&a, &revoke).await["state"], "succeeded");

        // A held process whose stop keeps failing bars custody recovery.
        fake.start_held.store(true, SeqCst);
        fake.stop_failures.store(1_000, SeqCst);
        let failed = call(&a, "publish_site", args(2114)).await;
        assert_eq!(terminal(&a, &failed).await["state"], "failed");
        assert!(fake.local.load(SeqCst));
        let supervisor = h.supervisor().clone();
        let _recovery = Stall::install(&supervisor.stall_recovery);
        drop(a);
        assert!(h.shutdown().await.is_err());
        assert!(
            supervisor.stall_recovery.lock().unwrap().is_some(),
            "custody recovery ran with process termination unconfirmed"
        );
        assert!(fake.local.load(SeqCst));
        fake.stop_failures.store(0, SeqCst);
    }

    /// A failed ingress-file disposition leaves its publish receipt
    /// unconfirmed even when the receipt's own terminal write fails after the
    /// no-POST intent was released and orphan settlement records it, by the
    /// final drain before any reaper turn or by a reaper turn.
    #[tokio::test]
    async fn an_orphaned_residue_receipt_stays_unconfirmed() {
        use std::sync::atomic::Ordering::SeqCst;
        let mut h = Harness::start_with_web(false, false, true).await;
        let a = h.client(0).await;
        let (vm, fake) = enrolled(&h, &a, 2200).await;
        let args = |n| json!({"vm_id":vm,"request_id":key(n)});
        let host = h.supervisor().host_id;
        let fail_terminal = "CREATE TRIGGER terminal_failure BEFORE UPDATE OF status ON activity
             WHEN OLD.action=15 AND OLD.status=0 AND NEW.status<>0
             BEGIN SELECT RAISE(ABORT, 'synthetic terminal write failure'); END;";
        let unconfirmed = |row: &Value| {
            assert_eq!(
                (&row["state"], &row["reason"], &row["cleanup"]),
                (
                    &json!("failed"),
                    &json!("cleanup_unconfirmed"),
                    &json!(proof_wire(false))
                ),
                "{row}"
            );
        };

        // Settled by the final drain before any reaper turn.
        fake.residue.store(true, SeqCst);
        trigger(&h, fail_terminal);
        let pending = call(&a, "publish_site", args(2210)).await;
        assert_eq!(pending["ok"], true, "{pending}");
        until(|| h.supervisor().publisher.workers().is_empty()).await;
        assert_eq!(phase(&a, &vm).await["phase"], "unpublished");
        assert_eq!(operation(&a, &pending).await["state"], "pending");
        trigger(&h, "DROP TRIGGER terminal_failure");
        drop(a);
        assert!(h.shutdown().await.is_err());
        let mut h = h.reopen(|_| {}).await;
        enroll(&h, &fake, true, Profile::synthetic(host, "tunnel-site"));
        let a = h.client(0).await;
        unconfirmed(&terminal(&a, &pending).await);

        // Settled by a reaper turn.
        let start = call(&a, "vm_start", args(2211)).await;
        assert_eq!(
            terminal_static_start(&a, &start).await["state"],
            "succeeded"
        );
        fake.residue.store(true, SeqCst);
        trigger(&h, fail_terminal);
        let pending = call(&a, "publish_site", args(2212)).await;
        assert_eq!(pending["ok"], true, "{pending}");
        until(|| h.supervisor().publisher.workers().is_empty()).await;
        assert_eq!(operation(&a, &pending).await["state"], "pending");
        trigger(&h, "DROP TRIGGER terminal_failure");
        h.tick().await;
        unconfirmed(&terminal(&a, &pending).await);
        assert_eq!((fake.posts.load(SeqCst), fake.deletes.load(SeqCst)), (0, 0));
        drop(a);
        assert!(h.shutdown().await.is_err());
    }

    /// A residue publish whose worker cleanup fails before its terminal
    /// receipt write (here the `revoking` checkpoint). Once SQLite recovers,
    /// the reaper or the final drain releases the intent and settles the
    /// orphaned receipt, which stays unconfirmed on both paths.
    #[tokio::test]
    async fn a_residue_receipt_stays_unconfirmed_when_worker_cleanup_fails() {
        use std::sync::atomic::Ordering::SeqCst;
        let mut h = Harness::start_with_web(false, false, true).await;
        let a = h.client(0).await;
        let (vm, fake) = enrolled(&h, &a, 2300).await;
        let args = |n| json!({"vm_id":vm,"request_id":key(n)});
        let host = h.supervisor().host_id;
        let refuse_revoking =
            "CREATE TRIGGER refuse_revoking BEFORE UPDATE OF phase ON vm_publication
             WHEN NEW.phase='revoking'
             BEGIN SELECT RAISE(ABORT, 'synthetic checkpoint failure'); END;";
        let unconfirmed = |row: &Value| {
            assert_eq!(
                (&row["state"], &row["cleanup"]),
                (&json!("failed"), &json!(proof_wire(false))),
                "an orphaned residue receipt claims confirmed cleanup: {row}"
            );
        };

        // Settled by a reaper turn.
        fake.residue.store(true, SeqCst);
        trigger(&h, refuse_revoking);
        let pending = call(&a, "publish_site", args(2310)).await;
        assert_eq!(pending["ok"], true, "{pending}");
        until(|| h.supervisor().publisher.workers().is_empty()).await;
        assert_eq!(operation(&a, &pending).await["state"], "pending");
        assert_eq!(phase(&a, &vm).await["phase"], "publishing");
        trigger(&h, "DROP TRIGGER refuse_revoking");
        h.tick().await;
        unconfirmed(&terminal(&a, &pending).await);
        assert_eq!(phase(&a, &vm).await["phase"], "unpublished");

        // Settled by foreground shutdown before any reaper turn, checked
        // after restart.
        fake.residue.store(true, SeqCst);
        trigger(&h, refuse_revoking);
        let pending = call(&a, "publish_site", args(2311)).await;
        assert_eq!(pending["ok"], true, "{pending}");
        until(|| h.supervisor().publisher.workers().is_empty()).await;
        assert_eq!(operation(&a, &pending).await["state"], "pending");
        trigger(&h, "DROP TRIGGER refuse_revoking");
        drop(a);
        assert!(
            h.shutdown().await.is_err(),
            "a residue drain reported clean"
        );
        let mut h = h.reopen(|_| {}).await;
        enroll(&h, &fake, true, Profile::synthetic(host, "tunnel-site"));
        let a = h.client(0).await;
        unconfirmed(&terminal(&a, &pending).await);
        assert_eq!(phase(&a, &vm).await["phase"], "unpublished");
        assert_eq!((fake.posts.load(SeqCst), fake.deletes.load(SeqCst)), (0, 0));
        assert!(!fake.local.load(SeqCst));
        drop(a);
        h.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn a_failed_revoking_checkpoint_keeps_the_owner_cause() {
        let mut h = Harness::start_with_web(false, false, true).await;
        let a = h.client(0).await;
        let (vm, _fake) = enrolled(&h, &a, 1440).await;
        let args = |n| json!({"vm_id":vm,"request_id":key(n)});
        // Parked before connector startup.
        let (worker, in_gate, release) = gate();
        *h.supervisor().publisher.activate_gate.lock().unwrap() = Some(worker);
        let pending = call(&a, "publish_site", args(1450)).await;
        entered(in_gate).await;
        // The owner revokes; the deferred stop finds an empty local slot.
        assert_eq!(
            call(&a, "unpublish_site", args(1450)).await["reason"],
            "key_collision"
        );
        until(|| revocations_settled(h.supervisor())).await;
        trigger(
            &h,
            "CREATE TRIGGER refuse_revoking BEFORE UPDATE OF phase ON vm_publication
             WHEN NEW.phase='revoking'
             BEGIN SELECT RAISE(ABORT, 'synthetic checkpoint failure'); END;",
        );
        release.send(()).unwrap();
        until(|| h.supervisor().publisher.workers().is_empty()).await;
        assert_eq!(phase(&a, &vm).await["phase"], "publishing");
        trigger(&h, "DROP TRIGGER refuse_revoking");
        // After SQLite recovers, the release records the owner cause.
        assert_eq!(settle(&h, &a, &vm).await["reason"], "cancelled");
        assert_eq!(terminal(&a, &pending).await["state"], "failed");
        drop(a);
        h.shutdown().await.unwrap();
    }

    /// A revocation linearized before the seal recorded its stop obligation;
    /// its dispatch is parked at the old lookup-then-enqueue gap, so no runner
    /// exists. The drain runs that obligation before it clears the registry,
    /// and the runner spawned after the drain finds nothing to stop.
    #[tokio::test]
    async fn a_stop_recorded_before_the_seal_settles_before_the_registry_clears() {
        use std::sync::atomic::Ordering::SeqCst;
        let mut h = Harness::start_with_web(false, false, true).await;
        let a = h.client(0).await;
        let (vm, fake) = enrolled(&h, &a, 1460).await;
        let published = call(
            &a,
            "publish_site",
            json!({"vm_id":vm,"request_id":key(1470)}),
        )
        .await;
        assert_eq!(terminal(&a, &published).await["state"], "succeeded");
        let supervisor = h.supervisor().clone();
        let mut parked = Stall::install(&supervisor.stall_revoked);
        let inventory = supervisor.connection("alpha".into()).inventory();
        let late_vm = vm.clone();
        let dispatch = tokio::task::spawn_blocking(move || {
            inventory.dispatch(
                request(
                    "unpublish_site",
                    json!({"vm_id":late_vm,"request_id":key(1471)}),
                ),
                || false,
            )
        });
        parked.entered().await;
        assert_eq!(supervisor.publisher.stop_obligations(), 1);
        let spawned = stops_spawned(&supervisor);
        supervisor.publisher.events.lock().unwrap().clear();
        drop(a);
        h.shutdown().await.unwrap();
        assert_eq!(
            *supervisor.publisher.events.lock().unwrap(),
            vec!["stopped", "cleared"],
            "the registry was cleared before the recorded stop ran"
        );
        assert_eq!(supervisor.publisher.stop_obligations(), 0);
        assert_eq!(stops_spawned(&supervisor), spawned);
        assert!(!fake.local.load(SeqCst));
        // The parked dispatch resumes after the drain and spawns its runner,
        // which finds no obligation and changes nothing.
        parked.release_and_join().await;
        assert!(dispatch.await.unwrap().is_err());
        assert_eq!(stops_spawned(&supervisor), spawned + 1);
        until(|| revocations_settled(&supervisor)).await;
        assert_eq!(
            *supervisor.publisher.events.lock().unwrap(),
            vec!["stopped", "cleared"]
        );
        assert_eq!(supervisor.publisher.registry_len(), 0);
    }

    /// An accepted stop is running, held across drain entry. The drain waits
    /// for it before it clears the registry.
    #[tokio::test]
    async fn an_accepted_stop_held_across_drain_entry_settles_before_the_registry_clears() {
        use std::sync::atomic::Ordering::SeqCst;
        let mut h = Harness::start_with_web(false, false, true).await;
        let a = h.client(0).await;
        let (vm, fake) = enrolled(&h, &a, 1560).await;
        let args = |n| json!({"vm_id":vm,"request_id":key(n)});
        let published = call(&a, "publish_site", args(1570)).await;
        assert_eq!(terminal(&a, &published).await["state"], "succeeded");
        let supervisor = h.supervisor().clone();
        let (stop, stop_entered, release_stop) = gate();
        *supervisor.publisher.stop_gate.lock().unwrap() = Some(stop);
        assert_eq!(
            call(&a, "unpublish_site", args(1570)).await["reason"],
            "key_collision"
        );
        entered(stop_entered).await;
        supervisor.publisher.events.lock().unwrap().clear();
        let (passed, first_pass) = tokio::sync::oneshot::channel();
        *supervisor.drain_first_pass.lock().unwrap() = Some(passed);
        let releaser = tokio::spawn(async move {
            first_pass.await.unwrap();
            // Long enough for a drain that skipped the running stop to clear.
            tokio::time::sleep(Duration::from_millis(50)).await;
            release_stop.send(()).unwrap();
        });
        drop(a);
        h.shutdown().await.unwrap();
        releaser.await.unwrap();
        assert_eq!(
            *supervisor.publisher.events.lock().unwrap(),
            vec!["stopped", "cleared"],
            "the registry was cleared before the accepted stop completed"
        );
        assert_eq!(supervisor.publisher.stop_obligations(), 0);
        assert!(!fake.local.load(SeqCst));
    }

    /// A dispatch whose revocation runs only after the seal has no authority:
    /// it records no stop obligation, spawns no runner and changes no
    /// connector. The registered worker settles and only then is the registry
    /// cleared.
    #[tokio::test]
    async fn a_revocation_after_the_seal_records_no_stop() {
        let mut h = Harness::start_with_web(false, false, true).await;
        let a = h.client(0).await;
        let (vm, _fake) = enrolled(&h, &a, 1660).await;
        let (worker, worker_entered, release_worker) = gate();
        *h.supervisor().publisher.activate_gate.lock().unwrap() = Some(worker);
        let pending = call(
            &a,
            "publish_site",
            json!({"vm_id":vm,"request_id":key(1670)}),
        )
        .await;
        assert_eq!(pending["ok"], true, "{pending}");
        entered(worker_entered).await;
        let supervisor = h.supervisor().clone();
        supervisor.publisher.events.lock().unwrap().clear();
        let spawned = stops_spawned(&supervisor);
        let (passed, first_pass) = tokio::sync::oneshot::channel();
        *supervisor.drain_first_pass.lock().unwrap() = Some(passed);
        let late = supervisor.clone();
        let late_vm = vm.clone();
        let releaser = tokio::spawn(async move {
            first_pass.await.unwrap();
            let inventory = late.connection("alpha".into()).inventory();
            let reply = tokio::task::spawn_blocking(move || {
                inventory.dispatch(
                    request(
                        "unpublish_site",
                        json!({"vm_id":late_vm,"request_id":key(1671)}),
                    ),
                    || false,
                )
            })
            .await
            .unwrap();
            let recorded = (late.publisher.stop_obligations(), stops_spawned(&late));
            release_worker.send(()).unwrap();
            (reply.is_err(), recorded)
        });
        drop(a);
        h.shutdown().await.unwrap();
        let (refused, recorded) = releaser.await.unwrap();
        assert!(refused);
        assert_eq!(
            recorded,
            (0, spawned),
            "a revocation after the seal recorded a stop"
        );
        assert_eq!(
            *supervisor.publisher.events.lock().unwrap(),
            vec!["cleared"]
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

    #[test]
    fn static_compose_network_fence_unverified_closes_foreground_admission() {
        use rusqlite::params;

        let temp = crate::service::application::tests::private_temp();
        let mut supervisor = Supervisor::new(
            temp.path(),
            VmPolicy::default(),
            ForegroundClock {
                utc: Some(Arc::new(AtomicI64::new(UTC))),
            },
            Some(RECIPE),
            NativeRefusal::QualificationMissing,
            None,
        )
        .unwrap();
        let state = Arc::get_mut(&mut supervisor).unwrap();
        state.recipes[1] = Some((WEB_RECIPE_ID, Some(WEB_DIGEST)));
        state
            .install_static_selection(Err(NativeRefusal::NetworkFenceUnverified))
            .unwrap();

        let vm = [2_u8; 16];
        let template = [3_u8; 16];
        supervisor.controller.store_for_test().transact(|db| {
            db.execute(
                "INSERT INTO vm_templates VALUES(?1,1,?2,zeroblob(32),zeroblob(32),4096)",
                params![template, WEB_DIGEST],
            )?;
            db.execute(
                "INSERT INTO vm_static_instances(id,name,principal,host,template,
                    source_generation,recipe_digest,disk_digest,vars_digest,state,persistent,staging)
                 VALUES(?1,'web-site','alice',?2,?3,zeroblob(16),?4,zeroblob(32),zeroblob(32),1,4096,0)",
                params![vm, supervisor.host_id, template, WEB_DIGEST],
            )?;
            db.execute("INSERT INTO vm_compose_projects(vm,project) VALUES(?1,'fixture')", [vm])?;
            Ok(())
        }).unwrap();
        {
            let mut gate = supervisor.lock().unwrap();
            gate.allow_agent_public_sites = true;
            gate.publish_profile =
                Some(Arc::new(Profile::synthetic(supervisor.host_id, "web-site")));
        }

        let dispatch = |supervisor: &Arc<Supervisor>, tool, args| {
            supervisor
                .connection("alice".into())
                .inventory()
                .dispatch(request(tool, args), || false)
        };
        let refused = |supervisor: &Arc<Supervisor>, tool, args| {
            assert_eq!(
                dispatch(supervisor, tool, args).unwrap_err().0,
                "network_fence_unverified",
                "{tool}",
            );
        };
        let compose = selected_static::FixtureStaticQualification::select().recipe();
        let catalog = dispatch(&supervisor, "vm_catalog", json!({})).unwrap();
        assert_eq!(
            catalog["capability"]["static_compose"],
            json!({"available":false,"reason":"network_fence_unverified"}),
        );
        assert!(
            catalog["capability"]
                .get("selected_static_qualification")
                .is_none()
        );
        let compose_id = hex::encode(compose.recipe_id);
        let entries = catalog["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().all(|entry| entry["recipe_id"] != compose_id));
        let build = |n| json!({"recipe_id":compose_id,"request_id":key(n)});
        assert_eq!(
            dispatch(&supervisor, "vm_template_build", build(1))
                .unwrap_err()
                .0,
            "not_found",
        );

        let args = |n| json!({"vm_id":hex::encode(vm),"request_id":key(n)});
        refused(
            &supervisor,
            "static_vm_create",
            json!({
                "template_id":hex::encode(template),"name":"another-site","request_id":key(2),
            }),
        );
        refused(&supervisor, "vm_start", args(3));
        // Stop, script, and publish require a running row to reach their gate.
        supervisor
            .controller
            .store_for_test()
            .transact(|db| {
                db.execute("UPDATE vm_static_instances SET state=2 WHERE id=?1", [vm])?;
                Ok(())
            })
            .unwrap();
        refused(&supervisor, "vm_stop", args(4));
        let mut script = args(5);
        script["script"] = json!("exit 0");
        refused(&supervisor, "vm_run_script", script);
        refused(&supervisor, "publish_site", args(6));

        // A measured fixture opens the selection. Once that observation
        // retires, neither the catalog nor fresh admissions can use it.
        Arc::get_mut(&mut supervisor)
            .unwrap()
            .install_static_selection(Ok(selected_static::FixtureStaticQualification::select()))
            .unwrap();
        let catalog = dispatch(&supervisor, "vm_catalog", json!({})).unwrap();
        assert_eq!(catalog["capability"]["static_compose"]["available"], true);
        assert_eq!(
            catalog["capability"]["selected_static_qualification"]["compose_recipe"]["recipe_id"],
            compose_id,
        );
        assert!(
            catalog["entries"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| { entry["recipe_id"] == compose_id && entry["state"] == "buildable" })
        );

        supervisor
            .static_selection()
            .unwrap()
            .retire_fixture_network();
        let catalog = dispatch(&supervisor, "vm_catalog", json!({})).unwrap();
        assert_eq!(
            catalog["capability"]["static_compose"]["reason"],
            "network_fence_unverified"
        );
        assert!(
            catalog["capability"]
                .get("selected_static_qualification")
                .is_none()
        );
        assert!(
            catalog["entries"]
                .as_array()
                .unwrap()
                .iter()
                .all(|entry| { entry["recipe_id"] != compose_id })
        );
        assert!(catalog["entries"].as_array().unwrap().iter().any(|entry| {
            entry["recipe_id"] == hex::encode(WEB_RECIPE_ID) && entry["state"] == "unqualified"
        }));
        refused(&supervisor, "vm_template_build", build(7));
        refused(&supervisor, "publish_site", args(8));
        let compose_args =
            |n| json!({"vm_id":hex::encode(vm),"project_id":"fixture","request_id":key(n)});
        refused(&supervisor, "compose_start", compose_args(9));
        refused(&supervisor, "compose_backup", compose_args(10));
        let foreign = supervisor
            .connection("bob".into())
            .inventory()
            .dispatch(request("compose_start", compose_args(11)), || false);
        assert_eq!(foreign.unwrap_err().0, "not_found");
        supervisor
            .controller
            .store_for_test()
            .read_only(|db| {
                for table in ["vm_operations", "vm_publication", "vm_publication_hosts"] {
                    assert_eq!(
                        crate::store::vm::count(db, &format!("SELECT count(*) FROM {table}"), [])?,
                        0,
                        "{table}",
                    );
                }
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn static_vm_extend_without_selected_test_is_not_found() {
        let temp = crate::service::application::tests::private_temp();
        let supervisor = Supervisor::new(
            temp.path(),
            VmPolicy::default(),
            ForegroundClock {
                utc: Some(Arc::new(AtomicI64::new(UTC))),
            },
            None,
            NativeRefusal::Disabled,
            None,
        )
        .unwrap();
        assert!(supervisor.lock().unwrap().fixture.is_none());
        assert!(!supervisor.has_selected_test());

        let vm: Id = [2; 16];
        let generation: Id = [3; 16];
        let store = supervisor.controller.store_for_test();
        store
            .transact(|db| {
                db.execute(
                    "INSERT INTO vm_static_instances(id,name,principal,host,template,
                        source_generation,recipe_digest,disk_digest,vars_digest,state,
                        persistent,staging,origin_generation)
                     VALUES(?1,'web-site','alice',?2,zeroblob(16),?3,
                        zeroblob(32),zeroblob(32),zeroblob(32),1,4096,0,?3)",
                    rusqlite::params![vm, supervisor.host_id, generation],
                )?;
                db.execute(
                    "INSERT INTO vm_generations(id,static_id,generation,active) VALUES(?1,?2,1,1)",
                    rusqlite::params![generation, vm],
                )?;
                Ok(())
            })
            .unwrap();

        let connection = supervisor.connection("alice".into());
        let refused = connection.inventory().dispatch(
            request(
                "vm_extend",
                json!({"vm_id":hex::encode(vm),"ttl_s":600,"request_id":key(1)}),
            ),
            || false,
        );
        assert_eq!(refused.unwrap_err().0, "not_found");
        store
            .read_only(|db| {
                assert_eq!(
                    crate::store::vm::count(
                        db,
                        "SELECT count(*) FROM activity WHERE principal='alice' AND vm=?1
                            AND action=?2 AND status=4 AND reason=?3",
                        rusqlite::params![vm, Action::Extend as i64, Refusal::NotFound as i64],
                    )?,
                    1
                );
                assert_eq!(
                    crate::store::vm::count(db, "SELECT count(*) FROM vm_operations", [])?,
                    0
                );
                Ok(())
            })
            .unwrap();
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
        assert_eq!(
            terminal_static_start(&a, &start).await["state"],
            "succeeded"
        );
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
        assert_eq!(
            terminal_static_start(&a, &start).await["state"],
            "succeeded"
        );
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
        assert_eq!(
            terminal_static_start(&a, &started).await["state"],
            "succeeded"
        );
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
            let settled = if tool == "vm_start" {
                terminal_static_start(&a, &operation).await
            } else {
                terminal(&a, &operation).await
            };
            assert_eq!(settled["state"], "succeeded", "{tool}");
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
            let settled = if tool == "vm_start" {
                terminal_static_start(&a, &operation).await
            } else {
                terminal(&a, &operation).await
            };
            assert_eq!(settled["state"], "succeeded", "{tool}");
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
        tokio::time::timeout(FIXTURE_START_GUARD, async {
            let identity = FixtureOrigin::new([0x33; 16]).unwrap();
            let lease = OriginPortLease::acquire().unwrap();
            let other = StaticOriginIdentity::for_generation([0x44; 16]).provisioning();
            let refused = identity.probe(&lease, other.server_name().into()).await;
            assert_eq!(
                refused.err().and_then(|fault| fault.stage),
                Some(FixtureBootStage::TlsIo)
            );
            identity.probe(&lease, identity.name.clone()).await.unwrap();
        })
        .await
        .expect("origin probe exceeded its hang guard");
    }

    /// A valid exchange that outlasts the former one-second cutoff still
    /// reaches a successful static start. The delay sits inside the probe.
    #[tokio::test]
    async fn static_fixture_start_accepts_a_tls_probe_past_the_former_one_second_cutoff() {
        const FORMER_CUTOFF: Duration = Duration::from_secs(1);
        const PROBE_DELAY: Duration = Duration::from_millis(1_100);
        assert!(PROBE_DELAY > FORMER_CUTOFF);
        assert!(FIXTURE_TLS_DEADLINE > PROBE_DELAY);
        assert!(FIXTURE_START_GUARD > FIXTURE_TLS_DEADLINE);

        let mut h = Harness::start_with_web(false, false, true).await;
        let a = h.client(0).await;
        let template = web_template(&h, &a, 450).await;
        let created = call(
            &a,
            "static_vm_create",
            json!({"name":"slow-probe","template_id":template,"request_id":key(451)}),
        )
        .await;
        let created = terminal(&a, &created).await;
        assert_eq!(created["state"], "succeeded", "{created}");
        let vm = created["vm_id"].clone();
        h.fixture(|fixture| {
            let delay_ms = u64::try_from(PROBE_DELAY.as_millis()).expect("probe delay fits u64");
            fixture
                .probe_delay_ms
                .store(delay_ms, std::sync::atomic::Ordering::Relaxed);
        });
        let began = Instant::now();
        let start = call(&a, "vm_start", json!({"vm_id":vm,"request_id":key(452)})).await;
        let started = terminal_static_start(&a, &start).await;
        assert!(
            began.elapsed() >= PROBE_DELAY,
            "the fixture probe did not apply its delay"
        );
        assert_settled_static_start(&h, &vm, &started);
        drop(a);
        h.shutdown().await.unwrap();
    }

    #[cfg(all(windows, target_arch = "x86_64"))]
    mod native_run_tests {
        include!("vm/native_run/tests.rs");
    }

    include!("vm/vault_test_cases.rs");

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
        // Hang guard only; the journey's step-level assertions are what matter.
        tokio::time::timeout(Duration::from_secs(120), journey())
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
        const EXPECTED_TOOL_COUNT: usize = 18 + 4 + 7;
        assert_eq!(names.len(), EXPECTED_TOOL_COUNT);
        assert_eq!(names, ToolName::ALL.map(ToolName::name));
        for (tool_name, required, optional) in [
            (
                "compose_deploy",
                &["vm_id", "project_id", "release_id", "request_id"][..],
                &[][..],
            ),
            (
                "compose_start",
                &["vm_id", "project_id", "request_id"][..],
                &[][..],
            ),
            (
                "compose_stop",
                &["vm_id", "project_id", "request_id"][..],
                &[][..],
            ),
            (
                "compose_down",
                &["vm_id", "project_id", "request_id"][..],
                &[][..],
            ),
            ("compose_status", &["vm_id", "project_id"][..], &[][..]),
            (
                "compose_logs",
                &["vm_id", "project_id"][..],
                &["limit", "cursor"][..],
            ),
            (
                "compose_backup",
                &["vm_id", "project_id", "request_id"][..],
                &[][..],
            ),
            (
                "vm_snapshot_create",
                &["vm_id", "request_id"][..],
                &["name"][..],
            ),
            ("vm_snapshot_list", &["vm_id"][..], &["limit", "cursor"][..]),
            (
                "vm_snapshot_restore",
                &["vm_id", "snapshot_id", "request_id"][..],
                &[][..],
            ),
            (
                "vm_snapshot_delete",
                &["vm_id", "snapshot_id", "request_id"][..],
                &[][..],
            ),
        ] {
            let tool = tools
                .tools
                .iter()
                .find(|tool| tool.name.as_ref() == tool_name)
                .unwrap_or_else(|| panic!("missing tool {tool_name}"));
            let actual_required = tool
                .input_schema
                .get("required")
                .and_then(Value::as_array)
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap())
                .collect::<Vec<_>>();
            assert_eq!(
                actual_required, required,
                "required mismatch for {tool_name}"
            );
            let properties = tool
                .input_schema
                .get("properties")
                .and_then(Value::as_object)
                .unwrap();
            assert_eq!(
                properties.len(),
                required.len() + optional.len(),
                "property count mismatch for {tool_name}"
            );
            for prop in required.iter().chain(optional.iter()) {
                assert!(
                    properties.contains_key(*prop),
                    "missing property {prop} on {tool_name}"
                );
            }
        }
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
                let (ticks, receiver) = TestTicks::channel();
                *supervisor.ticks.lock().unwrap() = Some(receiver);
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
                    ticks.send(None).unwrap();
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

    // -------------------------------------------------------------------------
    // Docker E3 multi-hostname publication tests
    // -------------------------------------------------------------------------

    async fn enrolled_multi(
        h: &Harness,
        client: &Client,
        key_base: u64,
        policy: bool,
        profile: Profile,
    ) -> (Value, Arc<crate::vm::runtime::publication::Fake>) {
        use std::sync::atomic::Ordering::SeqCst;
        let fake = Arc::new(crate::vm::runtime::publication::Fake::default());
        fake.credential.store(true, SeqCst);
        fake.multi_host.store(true, SeqCst);
        let compose_recipe = h
            .supervisor()
            .static_selection()
            .map(|selected| selected.recipe());
        let recipe = compose_recipe.map_or(WEB_RECIPE_ID, |recipe| recipe.recipe_id);
        if let Some(selected) = compose_recipe {
            let catalog = call(client, "vm_catalog", json!({})).await;
            let id = hex::encode(recipe);
            assert!(
                catalog["entries"]
                    .as_array()
                    .unwrap()
                    .contains(&json!({"recipe_id":id,"guest":"ubuntu","state":"buildable"}))
            );
            assert_eq!(
                catalog["capability"]["static_compose"],
                json!({"available":true}),
            );
            assert_eq!(
                catalog["capability"]["selected_static_qualification"]["compose_recipe"],
                json!({"recipe_id":id,"recipe_sha256":hex::encode(selected.recipe_sha256)}),
            );
        }
        let build = call(
            client,
            "vm_template_build",
            json!({"recipe_id":hex::encode(recipe), "request_id":key(key_base)}),
        )
        .await;
        h.hold_build.notify_one();
        let template = terminal(client, &build).await["template_id"].clone();
        if compose_recipe.is_some() {
            let reply = call(
                client,
                "vm_create",
                json!({
                    "template_id":template,"request_id":key(key_base + 3)
                }),
            )
            .await;
            assert_eq!(reply["reason"], "not_found", "{reply}");
            h.supervisor()
                .controller
                .compose_store()
                .read_only(|db| {
                    assert_eq!(
                        crate::store::vm::count(db, "SELECT count(*) FROM vm_instances", [])?,
                        0
                    );
                    Ok(())
                })
                .unwrap();
        }
        let created = call(client, "static_vm_create",
            json!({"template_id":template,"name":profile.static_name(),"request_id":key(key_base + 2)})).await;
        let vm_id = terminal(client, &created).await["vm_id"].clone();
        let start = call(
            client,
            "vm_start",
            json!({"vm_id":vm_id,"request_id":key(key_base + 1)}),
        )
        .await;
        assert_eq!(
            terminal_static_start(client, &start).await["state"],
            "succeeded"
        );
        let id = parse_id(vm_id.as_str().unwrap()).unwrap();
        let supervisor = h.supervisor();
        let origin = supervisor
            .gate
            .lock()
            .unwrap()
            .fixture
            .as_ref()
            .unwrap()
            .publish_origin(id)
            .unwrap();
        let (host, mut peer) = tokio::io::duplex(64 * 1024);
        let boot = key_base;
        let witness = fake.clone();
        tokio::spawn(async move {
            use arx_guest::compose::{ComposeRequest, ComposeResponse};
            fake_compose_hello(&mut peer, boot).await;
            loop {
                use tokio::io::AsyncReadExt;
                let Ok(len) = peer.read_u32().await else {
                    break;
                };
                assert!(len as usize <= arx_guest::MAX_FRAME);
                let mut payload = vec![0; len as usize];
                peer.read_exact(&mut payload).await.unwrap();
                let ComposeRequest::ComposeInstallRoutes {
                    id,
                    generation,
                    readiness_host,
                    zone,
                    routes,
                } = serde_json::from_slice(&payload).unwrap()
                else {
                    panic!("expected install")
                };
                assert_eq!(boot, generation);
                assert!(!witness.local.load(SeqCst));
                let names: Vec<_> = routes.iter().map(|route| route.hostname.clone()).collect();
                assert!(witness.preflight_order.lock().unwrap().ends_with(&names));
                witness
                    .starts_at_route_install
                    .lock()
                    .unwrap()
                    .push(witness.starts.load(SeqCst));
                assert!(
                    arx_guest::compose::routing::RoutingTable::new(
                        readiness_host,
                        Some(zone),
                        arx_guest::compose::routing::SLIRP_FORWARD_PEER,
                        routes
                    )
                    .is_ok()
                );
                fake_compose_reply(
                    &mut peer,
                    ComposeResponse::ComposeRoutesInstalled { id, generation },
                )
                .await;
            }
        });
        let channel = crate::vm::guest::ComposeChannel::negotiate(host, boot, &supervisor.cancel)
            .await
            .unwrap();
        supervisor
            .compose
            .attach(
                supervisor.controller.compose_store(),
                id,
                compose::Session::for_origin(&origin, channel),
            )
            .unwrap();
        enroll(h, &fake, policy, profile);
        (vm_id, fake)
    }

    #[tokio::test]
    async fn multihost_policy_off_and_happy_path_single_connector_with_per_host_status() {
        use std::sync::atomic::Ordering::SeqCst;
        let mut h = Harness::start_configured(false, false, false, |fixture| {
            fixture.static_compose = true;
        })
        .await;
        let a = h.client(0).await;
        let host = h.supervisor().host_id;
        let zone = "b".repeat(32);
        let profile = Profile::synthetic_multi(
            host,
            "docker-multi",
            &["api.example.invalid", "app.example.invalid"],
            Some("example.invalid"),
            &zone,
        )
        .with_routes(vec![
            crate::vm::runtime::publication::RouteBinding {
                hostname: crate::vm::publish::Hostname::parse("api.example.invalid").unwrap(),
                project_id: "proj-a".into(),
                service: "api".into(),
                project_slot: 0,
                service_index: 0,
                route_slot: 0,
            },
            crate::vm::runtime::publication::RouteBinding {
                hostname: crate::vm::publish::Hostname::parse("app.example.invalid").unwrap(),
                project_id: "proj-a".into(),
                service: "web".into(),
                project_slot: 0,
                service_index: 1,
                route_slot: 1,
            },
        ]);
        let (vm, fake) = enrolled_multi(&h, &a, 4100, false, profile.clone()).await;
        let args = |n| json!({"vm_id":vm,"request_id":key(n)});

        let mut script = args(4109);
        script["script"] = json!("exit 0");
        let refused = call(&a, "vm_run_script", script).await;
        assert_eq!(refused["reason"], "disabled", "{refused}");

        // Policy off refuses with zero connector starts and zero DNS effects.
        let refused = call(&a, "publish_site", args(4110)).await;
        assert_eq!(refused["reason"], "public_sites_disabled", "{refused}");
        assert_eq!(fake.starts.load(SeqCst), 0);
        assert_eq!(fake.posts.load(SeqCst), 0);
        assert!(fake.preflight_order.lock().unwrap().is_empty());

        assert!(
            h.supervisor()
                .controller
                .publication_intent()
                .unwrap()
                .is_none()
        );
        for (tool, reason) in [
            ("static_vm_delete", "static_delete_forbidden"),
            ("compose_project_delete", "static_backup_delete_forbidden"),
            ("compose_volume_delete", "static_backup_delete_forbidden"),
            ("compose_backup_delete", "static_backup_delete_forbidden"),
        ] {
            let reply = call(&a, tool, json!({"vm_id":vm})).await;
            assert_eq!(reply["reason"], reason, "{tool}: {reply}");
        }

        // Turn policy on and publish 2 hostnames on one connector Job.
        h.supervisor().gate.lock().unwrap().allow_agent_public_sites = true;
        let published = call(&a, "publish_site", args(4111)).await;
        assert_eq!(terminal(&a, &published).await["state"], "succeeded");
        assert_eq!(
            *fake.starts_at_route_install.lock().unwrap(),
            [0],
            "boot-bound peer must receive the route set"
        );
        assert_eq!(fake.starts.load(SeqCst), 1, "must use one connector Job");
        assert_eq!(fake.posts.load(SeqCst), 2);
        assert!(fake.local.load(SeqCst));
        assert_eq!(
            *fake.preflight_order.lock().unwrap(),
            vec!["api.example.invalid", "app.example.invalid"]
        );
        assert_eq!(
            *fake.create_order.lock().unwrap(),
            vec!["api.example.invalid", "app.example.invalid"]
        );
        assert_eq!(
            *fake.origin_probed.lock().unwrap(),
            vec!["api.example.invalid", "app.example.invalid"]
        );

        // Generated ingress YAML contains both exact host rows, httpHostHeader,
        // TLS verification, WARP off, and final http_status:404.
        let ingress = fake.ingress.lock().unwrap().clone().expect("ingress yaml");
        assert!(
            ingress.contains("hostname: 'api.example.invalid'"),
            "{ingress}"
        );
        assert!(
            ingress.contains("httpHostHeader: 'api.example.invalid'"),
            "{ingress}"
        );
        assert!(
            ingress.contains("hostname: 'app.example.invalid'"),
            "{ingress}"
        );
        assert!(
            ingress.contains("httpHostHeader: 'app.example.invalid'"),
            "{ingress}"
        );
        assert!(ingress.contains("noTLSVerify: false"), "{ingress}");
        assert!(
            ingress.contains("warp-routing:\n  enabled: false"),
            "{ingress}"
        );
        assert!(ingress.contains("- service: http_status:404"), "{ingress}");

        fake.upstream_unavailable.store(true, SeqCst);
        let supervisor = h.supervisor();
        drop(supervisor.publisher.slot().await);
        assert_eq!(
            supervisor
                .publisher
                .turn(&supervisor.controller, &supervisor.cancel)
                .await
                .unwrap(),
            crate::vm::runtime::publication::Turn::Healthy,
        );
        assert!(fake.local.load(SeqCst));
        assert_eq!(fake.host_records.lock().unwrap().len(), 2);
        fake.upstream_unavailable.store(false, SeqCst);

        // Publish status projects per-host non-secret state without leaking hostnames or record IDs.
        let status = phase(&a, &vm).await;
        let status_text = status.to_string();
        assert_eq!(status["phase"], "published", "{status}");
        assert_eq!(status["host_count"], 2, "{status}");
        assert_eq!(status["hosts"][0]["position"], 0);
        assert_eq!(status["hosts"][0]["state"], "verified");
        assert_eq!(status["hosts"][0]["route_slot"], 0);
        assert_eq!(status["hosts"][0]["project_id"], "proj-a");
        assert_eq!(status["hosts"][0]["service"], "api");
        assert_eq!(status["hosts"][1]["position"], 1);
        assert_eq!(status["hosts"][1]["state"], "verified");
        assert_eq!(status["hosts"][1]["route_slot"], 1);
        assert!(
            !status_text.contains("api.example.invalid"),
            "leaked hostname: {status}"
        );
        assert!(
            !status_text.contains("app.example.invalid"),
            "leaked hostname: {status}"
        );

        // Each child row has a distinct ownership marker.
        let active = h
            .supervisor()
            .controller
            .publication_intent()
            .unwrap()
            .unwrap();
        // A subsequent release may keep using both bound slots. Reordering the
        // manifest would send the api hostname to the web service's loopback port.
        let release = [0x33u8; 16];
        h.supervisor().controller.compose_store().transact(|tx| {
            tx.execute("INSERT INTO vm_compose_projects(vm,project) VALUES(?1,'proj-a')",[active.vm])?;
            let request = compose::Mutation {
                target:compose::Target {vm:active.vm,project:"proj-a".into()}, kind:compose::Kind::Deploy,
                key:RequestId::parse(&key(4140)).unwrap(),release:Some(release),
            };
            for (names,allowed) in [(["api","web"],true),(["web","api"],false)] {
                let manifest = serde_json::to_vec(&json!({"services":names.map(|name|json!({"name":name,"http_port":8080}))}))?;
                tx.execute("INSERT INTO vm_compose_releases(vm,project,id,digest,manifest) VALUES(?1,'proj-a',?2,?3,?4)
                            ON CONFLICT(vm,project,id) DO UPDATE SET manifest=excluded.manifest,digest=excluded.digest",
                            rusqlite::params![active.vm,release,arx_guest::compose::sha256_hex(&manifest),manifest])?;
                assert_eq!(compose::deployment_preserves_routes(tx,&request)?,allowed);
            }
            Ok(())
        }).unwrap();
        assert_eq!(active.hosts.len(), 2);
        assert_ne!(active.hosts[0].marker, active.hosts[1].marker);

        // Unpublish stops ingress first (asserted inside Fake::remove) and removes both records.
        let revoke = call(&a, "unpublish_site", args(4112)).await;
        assert_eq!(terminal(&a, &revoke).await["state"], "succeeded");
        assert_eq!(phase(&a, &vm).await["phase"], "unpublished");
        assert!(!fake.local.load(SeqCst));
        assert_eq!(fake.deletes.load(SeqCst), 2);
        assert!(fake.host_records.lock().unwrap().is_empty());

        // Publish max bound (8 hostnames) on the same single connector Job.
        let eight = [
            "h0.example.invalid",
            "h1.example.invalid",
            "h2.example.invalid",
            "h3.example.invalid",
            "h4.example.invalid",
            "h5.example.invalid",
            "h6.example.invalid",
            "h7.example.invalid",
        ];
        enroll(
            &h,
            &fake,
            true,
            Profile::synthetic_multi(host, "docker-multi", &eight, Some("example.invalid"), &zone),
        );
        let pub8 = call(&a, "publish_site", args(4113)).await;
        assert_eq!(terminal(&a, &pub8).await["state"], "succeeded");
        assert_eq!(
            fake.starts.load(SeqCst),
            2,
            "one connector start for all 8 hosts"
        );
        assert_eq!(fake.posts.load(SeqCst), 10);
        assert_eq!(phase(&a, &vm).await["host_count"], 8);
        let rev8 = call(&a, "unpublish_site", args(4114)).await;
        assert_eq!(terminal(&a, &rev8).await["state"], "succeeded");
        assert_eq!(fake.deletes.load(SeqCst), 10);
        assert!(fake.host_records.lock().unwrap().is_empty());

        assert!(
            h.supervisor()
                .controller
                .publication_intent()
                .unwrap()
                .is_none()
        );

        drop(a);
        h.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn multihost_origin_mismatch_blocks_before_dns_post() {
        use std::sync::atomic::Ordering::SeqCst;
        let mut h = Harness::start_with_web(false, false, true).await;
        let a = h.client(0).await;
        let host = h.supervisor().host_id;
        let zone = "b".repeat(32);
        let profile = Profile::synthetic_multi(
            host,
            "docker-origin-mismatch",
            &["a.example.invalid", "b.example.invalid"],
            Some("example.invalid"),
            &zone,
        );
        let (vm, fake) = enrolled_multi(&h, &a, 4200, true, profile).await;
        fake.host_origin_mismatch
            .lock()
            .unwrap()
            .insert("b.example.invalid".into());

        let result = call(
            &a,
            "publish_site",
            json!({"vm_id":vm,"request_id":key(4210)}),
        )
        .await;
        let term = terminal(&a, &result).await;
        assert_eq!(term["state"], "failed", "{term}");
        assert_eq!(term["reason"], "origin_unavailable", "{term}");
        assert_eq!(
            fake.posts.load(SeqCst),
            0,
            "origin mismatch must prevent any DNS POST"
        );
        assert!(!fake.local.load(SeqCst), "connector must not remain live");
        assert_eq!(phase(&a, &vm).await["phase"], "unpublished");

        drop(a);
        h.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn multihost_ambiguous_middle_row_cleans_other_rows_and_preserves_binding_across_restart_and_drift()
     {
        use std::sync::atomic::Ordering::SeqCst;
        let mut h = Harness::start_with_web(false, false, true).await;
        let a = h.client(0).await;
        let host = h.supervisor().host_id;
        let zone = "b".repeat(32);
        let original = Profile::synthetic_multi(
            host,
            "docker-ambiguous",
            &[
                "a.example.invalid",
                "b.example.invalid",
                "c.example.invalid",
            ],
            Some("example.invalid"),
            &zone,
        );
        let (vm, fake) = enrolled_multi(&h, &a, 4300, true, original).await;
        let args = |n| json!({"vm_id":vm,"request_id":key(n)});

        // Row 0 succeeds; row 1 parks in late_commit (post_possible=1, record=None);
        // row 2 is never posted.
        let (late_tx, late_rx) = tokio::sync::oneshot::channel();
        fake.host_late_commit
            .lock()
            .unwrap()
            .insert("b.example.invalid".into(), late_rx);
        let pending = call(&a, "publish_site", args(4310)).await;
        until(|| fake.posts.load(SeqCst) == 2).await;

        let before = h
            .supervisor()
            .controller
            .publication_intent()
            .unwrap()
            .unwrap();
        assert!(before.hosts[0].record.is_some());
        assert!(before.hosts[1].post_possible && before.hosts[1].record.is_none());
        assert!(!before.hosts[2].post_possible && before.hosts[2].record.is_none());

        // Shutdown stops ingress, cleans row 0's owned DNS and row 2 (zero DNS),
        // and leaves row 1 ambiguous + cleanup_pending (unconfirmed drain).
        drop(a);
        assert!(
            h.shutdown().await.is_err(),
            "ambiguous middle row must make drain unconfirmed"
        );
        assert!(!fake.local.load(SeqCst));
        assert!(
            !fake
                .host_records
                .lock()
                .unwrap()
                .contains_key("a.example.invalid"),
            "row 0 owned DNS must be cleaned even while row 1 is ambiguous"
        );
        assert_eq!(fake.deletes.load(SeqCst), 1);

        // Reopen with drifted owner config: original middle-row binding and marker
        // stay pinned, and replacement publish is refused.
        let mut h = h.reopen(|_| {}).await;
        let drifted = Profile::synthetic_multi(
            host,
            "docker-ambiguous",
            &["x.other.invalid", "y.other.invalid"],
            Some("other.invalid"),
            &"d".repeat(32),
        );
        enroll(&h, &fake, true, drifted);
        let a = h.client(0).await;
        let start = call(&a, "vm_start", args(4311)).await;
        assert_eq!(
            terminal_static_start(&a, &start).await["state"],
            "succeeded"
        );
        let after = h
            .supervisor()
            .controller
            .publication_intent()
            .unwrap()
            .unwrap();
        assert_eq!(after.hosts.len(), 3);
        assert_eq!(
            after.hosts[0].state,
            crate::store::vm::publish::HostState::Cleaned
        );
        assert_eq!(after.hosts[1].hostname.as_str(), "b.example.invalid");
        assert_eq!(after.hosts[1].marker, before.hosts[1].marker);
        assert!(after.hosts[1].ambiguous);
        assert_eq!(
            after.hosts[2].state,
            crate::store::vm::publish::HostState::Cleaned
        );
        let status = phase(&a, &vm).await;
        assert_eq!(status["phase"], "cleanup_pending", "{status}");
        assert_eq!(status["hosts"][0]["state"], "cleaned");
        assert_eq!(status["hosts"][1]["state"], "cleanup_pending");
        assert_eq!(status["hosts"][1]["ambiguous"], true);
        assert_eq!(status["hosts"][2]["state"], "cleaned");
        assert_eq!(
            call(&a, "publish_site", args(4312)).await["reason"],
            "publish_cleanup_pending"
        );

        // Absence cannot discharge row 1.
        fake.routes.lock().unwrap().clear();
        h.tick().await;
        until(|| !fake.routes.lock().unwrap().is_empty()).await;
        assert_eq!(phase(&a, &vm).await["phase"], "cleanup_pending");

        // Once the late commit materializes, reconciliation finds row 1 by its
        // original pinned marker, deletes it, and releases the intent.
        late_tx.send(()).unwrap();
        until(|| {
            fake.host_records
                .lock()
                .unwrap()
                .contains_key("b.example.invalid")
        })
        .await;
        settle(&h, &a, &vm).await;
        assert_eq!(phase(&a, &vm).await["phase"], "unpublished");
        assert_eq!(fake.deletes.load(SeqCst), 2);
        assert!(fake.host_records.lock().unwrap().is_empty());
        assert_ne!(terminal(&a, &pending).await["state"], "succeeded");

        drop(a);
        h.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn multihost_partial_ownership_foreign_records_and_unpublish_under_failure() {
        use std::sync::atomic::Ordering::SeqCst;
        let mut h = Harness::start_with_web(false, false, true).await;
        let a = h.client(0).await;
        let host = h.supervisor().host_id;
        let zone = "b".repeat(32);
        let profile = Profile::synthetic_multi(
            host,
            "docker-partial",
            &["a.example.invalid", "b.example.invalid"],
            Some("example.invalid"),
            &zone,
        );
        let (vm, fake) = enrolled_multi(&h, &a, 4400, true, profile).await;
        let args = |n| json!({"vm_id":vm,"request_id":key(n)});

        // Foreign record on 2nd host blocks preflight before any connector start or POST.
        fake.host_foreign
            .lock()
            .unwrap()
            .insert("b.example.invalid".into());
        let conflict = call(&a, "publish_site", args(4410)).await;
        let term = terminal(&a, &conflict).await;
        assert_eq!(term["state"], "failed", "{term}");
        assert_eq!(term["reason"], "dns_conflict", "{term}");
        assert_eq!(fake.starts.load(SeqCst), 0);
        assert_eq!(fake.posts.load(SeqCst), 0);

        assert!(fake.starts_at_route_install.lock().unwrap().is_empty());

        // Clear foreign record and publish both hosts.
        fake.host_foreign.lock().unwrap().clear();
        let published = call(&a, "publish_site", args(4411)).await;
        assert_eq!(terminal(&a, &published).await["state"], "succeeded");
        assert_eq!(fake.posts.load(SeqCst), 2);

        // During unpublish, row 0 encounters a transient delete failure AND row 1
        // is healthy: ingress stops first, row 1 is STILL cleaned independently,
        // and row 0 remains cleanup_pending.
        fake.host_fail_remove
            .lock()
            .unwrap()
            .insert("a.example.invalid".into());
        let revoke = call(&a, "unpublish_site", args(4412)).await;
        assert_eq!(terminal(&a, &revoke).await["state"], "failed");
        assert!(!fake.local.load(SeqCst), "ingress must stop first");
        assert_eq!(
            fake.deletes.load(SeqCst),
            1,
            "row 1 must be cleaned despite row 0 failure"
        );
        assert!(
            !fake
                .host_records
                .lock()
                .unwrap()
                .contains_key("b.example.invalid"),
            "row 1 DNS must already be removed"
        );
        assert!(
            fake.host_records
                .lock()
                .unwrap()
                .contains_key("a.example.invalid"),
            "row 0 DNS still pending"
        );
        let status = phase(&a, &vm).await;
        assert_eq!(status["phase"], "cleanup_pending", "{status}");
        assert_eq!(status["hosts"][0]["state"], "cleanup_pending");
        assert_eq!(status["hosts"][1]["state"], "cleaned");

        // Now clear the transient failure on row 0, inject a foreign record on row 0:
        // owned record on row 0 is deleted, but foreign record keeps it pending
        // (preserving foreign ownership) until foreign record is cleared.
        fake.host_fail_remove.lock().unwrap().clear();
        fake.host_foreign
            .lock()
            .unwrap()
            .insert("a.example.invalid".into());
        let revoke2 = call(&a, "unpublish_site", args(4413)).await;
        assert_eq!(terminal(&a, &revoke2).await["state"], "failed");
        assert_eq!(
            fake.deletes.load(SeqCst),
            2,
            "owned record removed while foreign stays"
        );
        assert_eq!(phase(&a, &vm).await["phase"], "cleanup_pending");

        // Removing the foreign record lets final reconciliation release cleanly.
        fake.host_foreign.lock().unwrap().clear();
        settle(&h, &a, &vm).await;
        assert_eq!(phase(&a, &vm).await["phase"], "unpublished");
        assert!(fake.host_records.lock().unwrap().is_empty());

        let republish = call(&a, "publish_site", args(4414)).await;
        assert_eq!(terminal(&a, &republish).await["state"], "succeeded");
        trigger(
            &h,
            "CREATE TRIGGER refuse_row BEFORE UPDATE OF state ON vm_publication_hosts
                     WHEN NEW.position=0 AND NEW.state='cleaned'
                     BEGIN SELECT RAISE(ABORT, 'synthetic row write'); END;",
        );
        let revoke = call(&a, "unpublish_site", args(4415)).await;
        assert_eq!(terminal(&a, &revoke).await["state"], "failed");
        assert!(fake.host_records.lock().unwrap().is_empty());
        let status = phase(&a, &vm).await;
        assert_eq!(status["phase"], "cleanup_pending");
        assert_eq!(status["hosts"][1]["state"], "cleaned");
        trigger(&h, "DROP TRIGGER refuse_row");
        settle(&h, &a, &vm).await;

        drop(a);
        h.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn multihost_config_drift_requires_explicit_unpublish_before_new_set() {
        use std::sync::atomic::Ordering::SeqCst;
        let mut h = Harness::start_with_web(false, false, true).await;
        let a = h.client(0).await;
        let host = h.supervisor().host_id;
        let zone = "b".repeat(32);
        let initial = Profile::synthetic_multi(
            host,
            "docker-drift",
            &["a.example.invalid", "b.example.invalid"],
            Some("example.invalid"),
            &zone,
        );
        let (vm, fake) = enrolled_multi(&h, &a, 4500, true, initial).await;
        let args = |n| json!({"vm_id":vm,"request_id":key(n)});

        let pub1 = call(&a, "publish_site", args(4510)).await;
        assert_eq!(terminal(&a, &pub1).await["state"], "succeeded");
        assert_eq!(fake.posts.load(SeqCst), 2);

        // Drift the owner config to a 3-host set while published: no silent
        // expansion occurs while the original intent holds the publication slot.
        let expanded = Profile::synthetic_multi(
            host,
            "docker-drift",
            &[
                "a.example.invalid",
                "b.example.invalid",
                "c.example.invalid",
            ],
            Some("example.invalid"),
            &zone,
        );
        enroll(&h, &fake, true, expanded);
        let second = call(&a, "publish_site", args(4511)).await;
        assert_eq!(second["reason"], "publish_cleanup_pending", "{second}");
        assert_eq!(fake.posts.load(SeqCst), 2, "must not silently expand");
        assert!(
            !fake
                .host_records
                .lock()
                .unwrap()
                .contains_key("c.example.invalid"),
            "new host must not be published without explicit unpublish"
        );

        // Explicit unpublish tears down the original 2 pinned hosts.
        fake.remove_order.lock().unwrap().clear();
        let rev = call(&a, "unpublish_site", args(4512)).await;
        assert_eq!(terminal(&a, &rev).await["state"], "succeeded");
        assert_eq!(
            *fake.remove_order.lock().unwrap(),
            vec!["a.example.invalid", "b.example.invalid"]
        );

        // Explicit publish now applies the new 3-host set.
        let pub2 = call(&a, "publish_site", args(4513)).await;
        assert_eq!(terminal(&a, &pub2).await["state"], "succeeded");
        assert_eq!(fake.posts.load(SeqCst), 5);
        assert_eq!(phase(&a, &vm).await["host_count"], 3);
        let rev2 = call(&a, "unpublish_site", args(4514)).await;
        assert_eq!(terminal(&a, &rev2).await["state"], "succeeded");

        drop(a);
        h.shutdown().await.unwrap();
    }

    // -------------------------------------------------------------------------
    // Docker E4 protected SQLite backup tests
    // -------------------------------------------------------------------------

    pub(crate) fn backup_fixture_root() -> tempfile::TempDir {
        let root = crate::service::application::tests::private_temp();
        protect_fixture(root.path(), true).unwrap();
        for name in ["settled", "staging"] {
            let child = root.path().join(name);
            fs::create_dir(&child).unwrap();
            protect_fixture(&child, true).unwrap();
        }
        root
    }

    #[test]
    fn backup_fixture_custody_directories_have_the_current_owner() {
        let root = backup_fixture_root();
        for path in [
            root.path().to_owned(),
            root.path().join("staging"),
            root.path().join("settled"),
        ] {
            crate::local::require_owner(&path).unwrap();
            crate::security::private(&path).unwrap();
        }
    }

    fn enroll_backup_project(
        h: &Harness,
        vm_id: Id,
        project: &str,
        release_id: Id,
        max_bytes: u64,
    ) -> (compose::Target, Id) {
        use crate::store::vm::compose as db;
        use arx_guest::compose as guest;

        let vm = hex::encode(vm_id);
        let enrollment: compose::ReleaseInput = serde_json::from_value(json!({
            "vm_id": vm,
            "release_id": hex::encode(release_id),
            "manifest": {
                "schema": guest::MANIFEST_SCHEMA,
                "project_id": project,
                "volumes": [{"name": "data", "uid": 10001, "gid": 10001}],
                "secrets": [],
                "services": [{
                    "name": "web",
                    "image": format!("example.invalid/calm@sha256:{}", "c".repeat(64)),
                    "platform": guest::PLATFORM,
                    "uid": 10001,
                    "gid": 10001,
                    "read_only_rootfs": true,
                    "http_port": 8080,
                    "health_path": "/health",
                    "environment": [],
                    "mounts": [{"volume": "data", "target": "/data", "read_only": false}],
                    "secrets": [],
                    "limits": {"cpu_millis": 500, "memory_mib": 512, "pids": 64}
                }]
            }
        }))
        .unwrap();
        let enrolled = enrollment.enroll().unwrap();
        let target = enrolled.target.clone();
        let store = h.supervisor().controller.compose_store();
        db::enroll(store, &[enrolled]).unwrap();
        let generation = store
            .transact(|tx| db::boot(tx, "alpha", &target))
            .unwrap()
            .unwrap();
        // Mark revision 1 committed on this project so backups have a committed anchor.
        store
            .transact(|tx| {
                tx.execute(
                    "UPDATE vm_compose_projects SET desired='started', last_revision=1, committed=1,
                     committed_release=?3, rendered_revision=1, rendered_release=?3, settled_generation=?4
                     WHERE vm=?1 AND project=?2",
                    rusqlite::params![vm_id, project, release_id, generation],
                )?;
                Ok(())
            })
            .unwrap();
        h.supervisor()
            .controller
            .enroll_compose_backup_target(vm_id, project, "data", "calm-life.sqlite3", max_bytes)
            .unwrap();

        (target, generation)
    }

    async fn setup_compose_backup_project(
        h: &Harness,
        a: &Client,
        key_base: u64,
        max_bytes: u64,
    ) -> (
        Value,
        Id,
        Id,
        compose::Target,
        tempfile::TempDir,
        Arc<compose_backup::ManagedBackupDir>,
    ) {
        let (vm, _) = enrolled(h, a, key_base).await;
        let vm_id = parse_id(vm.as_str().unwrap()).unwrap();
        let (target, generation) =
            enroll_backup_project(h, vm_id, "calm-life", [0x61; 16], max_bytes);
        let backup_root = backup_fixture_root();
        let managed = Arc::new(
            compose_backup::ManagedBackupDir::new(backup_root.path(), h.temp.path(), None).unwrap(),
        );
        *h.supervisor().compose_backup.custody.lock().unwrap() = Some(managed.clone());
        *h.supervisor().compose_backup.capacity.lock().unwrap() = Some(Capacity {
            free_bytes: 150 * 1024 * 1024 * 1024,
            managed_bytes: 0,
        });
        (vm, vm_id, generation, target, backup_root, managed)
    }

    async fn poll_compose_backup_terminal(client: &Client, args: &Value) -> Value {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            let reply = call(client, "compose_backup", args.clone()).await;
            if reply["state"] != "pending" {
                return reply;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "compose_backup timed out: {reply}"
            );
            tokio::time::sleep(Duration::from_millis(15)).await;
        }
    }

    fn make_wal_sqlite_bytes(
        dir: &Path,
        name: &str,
        rows: &[(&str, i64)],
        pad_bytes: usize,
    ) -> (Vec<u8>, String) {
        let vol = dir.join(format!("vol-{name}"));
        let stage = dir.join(format!("stage-{name}"));
        fs::create_dir_all(&vol).unwrap();
        fs::create_dir_all(&stage).unwrap();
        let db_path = vol.join("calm-life.sqlite3");
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA wal_autocheckpoint=0;
             CREATE TABLE events(id INTEGER PRIMARY KEY, label TEXT NOT NULL, val INTEGER NOT NULL);
             CREATE TABLE pad(id INTEGER PRIMARY KEY, data BLOB NOT NULL);",
        )
        .unwrap();
        for (label, val) in rows {
            conn.execute(
                "INSERT INTO events(label, val) VALUES(?1, ?2)",
                rusqlite::params![label, val],
            )
            .unwrap();
        }
        if pad_bytes > 0 {
            let blob = vec![0x5au8; pad_bytes];
            conn.execute("INSERT INTO pad(data) VALUES(?1)", [blob])
                .unwrap();
        }
        assert!(vol.join("calm-life.sqlite3-wal").exists());
        let staged = stage.join("out.sqlite3");
        let meta = arx_guest::compose_backup::backup_sqlite_online(
            &vol,
            "calm-life.sqlite3",
            &staged,
            arx_guest::compose_backup::MAX_BACKUP_BYTES,
            std::time::Instant::now() + Duration::from_secs(15),
        )
        .unwrap();
        drop(conn);
        let bytes = fs::read(&staged).unwrap();
        assert_eq!(bytes.len() as u64, meta.total_bytes);
        assert_eq!(&bytes[18..20], &[1, 1]);
        (bytes, meta.digest)
    }

    async fn backup_test_peer(h: &Harness, vm: Id, generation: Id, boot: u64) -> DuplexStream {
        let (host_io, mut peer) = tokio::io::duplex(256 * 1024);
        let supervisor = h.supervisor();
        let (channel, ()) = tokio::join!(
            crate::vm::guest::ComposeChannel::negotiate(host_io, boot, &supervisor.cancel),
            fake_compose_hello(&mut peer, boot)
        );
        supervisor
            .controller
            .compose_store()
            .transact(|tx| crate::store::vm::compose::register_boot(tx, vm, generation, boot))
            .unwrap();
        supervisor
            .compose_backup
            .attach(
                vm,
                compose_backup::BackupSession::new(generation, channel.unwrap()),
            )
            .unwrap();
        peer
    }

    async fn serve_backup_payloads(
        mut peer: DuplexStream,
        boot: u64,
        mut payloads: std::collections::VecDeque<(Vec<u8>, String)>,
        releases: Arc<AtomicU64>,
    ) {
        use arx_guest::compose::{
            ComposeRequest as Req, ComposeResponse as Resp, OperationKind, OperationStage,
            OperationState, OperationView, RejectCode, encode_base64,
        };
        use arx_guest::compose_backup::{BACKUP_CHUNK_BYTES, chunk_count};

        let mut active: Option<(String, Vec<u8>, String)> = None;
        loop {
            use tokio::io::AsyncReadExt;
            let Ok(len) = peer.read_u32().await else {
                break;
            };
            let mut buf = vec![0; len as usize];
            if peer.read_exact(&mut buf).await.is_err() {
                break;
            }
            let req: Req = serde_json::from_slice(&buf).unwrap();
            match req {
                Req::ComposeOperation { id, operation_id } => {
                    let resp = match &active {
                        Some((op, _, _)) if *op == operation_id => Resp::ComposeOperationState {
                            id,
                            operation: OperationView {
                                operation_id,
                                kind: OperationKind::Backup,
                                project_id: "calm-life".into(),
                                revision: 1,
                                state: OperationState::Succeeded,
                                stage: OperationStage::Settled,
                                failure: None,
                                restored_previous: false,
                            },
                        },
                        _ => Resp::ComposeRejected {
                            id,
                            code: RejectCode::UnknownOperation,
                            rule: None,
                        },
                    };
                    fake_compose_reply(&mut peer, resp).await;
                }
                Req::ComposeBackupBegin {
                    id,
                    operation_id,
                    generation: g,
                    project_id,
                    volume,
                    database,
                    ..
                } => {
                    assert_eq!(g, boot);
                    assert_eq!(project_id, "calm-life");
                    assert_eq!(volume, "data");
                    assert_eq!(database, "calm-life.sqlite3");
                    let (data, digest) = payloads.pop_front().unwrap();
                    active = Some((operation_id.clone(), data, digest));
                    fake_compose_reply(
                        &mut peer,
                        Resp::ComposeOperationState {
                            id,
                            operation: OperationView {
                                operation_id,
                                kind: OperationKind::Backup,
                                project_id,
                                revision: 1,
                                state: OperationState::Succeeded,
                                stage: OperationStage::Settled,
                                failure: None,
                                restored_previous: false,
                            },
                        },
                    )
                    .await;
                }
                Req::ComposeBackupMeta { id, operation_id } => {
                    let (op, data, digest) = active.as_ref().unwrap();
                    assert_eq!(op, &operation_id);
                    fake_compose_reply(
                        &mut peer,
                        Resp::ComposeBackupReady {
                            id,
                            operation_id,
                            project_id: "calm-life".into(),
                            revision: 1,
                            total_bytes: data.len() as u64,
                            chunks: chunk_count(data.len() as u64).unwrap(),
                            digest: digest.clone(),
                        },
                    )
                    .await;
                }
                Req::ComposeBackupChunk {
                    id,
                    operation_id,
                    index,
                } => {
                    let (op, data, _) = active.as_ref().unwrap();
                    assert_eq!(op, &operation_id);
                    let start = index as usize * BACKUP_CHUNK_BYTES;
                    let end = (start + BACKUP_CHUNK_BYTES).min(data.len());
                    fake_compose_reply(
                        &mut peer,
                        Resp::ComposeBackupData {
                            id,
                            operation_id,
                            index,
                            data: encode_base64(&data[start..end]),
                        },
                    )
                    .await;
                }
                Req::ComposeBackupRelease { id, operation_id } => {
                    if active
                        .as_ref()
                        .is_some_and(|(op, _, _)| op == &operation_id)
                    {
                        active = None;
                    }
                    releases.fetch_add(1, Ordering::SeqCst);
                    fake_compose_reply(
                        &mut peer,
                        Resp::ComposeBackupReleased {
                            id,
                            operation_id,
                            removed: true,
                        },
                    )
                    .await;
                }
                _ => panic!("unexpected compose request"),
            }
        }
    }

    fn admit_test_backup(
        h: &Harness,
        target: &compose::Target,
        nonce: u64,
    ) -> compose_backup::BackupIntent {
        let supervisor = h.supervisor();
        let session = supervisor.compose_backup.session(target.vm).unwrap();
        let now = supervisor.controller.compose_now().unwrap();
        supervisor
            .controller
            .compose_store()
            .transact(|tx| {
                Ok(crate::store::vm::compose::admit_backup(
                    tx,
                    "alpha",
                    target,
                    RequestId::parse(&key(nonce)).unwrap(),
                    session.generation,
                    session.wire_boot,
                    now,
                    |_, _| Ok(Ok(())),
                )?
                .unwrap())
            })
            .unwrap()
    }

    fn replay_test_backup(h: &Harness, nonce: u64) -> compose_backup::BackupIntent {
        h.supervisor()
            .controller
            .compose_store()
            .transact(|tx| {
                crate::store::vm::compose::backup_replay(
                    tx,
                    "alpha",
                    RequestId::parse(&key(nonce)).unwrap().key,
                )
            })
            .unwrap()
            .unwrap()
    }

    async fn drive_test_backup(
        h: &Harness,
        target: &compose::Target,
        nonce: u64,
        cancel: &Shutdown,
    ) -> Result<()> {
        let supervisor = h.supervisor();
        supervisor
            .compose_backup
            .drive(
                &supervisor.controller,
                "alpha",
                RequestId::parse(&key(nonce)).unwrap().key,
                target,
                supervisor.compose_backup.session(target.vm).unwrap(),
                cancel,
            )
            .await
    }

    async fn assert_no_queued_frame(peer: &mut DuplexStream) {
        tokio::select! {
            biased;
            byte = peer.read_u8() => panic!("unexpected guest traffic or channel closure: {byte:?}"),
            () = std::future::ready(()) => (),
        }
    }

    #[tokio::test]
    async fn compose_backup_rechecks_held_capacity_before_guest_io_and_host_staging() {
        use compose_backup::{BackupHost, BackupOpStatus};
        let h = Harness::start_with_web(false, false, true).await;
        let client = h.client(0).await;
        let (_, vm, generation, target, root, managed) =
            setup_compose_backup_project(&h, &client, 5900, 64 * 1024).await;
        let boot = 5901;
        let mut peer = backup_test_peer(&h, vm, generation, boot).await;
        let supervisor = h.supervisor();
        let controller = &supervisor.controller;
        let session = supervisor.compose_backup.session(vm).unwrap();

        let mut backup = BackupHost::default();
        let samples = Arc::new(AtomicU64::new(0));
        let counter = samples.clone();
        backup
            .install_native(controller.compose_store(), managed, move || {
                matches!(counter.fetch_add(1, Ordering::SeqCst), 1 | 2).then_some(Capacity {
                    free_bytes: 150 << 30,
                    managed_bytes: 0,
                })
            })
            .unwrap();

        let cancel = Shutdown::default();
        let rejected = admit_test_backup(&h, &target, 5910);
        backup
            .drive(
                controller,
                "alpha",
                RequestId::parse(&key(5910)).unwrap().key,
                &target,
                session.clone(),
                &cancel,
            )
            .await
            .unwrap();
        assert_no_queued_frame(&mut peer).await;
        assert_eq!(replay_test_backup(&h, 5910).status, BackupOpStatus::Failed);

        let releases = Arc::new(AtomicU64::new(0));
        let payload = vec![0u8; 512];
        let digest = hex::encode(Sha256::digest(&payload));
        let guest = tokio::spawn(serve_backup_payloads(
            peer,
            boot,
            [(payload, digest)].into(),
            releases.clone(),
        ));
        let lost = admit_test_backup(&h, &target, 5911);
        backup
            .drive(
                controller,
                "alpha",
                RequestId::parse(&key(5911)).unwrap().key,
                &target,
                session,
                &cancel,
            )
            .await
            .unwrap();
        assert_eq!(samples.load(Ordering::SeqCst), 4);
        assert_eq!(releases.load(Ordering::SeqCst), 1);
        for (nonce, intent) in [(5910, rejected), (5911, lost)] {
            let settled = replay_test_backup(&h, nonce);
            assert_eq!(settled.status, BackupOpStatus::Failed);
            controller
                .compose_store()
                .transact(|tx| {
                    let row: (String, u64, Option<String>) = tx.query_row(
                        "SELECT b.state,b.host_staging_charge,o.failure
                     FROM vm_compose_backups b JOIN vm_compose_backup_ops o ON o.backup_id=b.id
                     WHERE b.id=?1",
                        [intent.backup_id],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                    )?;
                    assert_eq!(
                        row,
                        (
                            "cleaned".into(),
                            0,
                            Some("backup_capacity_unavailable".into())
                        )
                    );
                    Ok(())
                })
                .unwrap();
        }
        for leaf in ["staging", "settled"] {
            assert!(
                std::fs::read_dir(root.path().join(leaf))
                    .unwrap()
                    .next()
                    .is_none()
            );
        }
        guest.abort();
    }

    #[tokio::test]
    async fn compose_backup_queued_project_releases_lease_on_deadline_and_cancel() {
        use arx_guest::compose::{ComposeRequest as Req, ComposeResponse as Resp, RejectCode};
        use compose_backup::{BackupOpStatus, BackupRecordState};
        let mut h = Harness::start_with_web(false, false, true).await;
        let client = h.client(0).await;
        let cap = 256 * 1024;
        let (_, vm, generation, first, _root, _managed) =
            setup_compose_backup_project(&h, &client, 5700, cap).await;
        let (queued, _) = enroll_backup_project(&h, vm, "queued-app", [0x62; 16], cap);
        let mut peer = backup_test_peer(&h, vm, generation, 5701).await;
        let supervisor = h.supervisor();
        let disk = || {
            supervisor
                .controller
                .compose_store()
                .transact(|tx| crate::store::vm::disk_bytes(tx))
                .unwrap()
                .unwrap()
        };
        let initial_disk = disk();
        let holder = admit_test_backup(&h, &first, 5710);
        {
            let active = drive_test_backup(&h, &first, 5710, &supervisor.cancel);
            tokio::pin!(active);
            let query = tokio::select! {
                result = &mut active => panic!("first backup settled before query: {result:?}"),
                request = fake_compose_read(&mut peer) => request,
            };
            let Req::ComposeOperation { id, operation_id } = query else {
                panic!("expected holder query")
            };
            assert_eq!(operation_id, hex::encode(holder.id));

            // Keep the first driver's query unanswered while both queued operations finish.
            for (nonce, shutdown) in [(5711, false), (5712, true)] {
                supervisor.compose_backup.timeouts.lock().unwrap().absolute = if shutdown {
                    Duration::from_secs(30)
                } else {
                    Duration::from_millis(200)
                };
                let pending = admit_test_backup(&h, &queued, nonce);
                assert_eq!(pending.host_staging_charge, cap);
                assert_eq!(disk(), initial_disk + 2 * cap);
                let cancel = Shutdown::default();
                let waiting = drive_test_backup(&h, &queued, nonce, &cancel);
                tokio::pin!(waiting);
                tokio::select! {
                    biased;
                    result = &mut waiting => panic!("queued operation did not wait: {result:?}"),
                    () = std::future::ready(()) => (),
                }
                if shutdown {
                    cancel.cancel();
                }
                tokio::time::timeout(Duration::from_secs(3), waiting)
                    .await
                    .expect("queued backup held its lease")
                    .unwrap();
                let done = replay_test_backup(&h, nonce);
                assert_eq!(done.status, BackupOpStatus::Interrupted);
                assert_eq!(done.backup_state, BackupRecordState::Cleaned);
                assert_eq!(done.host_staging_charge, 0);
                assert!(!done.sent);
                assert_eq!(disk(), initial_disk + cap);
                assert_no_queued_frame(&mut peer).await;
            }
            fake_compose_reply(
                &mut peer,
                Resp::ComposeRejected {
                    id,
                    code: RejectCode::Busy,
                    rule: None,
                },
            )
            .await;
            active.await.unwrap();
        }
        assert_eq!(disk(), initial_disk);
        drop(client);
        h.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn compose_backup_postpublication_commit_failure_and_witness_retry_keep_charge() {
        use compose_backup::{BackupOpStatus, BackupRecordState};
        let mut h = Harness::start_with_web(false, false, true).await;
        let client = h.client(0).await;
        let cap = 256 * 1024;
        let (vm_value, vm, generation, target, root, managed) =
            setup_compose_backup_project(&h, &client, 5800, cap).await;
        let source = tempfile::tempdir().unwrap();
        let payload = make_wal_sqlite_bytes(source.path(), "commit", &[("entry", 1)], 0);
        let peer = backup_test_peer(&h, vm, generation, 5801).await;
        let task = tokio::spawn(serve_backup_payloads(
            peer,
            5801,
            [payload.clone(), payload].into(),
            Arc::new(AtomicU64::new(0)),
        ));
        let supervisor = h.supervisor();
        let store = supervisor.controller.compose_store();
        let disk = || {
            store
                .transact(|tx| crate::store::vm::disk_bytes(tx))
                .unwrap()
                .unwrap()
        };
        let baseline = disk();
        for (nonce, retry) in [(5810, false), (5811, true)] {
            let intent = admit_test_backup(&h, &target, nonce);
            let staging = root
                .path()
                .join("staging")
                .join(format!("{}.part", hex::encode(intent.id)));
            let settled = managed.settled_path(intent.backup_id).unwrap();
            store.transact(|tx| {
                tx.execute_batch("CREATE TEMP TRIGGER reject_commit BEFORE UPDATE OF state ON vm_compose_backups
                    WHEN NEW.state='settled' BEGIN SELECT RAISE(ABORT,'fixture commit failure'); END;")?;
                if retry {
                    tx.execute_batch("CREATE TEMP TRIGGER reject_failure BEFORE UPDATE OF status ON vm_compose_backup_ops
                        WHEN NEW.status='interrupted' BEGIN SELECT RAISE(ABORT,'fixture settlement failure'); END;")?;
                }
                Ok(())
            }).unwrap();
            assert!(
                drive_test_backup(&h, &target, nonce, &supervisor.cancel)
                    .await
                    .is_err()
            );
            store
                .transact(|tx| {
                    Ok(tx.execute_batch(
                        "DROP TRIGGER reject_commit; DROP TRIGGER IF EXISTS reject_failure",
                    )?)
                })
                .unwrap();
            assert!(staging.is_file() && settled.is_file());
            if retry {
                let pending = replay_test_backup(&h, nonce);
                assert_eq!(pending.status, BackupOpStatus::Pending);
                assert!(pending.has_staging_witness);
                assert_eq!(pending.host_staging_charge, cap);
                // A retry during shutdown must honor the persisted witness before any clean exit.
                let cancel = Shutdown::default();
                cancel.cancel();
                drive_test_backup(&h, &target, nonce, &cancel)
                    .await
                    .unwrap();
            }
            let terminal = replay_test_backup(&h, nonce);
            assert_eq!(terminal.status, BackupOpStatus::Interrupted);
            assert_eq!(
                terminal.failure.as_deref(),
                Some("attribution_commit_failed")
            );
            assert_eq!(terminal.backup_state, BackupRecordState::PartialUncertain);
            assert_eq!(terminal.host_staging_charge, cap);
            assert_eq!(disk(), baseline + cap);
            assert!(settled.is_file() && staging.is_file());
            assert_eq!(
                supervisor.compose_backup.reconcile_partials(store).unwrap(),
                1
            );
            assert!(!settled.exists() && !staging.exists());
            assert_eq!(disk(), baseline);
            let response = call(
                &client,
                "compose_backup",
                json!({"vm_id":vm_value,"project_id":target.project,"request_id":key(nonce)}),
            )
            .await;
            assert_eq!(response["state"], "interrupted");
            assert!(response.get("backup_id").is_none());
        }
        task.abort();
        drop(client);
        h.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn compose_backup_refuses_guest_named_unknown_and_pending_owners() {
        use arx_guest::compose::{ComposeRequest as Req, ComposeResponse as Resp, RejectCode};
        let mut h = Harness::start_with_web(false, false, true).await;
        let client = h.client(0).await;
        let cap = 256 * 1024;
        let (_, vm, generation, target, _root, _managed) =
            setup_compose_backup_project(&h, &client, 5900, cap).await;
        let mut peer = backup_test_peer(&h, vm, generation, 5901).await;
        let supervisor = h.supervisor();
        supervisor.compose_backup.timeouts.lock().unwrap().idle = Duration::from_secs(1);
        for (nonce, unknown) in [(5910, true), (5911, false)] {
            let intent = admit_test_backup(&h, &target, nonce);
            let owner = if unknown { [0xA5; 16] } else { intent.id };
            let responder = async {
                let Req::ComposeOperation { id, .. } = fake_compose_read(&mut peer).await else {
                    panic!("expected query")
                };
                fake_compose_reply(
                    &mut peer,
                    Resp::ComposeRejected {
                        id,
                        code: RejectCode::UnknownOperation,
                        rule: None,
                    },
                )
                .await;
                let Req::ComposeBackupBegin { id, .. } = fake_compose_read(&mut peer).await else {
                    panic!("expected begin")
                };
                fake_compose_reply(
                    &mut peer,
                    Resp::ComposeBackupOccupied {
                        id,
                        operation_id: hex::encode(owner),
                    },
                )
                .await;
            };
            let (result, ()) = tokio::join!(
                drive_test_backup(&h, &target, nonce, &supervisor.cancel),
                responder
            );
            result.unwrap();
            let done = replay_test_backup(&h, nonce);
            assert_eq!(done.failure.as_deref(), Some("backup_staging_occupied"));
            assert_eq!(done.status, compose_backup::BackupOpStatus::Failed);
            assert_no_queued_frame(&mut peer).await;
        }
        drop(client);
        h.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn compose_backup_wal_end_to_end_restore_retention_cap_and_delete_authority() {
        use arx_guest::compose_backup::BACKUP_CHUNK_BYTES;
        let mut h = Harness::start_with_web(false, false, true).await;
        let a = h.client(0).await;
        let max_bytes = 2 * 1024 * 1024;
        let (vm, vm_id, generation, target, _backup_dir, managed) =
            setup_compose_backup_project(&h, &a, 5100, max_bytes).await;
        let fixture_src = tempfile::tempdir().unwrap();
        let (bytes1, digest1) = make_wal_sqlite_bytes(
            fixture_src.path(),
            "first",
            &[("boot", 1), ("wal_tx_one", 77)],
            24 * 1024,
        );
        let (bytes2, digest2) = make_wal_sqlite_bytes(
            fixture_src.path(),
            "second",
            &[("boot", 1), ("wal_tx_one", 77), ("wal_tx_two", 88)],
            0,
        );
        // Backup #1 genuinely spans multiple 16 KiB serial chunks.
        assert!(bytes1.len() > BACKUP_CHUNK_BYTES);

        let boot = 5101;
        let peer = backup_test_peer(&h, vm_id, generation, boot).await;
        let releases_seen = Arc::new(AtomicU64::new(0));
        let guest_task = tokio::spawn(serve_backup_payloads(
            peer,
            boot,
            [
                (bytes1.clone(), digest1.clone()),
                (bytes2.clone(), digest2.clone()),
            ]
            .into(),
            releases_seen.clone(),
        ));
        let store = h.supervisor().controller.compose_store();
        let disk_before = store
            .transact(|tx| crate::store::vm::disk_bytes(tx))
            .unwrap()
            .unwrap();

        // Backup #1 succeeds and returns opaque backup_id without any host path.
        let req1 = json!({"vm_id": vm, "project_id": "calm-life", "request_id": key(5110)});
        let b1 = poll_compose_backup_terminal(&a, &req1).await;
        assert_eq!(b1["state"], "succeeded", "{b1}");
        assert_eq!(b1["size_bytes"], bytes1.len() as u64);
        assert_eq!(b1["sha256"], digest1);
        let b1_id = parse_id(b1["backup_id"].as_str().unwrap()).unwrap();
        assert!(!b1.to_string().contains("settled"));
        assert!(!b1.to_string().contains(".sqlite3"));
        // Exact replay returns the same settled response.
        assert_eq!(call(&a, "compose_backup", req1.clone()).await, b1);
        // Same request_id on a different project collides.
        assert_eq!(
            call(
                &a,
                "compose_backup",
                json!({"vm_id": vm, "project_id": "other-app", "request_id": key(5110)}),
            )
            .await["reason"],
            "key_collision"
        );

        // Settled backup leaf is portable (header bytes 18/19 = 1/1, no sidecars).
        let settled1_path = managed.settled_path(b1_id).unwrap();
        let settled1_bytes = fs::read(&settled1_path).unwrap();
        assert_eq!(&settled1_bytes[18..20], &[1, 1]);
        for suffix in ["-wal", "-shm", "-journal"] {
            assert!(
                !settled1_path
                    .with_file_name(format!(
                        "{}{suffix}",
                        settled1_path.file_name().unwrap().to_str().unwrap()
                    ))
                    .exists()
            );
        }

        // Offline restore into a disposable owner-controlled target verifies WAL rows!
        let restore_dir = tempfile::tempdir().unwrap();
        let mut sha1 = [0u8; 32];
        hex::decode_to_slice(&digest1, &mut sha1).unwrap();
        // Restore target inside backup root is refused.
        let nested_in_backup = _backup_dir.path().join("nested-restore");
        fs::create_dir_all(&nested_in_backup).unwrap();
        assert!(
            managed
                .verify_offline_restore(b1_id, &nested_in_backup, "bad.sqlite3", sha1, max_bytes)
                .is_err()
        );
        let restored = managed
            .verify_offline_restore(
                b1_id,
                restore_dir.path(),
                "restored1.sqlite3",
                sha1,
                max_bytes,
            )
            .unwrap();
        let restored_bytes = fs::read(&restored.path).unwrap();
        assert_eq!(&restored_bytes[18..20], &[1, 1]);
        for suffix in ["-wal", "-shm", "-journal"] {
            assert!(
                !restore_dir
                    .path()
                    .join(format!("restored1.sqlite3{suffix}"))
                    .exists()
            );
        }
        // Existing target leaf is refused, and settled backup bytes stay untouched.
        assert!(
            managed
                .verify_offline_restore(
                    b1_id,
                    restore_dir.path(),
                    "restored1.sqlite3",
                    sha1,
                    max_bytes,
                )
                .is_err()
        );
        assert_eq!(fs::read(&settled1_path).unwrap(), settled1_bytes);
        let conn = rusqlite::Connection::open(&restored.path).unwrap();
        let rows: Vec<(String, i64)> = conn
            .prepare("SELECT label, val FROM events ORDER BY id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(rows, vec![("boot".into(), 1), ("wal_tx_one".into(), 77)]);
        drop(conn);

        // Backup #2 succeeds (2nd retained copy).
        let req2 = json!({"vm_id": vm, "project_id": "calm-life", "request_id": key(5111)});
        let b2 = poll_compose_backup_terminal(&a, &req2).await;
        assert_eq!(b2["state"], "succeeded", "{b2}");
        assert_eq!(b2["size_bytes"], bytes2.len() as u64);
        assert_eq!(b2["sha256"], digest2);
        let b2_id = parse_id(b2["backup_id"].as_str().unwrap()).unwrap();

        // Retained charge equals exact sum of both settled backups.
        let disk_after = store
            .transact(|tx| crate::store::vm::disk_bytes(tx))
            .unwrap()
            .unwrap();
        assert_eq!(
            disk_after - disk_before,
            bytes1.len() as u64 + bytes2.len() as u64
        );
        until(|| releases_seen.load(Ordering::SeqCst) == 2).await;

        // Backup #3 is REFUSED at the 2-copy per-project retention cap!
        let req3 = json!({"vm_id": vm, "project_id": "calm-life", "request_id": key(5112)});
        let refused3 = call(&a, "compose_backup", req3).await;
        assert_eq!(refused3["reason"], "backup_limit", "{refused3}");

        // Agent delete-authority refusals: MCP surface + SQLite immutability triggers.
        for tool in [
            "compose_backup_delete",
            "compose_volume_delete",
            "compose_project_delete",
            "compose_backup_restore",
        ] {
            let r = call(&a, tool, json!({"vm_id": vm, "backup_id": b1["backup_id"]})).await;
            assert_eq!(r["reason"], "static_backup_delete_forbidden", "{tool}: {r}");
        }
        for tool in ["backup_delete", "vm_backup_delete", "static_vm_delete"] {
            let r = call(&a, tool, json!({"vm_id": vm})).await;
            assert_eq!(r["reason"], "static_delete_forbidden", "{tool}: {r}");
        }
        for tool in ["backup-retire", "backup_retire", "compose_backup_retire"] {
            let r = call(&a, tool, json!({"vm_id": vm, "backup_id": b1["backup_id"]})).await;
            assert_eq!(r["reason"], "unknown_tool", "{tool}: {r}");
        }
        let snap_del = call(
            &a,
            "vm_snapshot_delete",
            json!({"vm_id": vm, "snapshot_id": b1["backup_id"], "request_id": key(5113)}),
        )
        .await;
        assert_eq!(snap_del["reason"], "static_backup_delete_forbidden");
        // Another principal learns nothing about the backup ID, and an unknown
        // ID on the caller's own VM stays not_found.
        let b = h.client(1).await;
        assert_eq!(
            call(
                &b,
                "vm_snapshot_delete",
                json!({"vm_id": vm, "snapshot_id": b1["backup_id"], "request_id": key(5114)}),
            )
            .await["reason"],
            "not_found"
        );
        assert_eq!(
            call(
                &a,
                "vm_snapshot_delete",
                json!({"vm_id": vm, "snapshot_id": hex::encode([0x5a; 16]), "request_id": key(5115)}),
            )
            .await["reason"],
            "not_found"
        );
        assert_eq!(
            call(
                &a,
                "vm_snapshot_restore",
                json!({"vm_id": vm, "snapshot_id": b1["backup_id"], "request_id": key(5116)}),
            )
            .await["reason"],
            "not_found"
        );
        drop(b);

        // SQLite BEFORE DELETE / BEFORE UPDATE triggers refuse mutating settled backups.
        assert!(
            store
                .transact(|tx| {
                    tx.execute("DELETE FROM vm_compose_backups WHERE id=?1", [b1_id])?;
                    Ok(())
                })
                .is_err()
        );
        assert!(
            store
                .transact(|tx| {
                    tx.execute(
                        "UPDATE vm_compose_backups SET state='cleaned', retained_charge=0 WHERE id=?1",
                        [b1_id],
                    )?;
                    Ok(())
                })
                .is_err()
        );

        // Drive pressure admission against the same host and await a full reaper sweep.
        let template = store
            .transact(|tx| {
                Ok(
                    tx.query_row("SELECT id FROM vm_templates LIMIT 1", [], |r| {
                        r.get::<_, Id>(0)
                    })?,
                )
            })
            .unwrap();
        let stop = call(&a, "vm_stop", json!({"vm_id": vm,"request_id":key(5141)})).await;
        assert_eq!(terminal(&a, &stop).await["state"], "succeeded");
        let create = call(
            &a,
            "vm_create",
            json!({"template_id":hex::encode(template), "request_id":key(5140),"ttl_s":60}),
        )
        .await;
        let victim = terminal(&a, &create).await;
        assert_eq!(victim["state"], "succeeded", "{victim}");
        h.fixture(|f| {
            f.host.capacity = Some(Capacity {
                free_bytes: 0,
                managed_bytes: 0,
            })
        });
        let pressure = call(
            &a,
            "static_vm_create",
            json!({
                "name":"pressure", "template_id":hex::encode(template), "request_id":key(5117)
            }),
        )
        .await;
        assert_eq!(pressure["reason"], "disk", "{pressure}");
        let test_id = parse_id(victim["vm_id"].as_str().unwrap()).unwrap();
        let proof = || {
            store
                .transact(|tx| crate::store::vm::tombstone_proof(tx, "alpha", test_id))
                .unwrap()
        };
        assert!(proof().is_none());
        h.now
            .store(create["expires_at_ms"].as_i64().unwrap(), Ordering::Relaxed);
        h.sweep().await;
        assert!(proof().is_some());
        let tombstone = call(&a, "vm_get", json!({"vm_id":victim["vm_id"]})).await;
        assert_eq!(tombstone["cleanup"], "confirmed", "{tombstone}");
        assert_eq!(tombstone["state"], "expired");
        assert!(managed.settled_path(b1_id).unwrap().exists());
        assert!(managed.settled_path(b2_id).unwrap().exists());
        let records = store
            .transact(|tx| crate::store::vm::compose::list_project_backups(tx, &target))
            .unwrap();
        assert_eq!(records.len(), 2);
        assert!(
            records
                .iter()
                .all(|r| r.state == compose_backup::BackupRecordState::Settled)
        );

        drop(a);
        guest_task.abort();
        h.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn compose_backup_serial_digest_cap_sequence_query_retry_cancel_and_uncertain_partial() {
        use arx_guest::compose::{
            ComposeRequest as Req, ComposeResponse as Resp, OperationKind, OperationStage,
            OperationState, OperationView, RejectCode, encode_base64,
        };
        use arx_guest::compose_backup::{BACKUP_CHUNK_BYTES, chunk_count};

        let mut h = Harness::start_with_web(false, false, true).await;
        let a = h.client(0).await;
        let max_bytes = 256 * 1024;
        let (vm, vm_id, generation, target, _backup_dir, _managed) =
            setup_compose_backup_project(&h, &a, 5200, max_bytes).await;
        let fixture_src = tempfile::tempdir().unwrap();
        let (good_bytes, good_digest) = make_wal_sqlite_bytes(
            fixture_src.path(),
            "serial",
            &[("row", 1), ("row2", 2)],
            24 * 1024,
        );
        assert!(good_bytes.len() > BACKUP_CHUNK_BYTES);
        let store = h.supervisor().controller.compose_store();

        // Helper to attach a one-shot custom fake serial handler on a given boot.
        async fn attach_peer(
            h: &Harness,
            vm_id: Id,
            generation: Id,
            boot: u64,
            peer_fn: impl FnOnce(DuplexStream) -> tokio::task::JoinHandle<()>,
        ) -> tokio::task::JoinHandle<()> {
            let (host_io, peer) = tokio::io::duplex(256 * 1024);
            let task = peer_fn(peer);
            let channel =
                crate::vm::guest::ComposeChannel::negotiate(host_io, boot, &h.supervisor().cancel)
                    .await
                    .unwrap();
            h.supervisor()
                .controller
                .compose_store()
                .transact(|tx| {
                    // A new fake serial peer represents a new boot; retire the
                    // previous active boot row first (one active boot per VM).
                    tx.execute(
                        "UPDATE vm_compose_boots SET retired=1 WHERE vm=?1 AND retired=0 AND boot<>?2",
                        rusqlite::params![vm_id, boot],
                    )?;
                    crate::store::vm::compose::register_boot(tx, vm_id, generation, boot)
                })
                .unwrap();
            h.supervisor()
                .compose_backup
                .attach(
                    vm_id,
                    compose_backup::BackupSession::new(generation, channel),
                )
                .unwrap();
            task
        }

        // Case 1: Digest mismatch (tampered chunk bytes) -> fails & cleans staging.
        let b1 = good_bytes.clone();
        let d1 = good_digest.clone();
        let t1 = attach_peer(&h, vm_id, generation, 5201, move |mut peer| {
            tokio::spawn(async move {
                fake_compose_hello(&mut peer, 5201).await;
                let Req::ComposeOperation { id, .. } = fake_compose_read(&mut peer).await else {
                    panic!("query")
                };
                fake_compose_reply(
                    &mut peer,
                    Resp::ComposeRejected {
                        id,
                        code: RejectCode::UnknownOperation,
                        rule: None,
                    },
                )
                .await;
                let Req::ComposeBackupBegin {
                    id, operation_id, ..
                } = fake_compose_read(&mut peer).await
                else {
                    panic!("begin")
                };
                fake_compose_reply(
                    &mut peer,
                    Resp::ComposeOperationState {
                        id,
                        operation: OperationView {
                            operation_id: operation_id.clone(),
                            kind: OperationKind::Backup,
                            project_id: "calm-life".into(),
                            revision: 1,
                            state: OperationState::Succeeded,
                            stage: OperationStage::Settled,
                            failure: None,
                            restored_previous: false,
                        },
                    },
                )
                .await;
                let Req::ComposeBackupMeta { id, .. } = fake_compose_read(&mut peer).await else {
                    panic!("meta")
                };
                fake_compose_reply(
                    &mut peer,
                    Resp::ComposeBackupReady {
                        id,
                        operation_id: operation_id.clone(),
                        project_id: "calm-life".into(),
                        revision: 1,
                        total_bytes: b1.len() as u64,
                        chunks: chunk_count(b1.len() as u64).unwrap(),
                        digest: d1,
                    },
                )
                .await;
                let mut tampered = b1.clone();
                let last = tampered.len() - 1;
                tampered[last] ^= 0xff;
                let n = chunk_count(b1.len() as u64).unwrap();
                for idx in 0..n {
                    let Req::ComposeBackupChunk { id, index, .. } =
                        fake_compose_read(&mut peer).await
                    else {
                        panic!("chunk")
                    };
                    assert_eq!(index, idx);
                    let start = idx as usize * BACKUP_CHUNK_BYTES;
                    let end = (start + BACKUP_CHUNK_BYTES).min(tampered.len());
                    fake_compose_reply(
                        &mut peer,
                        Resp::ComposeBackupData {
                            id,
                            operation_id: operation_id.clone(),
                            index,
                            data: encode_base64(&tampered[start..end]),
                        },
                    )
                    .await;
                }
                if let Ok(len) = tokio::io::AsyncReadExt::read_u32(&mut peer).await {
                    let mut buf = vec![0; len as usize];
                    let _ = tokio::io::AsyncReadExt::read_exact(&mut peer, &mut buf).await;
                }
            })
        })
        .await;
        let r_digest = poll_compose_backup_terminal(
            &a,
            &json!({"vm_id": vm, "project_id": "calm-life", "request_id": key(5210)}),
        )
        .await;
        assert_eq!(r_digest["state"], "failed", "{r_digest}");
        assert_eq!(r_digest["failure"], "verification_or_flush_failed");
        assert!(r_digest.get("backup_id").is_none());
        t1.abort();

        // Case 2: Oversize cap in metadata -> fails immediately before staging.
        let d2 = good_digest.clone();
        let t2 = attach_peer(&h, vm_id, generation, 5202, move |mut peer| {
            tokio::spawn(async move {
                fake_compose_hello(&mut peer, 5202).await;
                let Req::ComposeOperation { id, .. } = fake_compose_read(&mut peer).await else {
                    panic!()
                };
                fake_compose_reply(
                    &mut peer,
                    Resp::ComposeRejected {
                        id,
                        code: RejectCode::UnknownOperation,
                        rule: None,
                    },
                )
                .await;
                let Req::ComposeBackupBegin {
                    id, operation_id, ..
                } = fake_compose_read(&mut peer).await
                else {
                    panic!()
                };
                fake_compose_reply(
                    &mut peer,
                    Resp::ComposeOperationState {
                        id,
                        operation: OperationView {
                            operation_id: operation_id.clone(),
                            kind: OperationKind::Backup,
                            project_id: "calm-life".into(),
                            revision: 1,
                            state: OperationState::Succeeded,
                            stage: OperationStage::Settled,
                            failure: None,
                            restored_previous: false,
                        },
                    },
                )
                .await;
                let Req::ComposeBackupMeta { id, .. } = fake_compose_read(&mut peer).await else {
                    panic!()
                };
                let oversized = max_bytes + 4096;
                fake_compose_reply(
                    &mut peer,
                    Resp::ComposeBackupReady {
                        id,
                        operation_id,
                        project_id: "calm-life".into(),
                        revision: 1,
                        total_bytes: oversized,
                        chunks: chunk_count(oversized).unwrap(),
                        digest: d2,
                    },
                )
                .await;
            })
        })
        .await;
        let r_cap = poll_compose_backup_terminal(
            &a,
            &json!({"vm_id": vm, "project_id": "calm-life", "request_id": key(5211)}),
        )
        .await;
        assert_eq!(r_cap["state"], "failed", "{r_cap}");
        assert_eq!(r_cap["failure"], "invalid_backup_metadata");
        t2.abort();

        // Case 3: Out-of-order chunk sequence -> fails & cleans staging.
        let b3 = good_bytes.clone();
        let d3 = good_digest.clone();
        let t3 = attach_peer(&h, vm_id, generation, 5203, move |mut peer| {
            tokio::spawn(async move {
                fake_compose_hello(&mut peer, 5203).await;
                let Req::ComposeOperation { id, .. } = fake_compose_read(&mut peer).await else {
                    panic!()
                };
                fake_compose_reply(
                    &mut peer,
                    Resp::ComposeRejected {
                        id,
                        code: RejectCode::UnknownOperation,
                        rule: None,
                    },
                )
                .await;
                let Req::ComposeBackupBegin {
                    id, operation_id, ..
                } = fake_compose_read(&mut peer).await
                else {
                    panic!()
                };
                fake_compose_reply(
                    &mut peer,
                    Resp::ComposeOperationState {
                        id,
                        operation: OperationView {
                            operation_id: operation_id.clone(),
                            kind: OperationKind::Backup,
                            project_id: "calm-life".into(),
                            revision: 1,
                            state: OperationState::Succeeded,
                            stage: OperationStage::Settled,
                            failure: None,
                            restored_previous: false,
                        },
                    },
                )
                .await;
                let Req::ComposeBackupMeta { id, .. } = fake_compose_read(&mut peer).await else {
                    panic!()
                };
                fake_compose_reply(
                    &mut peer,
                    Resp::ComposeBackupReady {
                        id,
                        operation_id: operation_id.clone(),
                        project_id: "calm-life".into(),
                        revision: 1,
                        total_bytes: b3.len() as u64,
                        chunks: chunk_count(b3.len() as u64).unwrap(),
                        digest: d3,
                    },
                )
                .await;
                let Req::ComposeBackupChunk { id, .. } = fake_compose_read(&mut peer).await else {
                    panic!()
                };
                // Reply with wrong chunk index (1 instead of 0).
                fake_compose_reply(
                    &mut peer,
                    Resp::ComposeBackupData {
                        id,
                        operation_id,
                        index: 1,
                        data: encode_base64(&b3[..b3.len().min(BACKUP_CHUNK_BYTES)]),
                    },
                )
                .await;
            })
        })
        .await;
        let r_seq = poll_compose_backup_terminal(
            &a,
            &json!({"vm_id": vm, "project_id": "calm-life", "request_id": key(5212)}),
        )
        .await;
        assert_eq!(r_seq["state"], "failed", "{r_seq}");
        assert_eq!(r_seq["failure"], "chunk_sequence_mismatch");
        t3.abort();

        // Case 4: Idle deadline expires on stalled chunk transfer -> interrupted cleanly.
        *h.supervisor().compose_backup.timeouts.lock().unwrap() = compose_backup::BackupTimeouts {
            idle: Duration::from_millis(80),
            absolute: Duration::from_secs(5),
            poll: Duration::from_millis(20),
            guest_deadline_s: 10,
        };
        let b4 = good_bytes.clone();
        let d4 = good_digest.clone();
        let t4 = attach_peer(&h, vm_id, generation, 5204, move |mut peer| {
            tokio::spawn(async move {
                fake_compose_hello(&mut peer, 5204).await;
                let Req::ComposeOperation { id, .. } = fake_compose_read(&mut peer).await else {
                    panic!()
                };
                fake_compose_reply(
                    &mut peer,
                    Resp::ComposeRejected {
                        id,
                        code: RejectCode::UnknownOperation,
                        rule: None,
                    },
                )
                .await;
                let Req::ComposeBackupBegin {
                    id, operation_id, ..
                } = fake_compose_read(&mut peer).await
                else {
                    panic!()
                };
                fake_compose_reply(
                    &mut peer,
                    Resp::ComposeOperationState {
                        id,
                        operation: OperationView {
                            operation_id: operation_id.clone(),
                            kind: OperationKind::Backup,
                            project_id: "calm-life".into(),
                            revision: 1,
                            state: OperationState::Succeeded,
                            stage: OperationStage::Settled,
                            failure: None,
                            restored_previous: false,
                        },
                    },
                )
                .await;
                let Req::ComposeBackupMeta { id, .. } = fake_compose_read(&mut peer).await else {
                    panic!()
                };
                fake_compose_reply(
                    &mut peer,
                    Resp::ComposeBackupReady {
                        id,
                        operation_id,
                        project_id: "calm-life".into(),
                        revision: 1,
                        total_bytes: b4.len() as u64,
                        chunks: chunk_count(b4.len() as u64).unwrap(),
                        digest: d4,
                    },
                )
                .await;
                // Read chunk request and stall without replying so idle deadline trips.
                let _ = fake_compose_read(&mut peer).await;
                tokio::time::sleep(Duration::from_secs(5)).await;
            })
        })
        .await;
        let r_idle = poll_compose_backup_terminal(
            &a,
            &json!({"vm_id": vm, "project_id": "calm-life", "request_id": key(5213)}),
        )
        .await;
        assert_eq!(r_idle["state"], "interrupted", "{r_idle}");
        assert_eq!(r_idle["failure"], "idle_deadline_exceeded");
        t4.abort();

        // Reset normal timeouts.
        *h.supervisor().compose_backup.timeouts.lock().unwrap() =
            compose_backup::BackupTimeouts::default();

        // Case 5: Uncertain partial (post-publish failure) retains
        // host_staging_charge in disk_bytes until attributed reconciliation
        // proves cleanup and releases the charge.
        let flush_root = backup_fixture_root();
        let mut native =
            compose_backup::ManagedBackupDir::new(flush_root.path(), h.temp.path(), None).unwrap();
        native.fail_published_flush();
        let native = Arc::new(native);
        *h.supervisor().compose_backup.custody.lock().unwrap() = Some(native.clone());
        let disk_before_partial = store
            .transact(|tx| crate::store::vm::disk_bytes(tx))
            .unwrap()
            .unwrap();
        let b5 = good_bytes.clone();
        let d5 = good_digest.clone();
        let t5 = attach_peer(&h, vm_id, generation, 5205, move |mut peer| {
            tokio::spawn(async move {
                fake_compose_hello(&mut peer, 5205).await;
                let Req::ComposeOperation { id, .. } = fake_compose_read(&mut peer).await else {
                    panic!()
                };
                fake_compose_reply(
                    &mut peer,
                    Resp::ComposeRejected {
                        id,
                        code: RejectCode::UnknownOperation,
                        rule: None,
                    },
                )
                .await;
                let Req::ComposeBackupBegin {
                    id, operation_id, ..
                } = fake_compose_read(&mut peer).await
                else {
                    panic!()
                };
                fake_compose_reply(
                    &mut peer,
                    Resp::ComposeOperationState {
                        id,
                        operation: OperationView {
                            operation_id: operation_id.clone(),
                            kind: OperationKind::Backup,
                            project_id: "calm-life".into(),
                            revision: 1,
                            state: OperationState::Succeeded,
                            stage: OperationStage::Settled,
                            failure: None,
                            restored_previous: false,
                        },
                    },
                )
                .await;
                let Req::ComposeBackupMeta { id, .. } = fake_compose_read(&mut peer).await else {
                    panic!()
                };
                fake_compose_reply(
                    &mut peer,
                    Resp::ComposeBackupReady {
                        id,
                        operation_id: operation_id.clone(),
                        project_id: "calm-life".into(),
                        revision: 1,
                        total_bytes: b5.len() as u64,
                        chunks: chunk_count(b5.len() as u64).unwrap(),
                        digest: d5,
                    },
                )
                .await;
                let n = chunk_count(b5.len() as u64).unwrap();
                for idx in 0..n {
                    let Req::ComposeBackupChunk { id, index, .. } =
                        fake_compose_read(&mut peer).await
                    else {
                        panic!()
                    };
                    let start = idx as usize * BACKUP_CHUNK_BYTES;
                    let end = (start + BACKUP_CHUNK_BYTES).min(b5.len());
                    fake_compose_reply(
                        &mut peer,
                        Resp::ComposeBackupData {
                            id,
                            operation_id: operation_id.clone(),
                            index,
                            data: encode_base64(&b5[start..end]),
                        },
                    )
                    .await;
                }
            })
        })
        .await;
        let args5 = json!({"vm_id": vm, "project_id": "calm-life", "request_id": key(5214)});
        let accepted = call(&a, "compose_backup", args5.clone()).await;
        assert_eq!(accepted["state"], "pending");
        let replay_key = RequestId::parse(&key(5214)).unwrap().key;
        until(|| {
            store
                .transact(|tx| crate::store::vm::compose::backup_replay(tx, "alpha", replay_key))
                .unwrap()
                .is_some_and(|intent| intent.status != compose_backup::BackupOpStatus::Pending)
        })
        .await;
        // Uncertain partial retains host_staging_charge in disk_bytes!
        let disk_with_partial = store
            .transact(|tx| crate::store::vm::disk_bytes(tx))
            .unwrap()
            .unwrap();
        assert_eq!(disk_with_partial - disk_before_partial, max_bytes);
        let backups = store
            .transact(|tx| crate::store::vm::compose::list_project_backups(tx, &target))
            .unwrap();
        let partial = backups
            .iter()
            .find(|b| b.state == compose_backup::BackupRecordState::PartialUncertain)
            .expect("partial_uncertain record");
        assert_eq!(partial.host_staging_charge, max_bytes);

        assert!(native.settled_path(partial.id).unwrap().is_file());
        // Missing names in a different root cannot certify cleanup.
        let other = backup_fixture_root();
        let empty = Arc::new(
            compose_backup::ManagedBackupDir::new(other.path(), h.temp.path(), None).unwrap(),
        );
        *h.supervisor().compose_backup.custody.lock().unwrap() = Some(empty);
        let r_partial = call(&a, "compose_backup", args5).await;
        assert_eq!(r_partial["state"], "failed", "{r_partial}");
        assert!(r_partial.get("backup_id").is_none());
        assert_eq!(
            h.supervisor()
                .compose_backup
                .reconcile_partials(store)
                .unwrap(),
            0
        );
        assert_eq!(
            store
                .transact(|tx| crate::store::vm::disk_bytes(tx))
                .unwrap()
                .unwrap()
                - disk_before_partial,
            max_bytes
        );
        // Attributed reconciliation releases the charge only after confirmed removal.
        *h.supervisor().compose_backup.custody.lock().unwrap() = Some(native);
        assert_eq!(
            h.supervisor()
                .compose_backup
                .reconcile_partials(store)
                .unwrap(),
            1
        );
        let disk_reconciled = store
            .transact(|tx| crate::store::vm::disk_bytes(tx))
            .unwrap()
            .unwrap();
        assert_eq!(disk_reconciled, disk_before_partial);
        t5.abort();

        // Case 6: Query-before-submit recovery on same boot: mark sent=1 in DB
        // before driving, and prove ComposeBackupBegin is NOT re-sent!
        let (_tmp_good, good_managed) = {
            let d = backup_fixture_root();
            let m = Arc::new(
                compose_backup::ManagedBackupDir::new(d.path(), h.temp.path(), None).unwrap(),
            );
            (d, m)
        };
        *h.supervisor().compose_backup.custody.lock().unwrap() = Some(good_managed);
        let k6 = RequestId::parse(&key(5215)).unwrap();
        let now = h.supervisor().controller.compose_now().unwrap();
        let intent6 = store
            .transact(|tx| {
                let intent = crate::store::vm::compose::admit_backup(
                    tx,
                    "alpha",
                    &target,
                    k6,
                    generation,
                    5206,
                    now,
                    |_, _| Ok(Ok(())),
                )?
                .unwrap();
                crate::store::vm::compose::mark_backup_sent(tx, intent.id)?;
                Ok(intent)
            })
            .unwrap();
        let b6 = good_bytes.clone();
        let d6 = good_digest.clone();
        let op6_hex = hex::encode(intent6.id);
        let t6 = attach_peer(&h, vm_id, generation, 5206, move |mut peer| {
            tokio::spawn(async move {
                fake_compose_hello(&mut peer, 5206).await;
                // Must query ComposeOperation (NEVER ComposeBackupBegin) because sent=1.
                let Req::ComposeOperation { id, operation_id } = fake_compose_read(&mut peer).await
                else {
                    panic!("must query operation first")
                };
                assert_eq!(operation_id, op6_hex);
                fake_compose_reply(
                    &mut peer,
                    Resp::ComposeOperationState {
                        id,
                        operation: OperationView {
                            operation_id: operation_id.clone(),
                            kind: OperationKind::Backup,
                            project_id: "calm-life".into(),
                            revision: 1,
                            state: OperationState::Succeeded,
                            stage: OperationStage::Settled,
                            failure: None,
                            restored_previous: false,
                        },
                    },
                )
                .await;
                let Req::ComposeBackupMeta { id, .. } = fake_compose_read(&mut peer).await else {
                    panic!()
                };
                fake_compose_reply(
                    &mut peer,
                    Resp::ComposeBackupReady {
                        id,
                        operation_id: operation_id.clone(),
                        project_id: "calm-life".into(),
                        revision: 1,
                        total_bytes: b6.len() as u64,
                        chunks: chunk_count(b6.len() as u64).unwrap(),
                        digest: d6,
                    },
                )
                .await;
                let n = chunk_count(b6.len() as u64).unwrap();
                for idx in 0..n {
                    let Req::ComposeBackupChunk { id, index, .. } =
                        fake_compose_read(&mut peer).await
                    else {
                        panic!()
                    };
                    let start = idx as usize * BACKUP_CHUNK_BYTES;
                    let end = (start + BACKUP_CHUNK_BYTES).min(b6.len());
                    fake_compose_reply(
                        &mut peer,
                        Resp::ComposeBackupData {
                            id,
                            operation_id: operation_id.clone(),
                            index,
                            data: encode_base64(&b6[start..end]),
                        },
                    )
                    .await;
                }
                let Req::ComposeBackupRelease { id, .. } = fake_compose_read(&mut peer).await
                else {
                    panic!()
                };
                fake_compose_reply(
                    &mut peer,
                    Resp::ComposeBackupReleased {
                        id,
                        operation_id,
                        removed: true,
                    },
                )
                .await;
            })
        })
        .await;
        // Replaying the same request_id drives the already-sent backup to completion!
        let r_recovered = poll_compose_backup_terminal(
            &a,
            &json!({"vm_id": vm, "project_id": "calm-life", "request_id": key(5215)}),
        )
        .await;
        assert_eq!(r_recovered["state"], "succeeded", "{r_recovered}");
        assert_eq!(r_recovered["sha256"], good_digest);
        t6.abort();

        // Case 7: Exact chunk length (short chunk, overlong final chunk) and
        // non-canonical Base64 are rejected and staging is cleaned (kills F).
        for (boot_n, req_n, mode, expected_failure) in [
            (5207u64, 5216u64, "short", "chunk_size_mismatch"),
            (5208u64, 5217u64, "overlong", "chunk_size_mismatch"),
            (5209u64, 5218u64, "bad_b64", "chunk_decode_failed"),
        ] {
            let bc = good_bytes.clone();
            let dc = good_digest.clone();
            let tc = attach_peer(&h, vm_id, generation, boot_n, move |mut peer| {
                tokio::spawn(async move {
                    fake_compose_hello(&mut peer, boot_n).await;
                    let Req::ComposeOperation { id, .. } = fake_compose_read(&mut peer).await
                    else {
                        panic!()
                    };
                    fake_compose_reply(
                        &mut peer,
                        Resp::ComposeRejected {
                            id,
                            code: RejectCode::UnknownOperation,
                            rule: None,
                        },
                    )
                    .await;
                    let Req::ComposeBackupBegin {
                        id, operation_id, ..
                    } = fake_compose_read(&mut peer).await
                    else {
                        panic!()
                    };
                    fake_compose_reply(
                        &mut peer,
                        Resp::ComposeOperationState {
                            id,
                            operation: OperationView {
                                operation_id: operation_id.clone(),
                                kind: OperationKind::Backup,
                                project_id: "calm-life".into(),
                                revision: 1,
                                state: OperationState::Succeeded,
                                stage: OperationStage::Settled,
                                failure: None,
                                restored_previous: false,
                            },
                        },
                    )
                    .await;
                    let Req::ComposeBackupMeta { id, .. } = fake_compose_read(&mut peer).await
                    else {
                        panic!()
                    };
                    let n = chunk_count(bc.len() as u64).unwrap();
                    fake_compose_reply(
                        &mut peer,
                        Resp::ComposeBackupReady {
                            id,
                            operation_id: operation_id.clone(),
                            project_id: "calm-life".into(),
                            revision: 1,
                            total_bytes: bc.len() as u64,
                            chunks: n,
                            digest: dc,
                        },
                    )
                    .await;
                    for idx in 0..n {
                        let Req::ComposeBackupChunk { id, index, .. } =
                            fake_compose_read(&mut peer).await
                        else {
                            panic!()
                        };
                        assert_eq!(index, idx);
                        let start = idx as usize * BACKUP_CHUNK_BYTES;
                        let end = (start + BACKUP_CHUNK_BYTES).min(bc.len());
                        let data = match mode {
                            "short" if idx == 0 => encode_base64(&bc[start..end - 16]),
                            "overlong" if idx + 1 == n => {
                                let mut extra = bc[start..end].to_vec();
                                extra.push(0);
                                encode_base64(&extra)
                            }
                            "bad_b64" if idx == 0 => "AB==".to_owned(),
                            _ => encode_base64(&bc[start..end]),
                        };
                        fake_compose_reply(
                            &mut peer,
                            Resp::ComposeBackupData {
                                id,
                                operation_id: operation_id.clone(),
                                index,
                                data,
                            },
                        )
                        .await;
                        if (mode == "short" || mode == "bad_b64") && idx == 0 {
                            break;
                        }
                    }
                    if let Ok(len) = tokio::io::AsyncReadExt::read_u32(&mut peer).await {
                        let mut buf = vec![0; len as usize];
                        let _ = tokio::io::AsyncReadExt::read_exact(&mut peer, &mut buf).await;
                    }
                })
            })
            .await;
            let reply = poll_compose_backup_terminal(
                &a,
                &json!({"vm_id": vm, "project_id": "calm-life", "request_id": key(req_n)}),
            )
            .await;
            assert_eq!(reply["state"], "failed", "{mode}: {reply}");
            assert_eq!(reply["failure"], expected_failure, "{mode}: {reply}");
            tc.abort();
        }

        // Case 8: sent=1 + UnknownOperation -> lost_operation_unknown (NO second Begin).
        let k8 = RequestId::parse(&key(5219)).unwrap();
        let now8 = h.supervisor().controller.compose_now().unwrap();
        let staged_owner = store
            .transact(|tx| {
                let owner = crate::store::vm::compose::admit_backup(
                    tx,
                    "alpha",
                    &target,
                    RequestId::parse(&format!("{:016x}{:016x}", now8 - 1, 5240)).unwrap(),
                    generation,
                    5210,
                    now8 - 1,
                    |_, _| Ok(Ok(())),
                )?
                .unwrap();
                crate::store::vm::compose::settle_backup_failed(
                    tx,
                    &owner,
                    "lost_release",
                    true,
                    true,
                    now8 - 1,
                )?;
                Ok(hex::encode(owner.id))
            })
            .unwrap();
        store
            .transact(|tx| {
                let intent = crate::store::vm::compose::admit_backup(
                    tx,
                    "alpha",
                    &target,
                    k8,
                    generation,
                    5210,
                    now8,
                    |_, _| Ok(Ok(())),
                )?
                .unwrap();
                crate::store::vm::compose::mark_backup_sent(tx, intent.id)
            })
            .unwrap();
        let t8 = attach_peer(&h, vm_id, generation, 5210, move |mut peer| {
            tokio::spawn(async move {
                fake_compose_hello(&mut peer, 5210).await;
                let Req::ComposeOperation { id, .. } = fake_compose_read(&mut peer).await else {
                    panic!("expected query")
                };
                fake_compose_reply(
                    &mut peer,
                    Resp::ComposeRejected {
                        id,
                        code: RejectCode::UnknownOperation,
                        rule: None,
                    },
                )
                .await;
                assert!(
                    tokio::time::timeout(
                        Duration::from_millis(80),
                        tokio::io::AsyncReadExt::read_u32(&mut peer)
                    )
                    .await
                    .is_err()
                );
            })
        })
        .await;
        let r8 = poll_compose_backup_terminal(
            &a,
            &json!({"vm_id": vm, "project_id": "calm-life", "request_id": key(5219)}),
        )
        .await;
        assert_eq!(r8["state"], "interrupted", "{r8}");
        assert_eq!(r8["failure"], "lost_operation_unknown");
        t8.abort();

        // Case 9: Stale guest staging (BackupStagingOccupied) -> host releases
        // prior terminal op on the same boot, retries Begin once, polls
        // Running -> Ambiguous -> ComposeReconcile -> Succeeded!
        *h.supervisor().compose_backup.timeouts.lock().unwrap() = compose_backup::BackupTimeouts {
            idle: Duration::from_secs(5),
            absolute: Duration::from_secs(15),
            poll: Duration::from_millis(15),
            guest_deadline_s: 30,
        };
        let b9 = good_bytes.clone();
        let d9 = good_digest.clone();
        let seen_stale_release = Arc::new(AtomicU64::new(0));
        let peer_stale = seen_stale_release.clone();
        let seen_reconcile = Arc::new(AtomicU64::new(0));
        let peer_reconcile = seen_reconcile.clone();
        let t9 = attach_peer(&h, vm_id, generation, 5210, move |mut peer| {
            tokio::spawn(async move {
                fake_compose_hello(&mut peer, 5210).await;
                let Req::ComposeOperation { id, .. } = fake_compose_read(&mut peer).await else {
                    panic!()
                };
                fake_compose_reply(
                    &mut peer,
                    Resp::ComposeRejected {
                        id,
                        code: RejectCode::UnknownOperation,
                        rule: None,
                    },
                )
                .await;
                let Req::ComposeBackupBegin {
                    id, operation_id, ..
                } = fake_compose_read(&mut peer).await
                else {
                    panic!()
                };
                fake_compose_reply(
                    &mut peer,
                    Resp::ComposeBackupOccupied {
                        id,
                        operation_id: staged_owner.clone(),
                    },
                )
                .await;
                let Req::ComposeBackupRelease {
                    id,
                    operation_id: stale_op,
                } = fake_compose_read(&mut peer).await
                else {
                    panic!("expected release of stale op")
                };
                assert_eq!(stale_op, staged_owner);
                peer_stale.fetch_add(1, Ordering::SeqCst);
                fake_compose_reply(
                    &mut peer,
                    Resp::ComposeBackupReleased {
                        id,
                        operation_id: stale_op,
                        removed: true,
                    },
                )
                .await;
                let Req::ComposeBackupBegin { id, .. } = fake_compose_read(&mut peer).await else {
                    panic!("expected retry Begin")
                };
                fake_compose_reply(
                    &mut peer,
                    Resp::ComposeOperationState {
                        id,
                        operation: OperationView {
                            operation_id: operation_id.clone(),
                            kind: OperationKind::Backup,
                            project_id: "calm-life".into(),
                            revision: 1,
                            state: OperationState::Running,
                            stage: OperationStage::BackingUp,
                            failure: None,
                            restored_previous: false,
                        },
                    },
                )
                .await;
                let Req::ComposeOperation { id, .. } = fake_compose_read(&mut peer).await else {
                    panic!("expected poll")
                };
                fake_compose_reply(
                    &mut peer,
                    Resp::ComposeOperationState {
                        id,
                        operation: OperationView {
                            operation_id: operation_id.clone(),
                            kind: OperationKind::Backup,
                            project_id: "calm-life".into(),
                            revision: 1,
                            state: OperationState::Ambiguous,
                            stage: OperationStage::BackingUp,
                            failure: None,
                            restored_previous: false,
                        },
                    },
                )
                .await;
                let Req::ComposeReconcile { id, .. } = fake_compose_read(&mut peer).await else {
                    panic!("expected reconcile")
                };
                peer_reconcile.fetch_add(1, Ordering::SeqCst);
                fake_compose_reply(
                    &mut peer,
                    Resp::ComposeOperationState {
                        id,
                        operation: OperationView {
                            operation_id: operation_id.clone(),
                            kind: OperationKind::Backup,
                            project_id: "calm-life".into(),
                            revision: 1,
                            state: OperationState::Succeeded,
                            stage: OperationStage::Settled,
                            failure: None,
                            restored_previous: false,
                        },
                    },
                )
                .await;
                let Req::ComposeBackupMeta { id, .. } = fake_compose_read(&mut peer).await else {
                    panic!()
                };
                let n = chunk_count(b9.len() as u64).unwrap();
                fake_compose_reply(
                    &mut peer,
                    Resp::ComposeBackupReady {
                        id,
                        operation_id: operation_id.clone(),
                        project_id: "calm-life".into(),
                        revision: 1,
                        total_bytes: b9.len() as u64,
                        chunks: n,
                        digest: d9,
                    },
                )
                .await;
                for idx in 0..n {
                    let Req::ComposeBackupChunk { id, index, .. } =
                        fake_compose_read(&mut peer).await
                    else {
                        panic!()
                    };
                    let start = idx as usize * BACKUP_CHUNK_BYTES;
                    let end = (start + BACKUP_CHUNK_BYTES).min(b9.len());
                    fake_compose_reply(
                        &mut peer,
                        Resp::ComposeBackupData {
                            id,
                            operation_id: operation_id.clone(),
                            index,
                            data: encode_base64(&b9[start..end]),
                        },
                    )
                    .await;
                }
                let Req::ComposeBackupRelease { id, .. } = fake_compose_read(&mut peer).await
                else {
                    panic!()
                };
                fake_compose_reply(
                    &mut peer,
                    Resp::ComposeBackupReleased {
                        id,
                        operation_id,
                        removed: true,
                    },
                )
                .await;
            })
        })
        .await;
        let r9 = poll_compose_backup_terminal(
            &a,
            &json!({"vm_id": vm, "project_id": "calm-life", "request_id": key(5220)}),
        )
        .await;
        assert_eq!(r9["state"], "succeeded", "{r9}");
        assert_eq!(seen_stale_release.load(Ordering::SeqCst), 1);
        assert_eq!(seen_reconcile.load(Ordering::SeqCst), 1);
        t9.abort();

        drop(a);
        h.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn compose_backup_cancel_mid_transfer_and_shared_channel_liveness() {
        use arx_guest::compose::{
            ComposeRequest as Req, ComposeResponse as Resp, OperationKind, OperationStage,
            OperationState, OperationView, RejectCode, encode_base64,
        };
        use arx_guest::compose_backup::{BACKUP_CHUNK_BYTES, chunk_count};

        // 1. Mid-transfer shutdown cancels cleanly, cleans staging, and attempts
        // guest release (no backup_id returned).
        let mut h = Harness::start_with_web(false, false, true).await;
        let a = h.client(0).await;
        let max_bytes = 256 * 1024;
        let (vm, vm_id, generation, _target, _backup_dir, managed) =
            setup_compose_backup_project(&h, &a, 5400, max_bytes).await;
        let fixture_src = tempfile::tempdir().unwrap();
        let (good_bytes, good_digest) =
            make_wal_sqlite_bytes(fixture_src.path(), "cancel", &[("row", 1)], 24 * 1024);
        let store = h.supervisor().controller.compose_store();

        // Trigger shutdown deterministically right after chunk 0 writes to real
        // host staging so the wire channel stays open for best-effort release.
        struct CancelAfterFirstChunkCustody {
            inner: Arc<compose_backup::ManagedBackupDir>,
            cancel: crate::service::Shutdown,
        }
        struct CancelAfterFirstChunkWriter {
            inner: Box<dyn compose_backup::StagingWriter>,
            cancel: crate::service::Shutdown,
            fired: bool,
        }
        impl compose_backup::StagingWriter for CancelAfterFirstChunkWriter {
            fn witness(&self) -> compose_backup::HostFileWitness {
                self.inner.witness()
            }
            fn write_chunk(&mut self, bytes: &[u8]) -> Result<()> {
                self.inner.write_chunk(bytes)?;
                if !self.fired {
                    self.fired = true;
                    self.cancel.cancel();
                }
                Ok(())
            }
            fn finish_and_publish(
                self: Box<Self>,
                backup_id: Id,
                expected_size: u64,
                expected_sha256: [u8; 32],
            ) -> std::result::Result<compose_backup::PublishedBackup, compose_backup::PublishError>
            {
                self.inner
                    .finish_and_publish(backup_id, expected_size, expected_sha256)
            }
            fn abort_and_cleanup(self: Box<Self>) -> Result<bool> {
                self.inner.abort_and_cleanup()
            }
        }
        impl compose_backup::BackupCustody for CancelAfterFirstChunkCustody {
            fn begin_staging(
                &self,
                operation_id: Id,
            ) -> Result<Box<dyn compose_backup::StagingWriter>> {
                Ok(Box::new(CancelAfterFirstChunkWriter {
                    inner: self.inner.begin_staging(operation_id)?,
                    cancel: self.cancel.clone(),
                    fired: false,
                }))
            }
            fn cleanup_partial_staging(
                &self,
                operation_id: Id,
                backup_id: Id,
                expected: Option<compose_backup::HostFileWitness>,
            ) -> Result<bool> {
                self.inner
                    .cleanup_partial_staging(operation_id, backup_id, expected)
            }
        }
        *h.supervisor().compose_backup.custody.lock().unwrap() =
            Some(Arc::new(CancelAfterFirstChunkCustody {
                inner: managed,
                cancel: h.supervisor().cancel.clone(),
            }));

        let (host_io, mut peer) = tokio::io::duplex(256 * 1024);
        let release_seen = Arc::new(AtomicU64::new(0));
        let peer_release = release_seen.clone();
        let bc = good_bytes.clone();
        let dc = good_digest.clone();
        let peer_task = tokio::spawn(async move {
            fake_compose_hello(&mut peer, 5401).await;
            let Req::ComposeOperation { id, .. } = fake_compose_read(&mut peer).await else {
                panic!()
            };
            fake_compose_reply(
                &mut peer,
                Resp::ComposeRejected {
                    id,
                    code: RejectCode::UnknownOperation,
                    rule: None,
                },
            )
            .await;
            let Req::ComposeBackupBegin {
                id, operation_id, ..
            } = fake_compose_read(&mut peer).await
            else {
                panic!()
            };
            fake_compose_reply(
                &mut peer,
                Resp::ComposeOperationState {
                    id,
                    operation: OperationView {
                        operation_id: operation_id.clone(),
                        kind: OperationKind::Backup,
                        project_id: "calm-life".into(),
                        revision: 1,
                        state: OperationState::Succeeded,
                        stage: OperationStage::Settled,
                        failure: None,
                        restored_previous: false,
                    },
                },
            )
            .await;
            let Req::ComposeBackupMeta { id, .. } = fake_compose_read(&mut peer).await else {
                panic!()
            };
            fake_compose_reply(
                &mut peer,
                Resp::ComposeBackupReady {
                    id,
                    operation_id: operation_id.clone(),
                    project_id: "calm-life".into(),
                    revision: 1,
                    total_bytes: bc.len() as u64,
                    chunks: chunk_count(bc.len() as u64).unwrap(),
                    digest: dc,
                },
            )
            .await;
            let Req::ComposeBackupChunk { id, index, .. } = fake_compose_read(&mut peer).await
            else {
                panic!()
            };
            assert_eq!(index, 0);
            fake_compose_reply(
                &mut peer,
                Resp::ComposeBackupData {
                    id,
                    operation_id: operation_id.clone(),
                    index: 0,
                    data: encode_base64(&bc[..BACKUP_CHUNK_BYTES]),
                },
            )
            .await;
            let Req::ComposeBackupRelease { id, operation_id } = fake_compose_read(&mut peer).await
            else {
                panic!("expected guest release after cancel")
            };
            peer_release.fetch_add(1, Ordering::SeqCst);
            fake_compose_reply(
                &mut peer,
                Resp::ComposeBackupReleased {
                    id,
                    operation_id,
                    removed: true,
                },
            )
            .await;
        });
        let channel =
            crate::vm::guest::ComposeChannel::negotiate(host_io, 5401, &h.supervisor().cancel)
                .await
                .unwrap();
        store
            .transact(|tx| crate::store::vm::compose::register_boot(tx, vm_id, generation, 5401))
            .unwrap();
        h.supervisor()
            .compose_backup
            .attach(
                vm_id,
                compose_backup::BackupSession::new(generation, channel),
            )
            .unwrap();

        let req_cancel = json!({"vm_id": vm, "project_id": "calm-life", "request_id": key(5410)});
        let r_cancel = poll_compose_backup_terminal(&a, &req_cancel).await;
        assert_eq!(r_cancel["state"], "interrupted", "{r_cancel}");
        assert_eq!(r_cancel["failure"], "cancelled");
        assert!(r_cancel.get("backup_id").is_none());
        until(|| release_seen.load(Ordering::SeqCst) == 1).await;
        drop(a);
        peer_task.abort();
        h.shutdown().await.unwrap();

        // 2. Shared ComposeChannel stays open during an in-flight exchange, and
        // a backup hitting its absolute deadline (< MIN_EXCHANGE_BUDGET) settles
        // deadline_exceeded without closing the shared channel so compose_stop on
        // another project still works (M1).
        let mut h2 = Harness::start_with_web(false, false, true).await;
        let a2 = h2.client(0).await;
        let (vm2, vm2_id, gen2, _target2, _bdir2, _m2) =
            setup_compose_backup_project(&h2, &a2, 5500, max_bytes).await;
        let store2 = h2.supervisor().controller.compose_store();
        let other_rel = [0x62u8; 16];
        let other_enroll: compose::ReleaseInput = serde_json::from_value(json!({
            "vm_id": vm2,
            "release_id": hex::encode(other_rel),
            "manifest": {
                "schema": arx_guest::compose::MANIFEST_SCHEMA,
                "project_id": "other-app",
                "volumes": [],
                "secrets": [],
                "services": [{
                    "name": "web",
                    "image": format!("example.invalid/other@sha256:{}", "d".repeat(64)),
                    "platform": arx_guest::compose::PLATFORM,
                    "uid": 10002,
                    "gid": 10002,
                    "read_only_rootfs": true,
                    "http_port": 8080,
                    "health_path": "/health",
                    "environment": [],
                    "mounts": [],
                    "secrets": [],
                    "limits": {"cpu_millis": 500, "memory_mib": 512, "pids": 64}
                }]
            }
        }))
        .unwrap();
        crate::store::vm::compose::enroll(store2, &[other_enroll.enroll().unwrap()]).unwrap();
        store2
            .transact(|tx| {
                tx.execute(
                    "UPDATE vm_compose_projects SET desired='started', last_revision=1, committed=1,
                     committed_release=?3, rendered_revision=1, rendered_release=?3, settled_generation=?4
                     WHERE vm=?1 AND project='other-app'",
                    rusqlite::params![vm2_id, "other-app", other_rel, gen2],
                )?;
                Ok(())
            })
            .unwrap();

        let (host_io2, mut peer2) = tokio::io::duplex(256 * 1024);
        let in_flight_gate = Arc::new(tokio::sync::Notify::new());
        let gate_peer = in_flight_gate.clone();
        let peer2_task = tokio::spawn(async move {
            fake_compose_hello(&mut peer2, 5501).await;
            let Req::ComposeStatus { id, project_id } = fake_compose_read(&mut peer2).await else {
                panic!("expected status probe")
            };
            gate_peer.notified().await;
            fake_compose_reply(
                &mut peer2,
                Resp::ComposeProjectStatus {
                    id,
                    project: arx_guest::compose::ProjectView {
                        project_id,
                        desired: arx_guest::compose::Desired::Started,
                        committed_revision: Some(1),
                        previous_revision: None,
                        recovery_required: false,
                        running_operation: None,
                        observation: arx_guest::compose::Observation::Live,
                        services: Vec::new(),
                    },
                },
            )
            .await;
            let Req::ComposeOperation { id, .. } = fake_compose_read(&mut peer2).await else {
                panic!("expected stop query")
            };
            fake_compose_reply(
                &mut peer2,
                Resp::ComposeRejected {
                    id,
                    code: RejectCode::UnknownOperation,
                    rule: None,
                },
            )
            .await;
            // compose_stop observes current project status before issuing ComposeStop.
            let Req::ComposeStatus { id, project_id } = fake_compose_read(&mut peer2).await else {
                panic!("expected pre-stop status")
            };
            fake_compose_reply(
                &mut peer2,
                Resp::ComposeProjectStatus {
                    id,
                    project: arx_guest::compose::ProjectView {
                        project_id,
                        desired: arx_guest::compose::Desired::Started,
                        committed_revision: Some(1),
                        previous_revision: None,
                        recovery_required: false,
                        running_operation: None,
                        observation: arx_guest::compose::Observation::Live,
                        services: Vec::new(),
                    },
                },
            )
            .await;
            let Req::ComposeStop {
                id,
                operation_id,
                project_id,
                ..
            } = fake_compose_read(&mut peer2).await
            else {
                panic!("expected compose_stop")
            };
            fake_compose_reply(
                &mut peer2,
                Resp::ComposeOperationState {
                    id,
                    operation: OperationView {
                        operation_id,
                        kind: OperationKind::Stop,
                        project_id,
                        revision: 1,
                        state: OperationState::Succeeded,
                        stage: OperationStage::Settled,
                        failure: None,
                        restored_previous: false,
                    },
                },
            )
            .await;
            // compose_stop observes post-stop live status before settling.
            let Req::ComposeStatus { id, project_id } = fake_compose_read(&mut peer2).await else {
                panic!("expected post-stop status")
            };
            fake_compose_reply(
                &mut peer2,
                Resp::ComposeProjectStatus {
                    id,
                    project: arx_guest::compose::ProjectView {
                        project_id,
                        desired: arx_guest::compose::Desired::Stopped,
                        committed_revision: Some(1),
                        previous_revision: None,
                        recovery_required: false,
                        running_operation: None,
                        observation: arx_guest::compose::Observation::Live,
                        services: Vec::new(),
                    },
                },
            )
            .await;
        });

        let mut ch_probe =
            crate::vm::guest::ComposeChannel::negotiate(host_io2, 5501, &h2.supervisor().cancel)
                .await
                .unwrap();
        let ch_backup = ch_probe.shared();
        let ch_lifecycle = ch_probe.shared();
        let cancel2 = h2.supervisor().cancel.clone();
        let probe_task = tokio::spawn(async move {
            ch_probe
                .exchange(&cancel2, |id| Req::ComposeStatus {
                    id,
                    project_id: "other-app".into(),
                })
                .await
                .unwrap()
        });
        tokio::time::sleep(Duration::from_millis(25)).await;
        assert!(
            ch_backup.is_open(),
            "shared ComposeChannel::is_open must stay true during an in-flight exchange"
        );

        store2
            .transact(|tx| crate::store::vm::compose::register_boot(tx, vm2_id, gen2, 5501))
            .unwrap();
        h2.supervisor()
            .compose_backup
            .attach(
                vm2_id,
                compose_backup::BackupSession::new(gen2, ch_backup.shared()),
            )
            .unwrap();
        h2.supervisor()
            .compose
            .attach(store2, vm2_id, compose::Session::new(gen2, ch_lifecycle))
            .unwrap();

        *h2.supervisor().compose_backup.timeouts.lock().unwrap() = compose_backup::BackupTimeouts {
            idle: Duration::from_secs(5),
            absolute: Duration::from_millis(200),
            poll: Duration::from_millis(5),
            guest_deadline_s: 10,
        };
        store2
            .transact(|tx| {
                Ok(tx.execute_batch(
            "CREATE TRIGGER reject_settlement BEFORE UPDATE OF status ON vm_compose_backup_ops
             BEGIN SELECT RAISE(ABORT,'fixture settlement failure'); END"
        )?)
            })
            .unwrap();
        let replay_args = json!({"vm_id": vm2, "project_id": "calm-life", "request_id": key(5510)});
        let refused = poll_compose_backup_terminal(&a2, &replay_args).await;
        assert_eq!(refused["reason"], "state_unavailable", "{refused}");
        let pending = store2
            .transact(|tx| {
                crate::store::vm::compose::backup_replay(
                    tx,
                    "alpha",
                    RequestId::parse(&key(5510)).unwrap().key,
                )
            })
            .unwrap()
            .unwrap();
        assert_eq!(pending.status, compose_backup::BackupOpStatus::Pending);
        assert_eq!(pending.host_staging_charge, max_bytes);
        // Shadow the replay table on this fixture connection. The pending row
        // on disk survives a failed SELECT and the subsequent retry uses it.
        store2
            .transact(|tx| {
                Ok(tx.execute_batch("CREATE TEMP TABLE vm_compose_backup_ops(missing INTEGER)")?)
            })
            .unwrap();
        let host = &h2.supervisor().compose_backup;
        let replay_failure = host
            .drive(
                &h2.supervisor().controller,
                "alpha",
                RequestId::parse(&key(5510)).unwrap().key,
                &pending.target,
                host.session(vm2_id).unwrap(),
                &h2.supervisor().cancel,
            )
            .await;
        store2
            .transact(|tx| Ok(tx.execute_batch("DROP TABLE temp.vm_compose_backup_ops")?))
            .unwrap();
        assert!(replay_failure.is_err());
        store2
            .transact(|tx| Ok(tx.execute_batch("DROP TRIGGER reject_settlement")?))
            .unwrap();
        let r_dl = poll_compose_backup_terminal(
            &a2,
            &json!({"vm_id": vm2, "project_id": "calm-life", "request_id": key(5510)}),
        )
        .await;
        assert_eq!(r_dl["state"], "interrupted", "{r_dl}");
        assert_eq!(r_dl["failure"], "guest_channel_error");
        in_flight_gate.notify_one();
        let _ = probe_task.await.unwrap();
        assert!(ch_backup.is_open(), "shared channel must remain open");

        let stop_req = json!({"vm_id": vm2, "project_id": "other-app", "request_id": key(5511)});
        let stop_deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            let reply = call(&a2, "compose_stop", stop_req.clone()).await;
            if reply["state"] == "succeeded" {
                break;
            }
            assert!(
                tokio::time::Instant::now() < stop_deadline,
                "compose_stop did not settle: {reply}"
            );
            tokio::time::sleep(Duration::from_millis(15)).await;
        }

        drop(a2);
        peer2_task.abort();
        h2.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn compose_backup_path_isolation_disk_cap_and_project_lease_serialization() {
        let mut h = Harness::start_with_web(false, false, true).await;
        let a = h.client(0).await;
        let max_bytes = 64 * 1024 * 1024;
        let (vm, vm_id, generation, target, _backup_dir, _managed) =
            setup_compose_backup_project(&h, &a, 5300, max_bytes).await;

        // 1. Path isolation: backup root overlapping VM state root is refused.
        assert!(compose_backup::ManagedBackupDir::new(h.temp.path(), h.temp.path(), None).is_err());
        let sub = h.temp.path().join("nested-backup");
        fs::create_dir_all(&sub).unwrap();
        assert!(compose_backup::ManagedBackupDir::new(&sub, h.temp.path(), None).is_err());
        let root = crate::vm::paths::LocalPath::parse("D:\\ArxcovoData\\backups").unwrap();
        let art = compose_backup::backup_artifact(
            &root,
            crate::vm::paths::ArtifactRole::ComposeBackup,
            [0xab; 16],
        )
        .unwrap();
        assert!(
            art.path_for(crate::vm::paths::ArtifactRole::ComposeBackup)
                .unwrap()
                .as_str()
                .ends_with(&format!("\\settled\\{}.sqlite3", "ab".repeat(16)))
        );
        assert!(
            compose_backup::backup_artifact(
                &root,
                crate::vm::paths::ArtifactRole::StaticDisk,
                [0xab; 16],
            )
            .is_err()
        );

        // Attach a live session on boot 5301.
        let (host_io, mut peer) = tokio::io::duplex(64 * 1024);
        let peer_task = tokio::spawn(async move {
            fake_compose_hello(&mut peer, 5301).await;
            tokio::time::sleep(Duration::from_secs(30)).await;
        });
        let channel =
            crate::vm::guest::ComposeChannel::negotiate(host_io, 5301, &h.supervisor().cancel)
                .await
                .unwrap();
        let store = h.supervisor().controller.compose_store();
        store
            .transact(|tx| crate::store::vm::compose::register_boot(tx, vm_id, generation, 5301))
            .unwrap();
        h.supervisor()
            .compose_backup
            .attach(
                vm_id,
                compose_backup::BackupSession::new(generation, channel.shared()),
            )
            .unwrap();
        h.supervisor()
            .compose
            .attach(store, vm_id, compose::Session::new(generation, channel))
            .unwrap();

        // 2. Insufficient host disk capacity refuses compose_backup with "disk".
        *h.supervisor().compose_backup.capacity.lock().unwrap() = Some(Capacity {
            free_bytes: 1024, // below 50 GiB free floor
            managed_bytes: 0,
        });
        let r_disk = call(
            &a,
            "compose_backup",
            json!({"vm_id": vm, "project_id": "calm-life", "request_id": key(5310)}),
        )
        .await;
        assert_eq!(r_disk["reason"], "disk", "{r_disk}");

        // Restore capacity and verify project lease serialization:
        // a pending backup blocks compose_stop/compose_deploy with "busy".
        *h.supervisor().compose_backup.capacity.lock().unwrap() = Some(Capacity {
            free_bytes: 150 * 1024 * 1024 * 1024,
            managed_bytes: 0,
        });
        let pending = call(
            &a,
            "compose_backup",
            json!({"vm_id": vm, "project_id": "calm-life", "request_id": key(5311)}),
        )
        .await;
        assert_eq!(pending["state"], "pending", "{pending}");
        // Second concurrent backup on same project refused as "busy".
        let busy_backup = call(
            &a,
            "compose_backup",
            json!({"vm_id": vm, "project_id": "calm-life", "request_id": key(5312)}),
        )
        .await;
        assert_eq!(busy_backup["reason"], "busy", "{busy_backup}");
        // Lifecycle op (compose_stop) on same project also refused as "busy".
        let busy_stop = call(
            &a,
            "compose_stop",
            json!({"vm_id": vm, "project_id": "calm-life", "request_id": key(5313)}),
        )
        .await;
        assert_eq!(busy_stop["reason"], "busy", "{busy_stop}");

        // Shutdown cancels the stalled peer cleanly.
        let _ = target;
        drop(a);
        peer_task.abort();
        h.shutdown().await.unwrap();
    }
}
