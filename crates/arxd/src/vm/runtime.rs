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
