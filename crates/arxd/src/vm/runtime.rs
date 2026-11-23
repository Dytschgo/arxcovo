use super::reaper::CleanupWork;
use crate::config::VmRuntimeProfile;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeRefusal {
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
    /// Compatibility refusal for the unwired private template adapter.
    InstallerPhaseUnavailable,
    InstallerPhaseMismatch,
    InstallerDeadlineExpired,
}

pub enum NativeCapability {
    Unavailable(NativeRefusal),
    Qualified(LaunchAuthority),
}

/// Intentionally uninhabited until a reviewed issuer can bind the full
/// qualification record and live evidence to a particular owned operation.
pub struct LaunchAuthority {
    _closed: std::convert::Infallible,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMutation {
    CopyMedia,
    Boot,
    CreateImage,
}

pub struct CandidateRuntime<'a> {
    profile: Option<&'a VmRuntimeProfile>,
}

impl<'a> CandidateRuntime<'a> {
    pub fn new(profile: Option<&'a VmRuntimeProfile>) -> Self {
        Self { profile }
    }

    pub fn capability(&self) -> NativeCapability {
        let reason = if !self.profile.is_some_and(VmRuntimeProfile::enabled) {
            NativeRefusal::Disabled
        } else if !cfg!(all(windows, target_arch = "x86_64")) {
            NativeRefusal::UnsupportedPlatform
        } else {
            NativeRefusal::QualificationMissing
        };
        NativeCapability::Unavailable(reason)
    }

    /// A fixture copy receipt has no offline launch or native file authority.
    pub fn snapshot_lease(&self) -> Result<OfflineLease, NativeRefusal> {
        Err(NativeRefusal::QualificationMissing)
    }

    /// Refuse before the private lifecycle can stage anything. Native launch
    /// independently requires an operation-bound LaunchAuthority.
    #[allow(
        dead_code,
        reason = "The private lifecycle has no production entry point yet"
    )]
    pub(super) fn template_installer(&self) -> Result<std::convert::Infallible, NativeRefusal> {
        Err(NativeRefusal::InstallerPhaseUnavailable)
    }

    /// The callback is the first boundary for external effects. Even a valid
    /// portable admission is insufficient to reach it.
    pub fn dispatch<T>(
        &self,
        _operation: NativeMutation,
        effect: impl FnOnce(LaunchAuthority) -> T,
    ) -> Result<T, NativeRefusal> {
        match self.capability() {
            NativeCapability::Unavailable(reason) => Err(reason),
            NativeCapability::Qualified(authority) => Ok(effect(authority)),
        }
    }
}

/// Diagnostic checks can describe absent evidence but cannot issue authority.
#[derive(Default)]
pub struct QualificationEvidence {
    pub identity: bool,
    pub distribution: bool,
    pub path_custody: bool,
    pub job: bool,
    pub whpx: bool,
    pub qmp: bool,
    pub guest_endpoint: bool,
    pub disk_ceiling: bool,
    pub owner_activity: bool,
    pub reserves: bool,
    pub network_fence: bool,
}

impl QualificationEvidence {
    pub fn check(&self) -> Result<(), NativeRefusal> {
        use NativeRefusal::*;
        [
            (self.identity, IdentityUnverified),
            (self.distribution, DistributionUnverified),
            (self.path_custody, PathCustodyUnverified),
            (self.job, JobUnverified),
            (self.whpx, WhpxUnverified),
            (self.qmp, QmpUnverified),
            (self.guest_endpoint, GuestEndpointUnverified),
            (self.disk_ceiling, DiskCeilingUnverified),
            (self.owner_activity, OwnerActivityUnverified),
            (self.reserves, ReservesUnverified),
            (self.network_fence, NetworkFenceUnverified),
        ]
        .into_iter()
        .find_map(|(proved, reason)| (!proved).then_some(reason))
        .map_or(Ok(()), Err)
    }
}

/// Maximum age of a host observation used to admit or start work.
pub const FRESHNESS: Duration = Duration::from_secs(5);

pub fn fresh_observation(
    captured: Instant,
    now: Instant,
    missing: NativeRefusal,
) -> Result<(), NativeRefusal> {
    now.checked_duration_since(captured)
        .filter(|age| *age <= FRESHNESS)
        .map(|_| ())
        .ok_or(missing)
}

/// Remaining headroom on the fixed volume and its host backing are independent.
pub fn capacity(
    allocated: u64,
    inner_free: u64,
    outer_free: u64,
    total: u64,
    ceiling: u64,
    floor: u64,
) -> Result<super::Capacity, NativeRefusal> {
    if total == 0 || total > ceiling || allocated.checked_add(inner_free).is_none_or(|n| n > total)
    {
        return Err(NativeRefusal::DiskCeilingUnverified);
    }
    let free_bytes = outer_free.min(
        inner_free
            .checked_add(floor)
            .ok_or(NativeRefusal::DiskCeilingUnverified)?,
    );
    Ok(super::Capacity {
        free_bytes,
        managed_bytes: allocated,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StopDisposition {
    CleanRelease,
    DirtyRelease,
    ChargedQuarantine,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstallerRecipe {
    Ubuntu,
    Windows,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstallerPhase {
    UbuntuInstall,
    WindowsSetup,
    WindowsFinalize,
}

impl InstallerPhase {
    pub fn recipe(self) -> InstallerRecipe {
        match self {
            Self::UbuntuInstall => InstallerRecipe::Ubuntu,
            Self::WindowsSetup | Self::WindowsFinalize => InstallerRecipe::Windows,
        }
    }
}

/// Create once before the first launch and keep through detached verification.
/// Per-stage limits can shorten this budget only.
pub struct InstallerBudget {
    recipe: InstallerRecipe,
    deadline: tokio::time::Instant,
}

impl InstallerBudget {
    pub fn start(recipe: InstallerRecipe) -> Self {
        let maximum = match recipe {
            InstallerRecipe::Ubuntu => Duration::from_secs(60 * 60),
            InstallerRecipe::Windows => Duration::from_secs(3 * 60 * 60),
        };
        Self {
            recipe,
            deadline: tokio::time::Instant::now() + maximum,
        }
    }

    /// Also used to clip installed-mode health and verification operations.
    pub fn clip(&self, limit: Duration) -> tokio::time::Instant {
        let now = tokio::time::Instant::now();
        if now >= self.deadline {
            self.deadline
        } else {
            now + limit.min(self.deadline - now)
        }
    }

    pub fn stage(&self, phase: InstallerPhase, limit: Duration) -> Option<InstallerStage> {
        (self.recipe == phase.recipe()).then(|| InstallerStage {
            phase,
            deadline: self.clip(limit),
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct InstallerStage {
    phase: InstallerPhase,
    deadline: tokio::time::Instant,
}

impl InstallerStage {
    pub fn phase(self) -> InstallerPhase {
        self.phase
    }

    pub fn deadline(self) -> tokio::time::Instant {
        self.deadline
    }

    /// Resolve a phase only after teardown has produced its release evidence.
    pub fn complete(
        self,
        result: Result<(), InstallerFailureReason>,
        observed: StopDisposition,
        cancelled: bool,
    ) -> Result<(), InstallerFailure> {
        use InstallerFailureReason as Failure;
        let reason = match result {
            Err(reason) => reason,
            Ok(()) if cancelled => Failure::Cancelled,
            Ok(()) if tokio::time::Instant::now() >= self.deadline => Failure::Timeout,
            Ok(()) if observed == StopDisposition::CleanRelease => return Ok(()),
            Ok(()) => Failure::ReleaseUnverified,
        };
        let recovery = if observed == StopDisposition::ChargedQuarantine {
            StopDisposition::ChargedQuarantine
        } else {
            StopDisposition::DirtyRelease
        };
        Err(InstallerFailure {
            phase: self.phase,
            reason,
            recovery,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstallerFailureReason {
    Timeout,
    Cancelled,
    Native(NativeRefusal),
    Protocol(super::qmp::ChannelError),
    ReleaseUnverified,
    ProcessFailed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InstallerFailure {
    pub phase: InstallerPhase,
    pub reason: InstallerFailureReason,
    pub recovery: StopDisposition,
}

#[derive(Default)]
pub struct StopEvidence {
    pub guest_shutdown: bool,
    pub clean_process_exit: bool,
    pub process_exited: bool,
    pub job_empty: bool,
    pub workers_joined: bool,
    pub artifacts_reopened: bool,
}

impl StopEvidence {
    /// The release check may acquire exclusive writer custody. It may only
    /// run once the child, its whole Job and its workers have stopped.
    pub fn verify_release(&mut self, verify: impl FnOnce() -> bool) {
        self.artifacts_reopened = self.ready_to_verify_release() && verify();
    }

    pub(super) fn ready_to_verify_release(&self) -> bool {
        self.process_exited && self.job_empty && self.workers_joined
    }

    pub fn disposition(&self) -> StopDisposition {
        if !(self.process_exited
            && self.job_empty
            && self.workers_joined
            && self.artifacts_reopened)
        {
            return StopDisposition::ChargedQuarantine;
        }
        if self.guest_shutdown && self.clean_process_exit {
            StopDisposition::CleanRelease
        } else {
            StopDisposition::DirtyRelease
        }
    }
}

/// Release must be bound to the old generation and pin the slot against reuse.
/// An observed process exit or fixture receipt cannot produce this lease.
pub struct OfflineLease {
    _closed: std::convert::Infallible,
}

pub const SNAPSHOT_COPY_MAXIMUM: Duration = Duration::from_secs(15 * 60);

/// A copy may shorten the controller's admission deadline. Each blocking
/// custody call receives this same deadline, including during inspection.
pub(super) fn snapshot_deadline(worker: &super::PendingOperation, start: Instant) -> Instant {
    start
        .checked_add(SNAPSHOT_COPY_MAXIMUM)
        .unwrap_or(worker.deadline())
        .min(worker.deadline())
}

/// One teardown authority per work item. Implementations must bind it to the
/// recorded subject and generation and serialize it against boot and stop.
/// Every method must return by its monotonic deadline.
pub(crate) trait ReaperRuntime {
    /// Signal the outstanding worker and join it or return Unconfirmed.
    /// Offline requires independent process, worker and artifact release proof,
    /// including on restart. A missing PID is insufficient.
    fn quiesce(&self, work: &CleanupWork, deadline: Instant) -> Quiesced;

    fn graceful_stop(&self, work: &CleanupWork, deadline: Instant) -> StopDisposition;

    /// Invoked only after quiesce returned an owned TEST Job for a lease
    /// expiry. The adapter must revalidate the held Job before using it.
    fn force_reap(&self, work: &CleanupWork, deadline: Instant) -> StopDisposition;

    /// Non-deleting release proof for a committed static generation after an
    /// interrupted boot. True requires the same independent process, worker
    /// and Job release proof as [`ReaperRuntime::quiesce`] for this exact VM
    /// and generation. It grants no artifact authority. Closed by default.
    fn static_released(&self, vm: super::Id, generation: super::Id, deadline: Instant) -> bool {
        let _ = (vm, generation, deadline);
        false
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[allow(dead_code, reason = "native release authority is an activation gate")]
pub(crate) enum Quiesced {
    Offline,
    OwnedTestJob,
    Unconfirmed,
}

impl ReaperRuntime for CandidateRuntime<'_> {
    fn quiesce(&self, _: &CleanupWork, _: Instant) -> Quiesced {
        Quiesced::Unconfirmed
    }

    fn graceful_stop(&self, _: &CleanupWork, _: Instant) -> StopDisposition {
        StopDisposition::ChargedQuarantine
    }

    fn force_reap(&self, _: &CleanupWork, _: Instant) -> StopDisposition {
        StopDisposition::ChargedQuarantine
    }
}
