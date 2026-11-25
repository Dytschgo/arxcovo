use std::time::Instant;

pub const RETRY_WINDOW_MS: i64 = 24 * 60 * 60 * 1000;
pub const DEFAULT_LEASE_SECONDS: u64 = 7 * 24 * 60 * 60;

pub(crate) const MAX_UTC_MS: i64 = 253_402_300_799_999;

/// Opaque generated VM, template, or snapshot identity. Never a filesystem path.
pub type Id = [u8; 16];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RequestId {
    pub(crate) key: Id,
    pub(crate) issued_at_ms: i64,
}

impl RequestId {
    /// The first 16 hex digits encode Unix milliseconds. The remaining 16
    /// encode a caller-generated nonce. Retries must reuse the original key.
    pub fn parse(value: &str) -> Result<Self, Refusal> {
        if value.len() != 32
            || !value
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        {
            return Err(Refusal::Invalid);
        }
        let mut key = [0; 16];
        hex::decode_to_slice(value, &mut key).map_err(|_| Refusal::Invalid)?;
        let issued_at_ms = i64::from_be_bytes(key[..8].try_into().expect("fixed timestamp width"));
        if !(0..=MAX_UTC_MS).contains(&issued_at_ms) {
            return Err(Refusal::Invalid);
        }
        Ok(Self { key, issued_at_ms })
    }
}

#[derive(Clone, Copy)]
pub struct ClockReading {
    pub utc_ms: i64,
    pub monotonic: Instant,
}

pub trait Clock: Send + Sync {
    fn now(&self) -> ClockReading;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i64)]
pub enum Action {
    Clone = 1,
    Script,
    Start,
    Stop,
    Snapshot,
    Restore,
    Expire,
    Extend,
    SealTemplate,
    Recover,
}

pub enum Operation<'a> {
    Clone {
        template: Id,
        lease_seconds: Option<u64>,
    },
    Script {
        script: &'a str,
        timeout_seconds: u64,
    },
    Start,
    Stop,
    Snapshot {
        snapshot: Id,
    },
    Restore {
        snapshot: Id,
    },
}

impl Operation<'_> {
    pub(crate) fn action(&self) -> Action {
        match self {
            Self::Clone { .. } => Action::Clone,
            Self::Script { .. } => Action::Script,
            Self::Start => Action::Start,
            Self::Stop => Action::Stop,
            Self::Snapshot { .. } => Action::Snapshot,
            Self::Restore { .. } => Action::Restore,
        }
    }
}

pub struct Request<'a> {
    pub principal: &'a str,
    pub request_id: RequestId,
    pub vm: Id,
    pub operation: Operation<'a>,
    pub disk: DiskClaim,
}

/// Worst-case additional bytes. Include old/new coexistence and temporary
/// artifacts in staging, and format overhead in metadata.
#[derive(Clone, Copy, Debug, Default)]
pub struct DiskClaim {
    pub staging: u64,
    pub images: u64,
    pub metadata: u64,
}

impl DiskClaim {
    pub(crate) fn persistent(self) -> Option<u64> {
        self.images.checked_add(self.metadata)
    }

    pub(crate) fn total(self) -> Option<u64> {
        self.persistent()?.checked_add(self.staging)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i64)]
pub enum Guest {
    UbuntuX86_64 = 1,
    WindowsX86_64,
}

pub struct SealedTemplate {
    pub id: Id,
    pub guest: Guest,
    pub recipe_digest: [u8; 32],
    pub disk_digest: [u8; 32],
    pub vars_digest: [u8; 32],
    pub disk: DiskClaim,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i64)]
pub enum Refusal {
    Invalid = 1,
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Pending,
    Succeeded,
    Failed,
    Interrupted,
}

impl Status {
    pub(crate) fn code(self) -> i64 {
        match self {
            Self::Pending => 0,
            Self::Succeeded => 1,
            Self::Failed => 2,
            Self::Interrupted => 3,
        }
    }

    pub(crate) fn from_code(code: i64) -> rusqlite::Result<Self> {
        match code {
            0 => Ok(Self::Pending),
            1 => Ok(Self::Succeeded),
            2 => Ok(Self::Failed),
            3 => Ok(Self::Interrupted),
            _ => Err(rusqlite::Error::InvalidQuery),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Receipt {
    pub activity_id: i64,
    pub status: Status,
}

#[derive(Debug)]
pub enum Admission {
    Admitted(PendingOperation),
    Existing(Receipt),
    Refused(Refusal),
}

/// Only a fresh admission yields a worker handle. A status lookup has no
/// dispatch authority. Deadlines are process-local and never persisted.
#[derive(Debug)]
pub struct PendingOperation {
    pub(crate) activity_id: i64,
    pub(crate) deadline: Instant,
    pub(crate) binding: std::sync::Arc<()>,
}

impl PendingOperation {
    pub fn activity_id(&self) -> i64 {
        self.activity_id
    }

    pub fn deadline(&self) -> Instant {
        self.deadline
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Completion {
    Succeeded,
    Failed,
    CleanupUnconfirmed,
}

/// Indicates the external reaper has independently proved release of the
/// resources charged to this intent. This metadata slice performs no I/O
/// against VM artifacts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CleanupResult {
    Confirmed,
    Unconfirmed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExpiryClaim {
    pub vm: Id,
    pub activity_id: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExpiredVm {
    pub expired_at_ms: i64,
    pub activity_id: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecoveryClaim {
    pub vm: Id,
    pub activity_id: i64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Subject<'a> {
    Vm(Id),
    Template(Id, [u8; 32]),
    Snapshot(Id, [u8; 32]),
    Recipe(&'a str, Option<[u8; 32]>),
}

pub(crate) const BUILD_ACTION: i64 = 11;
pub(crate) const STATIC_CREATE_ACTION: i64 = 12;
pub(crate) const READ_REFUSAL_ACTION: i64 = 13;
pub(crate) const HOST_REFUSAL_ACTION: i64 = 14;
pub(crate) const PUBLISH_ACTION: i64 = 15;
pub(crate) const UNPUBLISH_ACTION: i64 = 16;
pub(crate) const SNAPSHOT_DELETE_ACTION: i64 = 18;
pub(crate) const COMPOSE_BACKUP_ACTION: i64 = 24;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i64)]
pub(crate) enum FailureReason {
    StagingFailed = 1,
    InstallFailed,
    InstallTimedOut,
    BootFailed,
    HealthFailed,
    SealFailed,
    CloneFailed,
    DirtyStop,
    ExecutionUnknown,
    Cancelled,
    CleanupUnconfirmed,
    DnsConflict,
    DnsUnavailable,
    ConnectorFailed,
    ConnectorExited,
    ConnectorUnready,
    OriginUnavailable,
}

impl FailureReason {
    const ALL: [Self; 17] = {
        use FailureReason::*;
        [
            StagingFailed,
            InstallFailed,
            InstallTimedOut,
            BootFailed,
            HealthFailed,
            SealFailed,
            CloneFailed,
            DirtyStop,
            ExecutionUnknown,
            Cancelled,
            CleanupUnconfirmed,
            DnsConflict,
            DnsUnavailable,
            ConnectorFailed,
            ConnectorExited,
            ConnectorUnready,
            OriginUnavailable,
        ]
    };

    pub(crate) fn from_code(code: i64) -> rusqlite::Result<Self> {
        Self::ALL
            .into_iter()
            .find(|reason| *reason as i64 == code)
            .ok_or(rusqlite::Error::InvalidQuery)
    }

    pub(crate) fn wire(self) -> &'static str {
        match self {
            Self::StagingFailed => "staging_failed",
            Self::InstallFailed => "install_failed",
            Self::InstallTimedOut => "install_timed_out",
            Self::BootFailed => "boot_failed",
            Self::HealthFailed => "health_failed",
            Self::SealFailed => "seal_failed",
            Self::CloneFailed => "clone_failed",
            Self::DirtyStop => "dirty_stop",
            Self::ExecutionUnknown => "execution_unknown",
            Self::Cancelled => "cancelled",
            Self::CleanupUnconfirmed => "cleanup_unconfirmed",
            Self::DnsConflict => "dns_conflict",
            Self::DnsUnavailable => "dns_unavailable",
            Self::ConnectorFailed => "connector_failed",
            Self::ConnectorExited => "connector_exited",
            Self::ConnectorUnready => "connector_unready",
            Self::OriginUnavailable => "origin_unavailable",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct OperationRef {
    pub id: Id,
    pub receipt: Receipt,
}

#[derive(Clone, Copy)]
pub(crate) struct BuildRequest<'a> {
    pub principal: &'a str,
    pub key: RequestId,
    pub recipe: &'a str,
    pub digest: [u8; 32],
    pub retained: DiskClaim,
    pub transient: DiskClaim,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ArtifactSlot {
    pub generation: Id,
    pub role: &'static str,
    pub key: Id,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PairSlots {
    pub disk: ArtifactSlot,
    pub vars: ArtifactSlot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BuildSlots {
    pub id: Id,
    pub pair: PairSlots,
    pub seed: ArtifactSlot,
    pub verification: PairSlots,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ArtifactEvidence {
    pub identity: super::paths::FileIdentity,
    pub directory: Id,
    pub pin: super::paths::ArtifactPin,
}

/// What one slot of a never-activated static candidate is proven to hold
/// after a failed copy. Only the candidate's own worker reports this; it is
/// never authority over committed static or backup storage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code, reason = "native static copy is an activation gate")]
pub(crate) enum CandidateSlot {
    /// Never created, or created and disposed by its worker after an
    /// identity check of the exact object it created.
    Absent,
    /// Created with this identity and not disposed. Quarantined and charged.
    Retained(ArtifactEvidence),
    /// Effects unknown. Quarantined and charged.
    Unknown,
}

/// A failed static copy: the proven terminal cause plus per-slot custody.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code, reason = "native static copy is an activation gate")]
pub(crate) struct CopyFailure {
    pub reason: FailureReason,
    pub slots: [CandidateSlot; 2],
}

#[allow(dead_code, reason = "native static copy is an activation gate")]
impl CopyFailure {
    /// No slot effect is known; nothing may be released.
    pub(crate) fn unknown(reason: FailureReason) -> Self {
        Self {
            reason,
            slots: [CandidateSlot::Unknown; 2],
        }
    }

    pub(crate) fn untouched(reason: FailureReason) -> Self {
        Self {
            reason,
            slots: [CandidateSlot::Absent; 2],
        }
    }
}

#[derive(Debug)]
pub(crate) enum BuildAdmission {
    Fresh(Box<BuildSlots>, PendingOperation, OperationRef),
    Existing(OperationRef),
}

#[derive(Debug)]
pub(crate) enum CloneAdmission {
    Fresh(Id, PairSlots, PendingOperation, OperationRef),
    Existing(OperationRef),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScriptResult {
    Exited(u32),
    Unavailable,
    TimedOut,
    LaunchFailed,
    Terminated,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct OperationStatus {
    pub id: Id,
    pub action: &'static str,
    pub state: Status,
    pub stage: Option<&'static str>,
    pub cleanup: Option<&'static str>,
    pub vm: Option<Id>,
    pub recipe: Option<String>,
    pub template: Option<Id>,
    pub expires_at_ms: Option<i64>,
    pub snapshot: Option<Id>,
    pub script: Option<ScriptResult>,
    pub failure: Option<FailureReason>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct VmStatus {
    pub id: Id,
    pub template: Option<Id>,
    pub guest: Option<Guest>,
    pub state: &'static str,
    pub ready: bool,
    pub expires_at_ms: Option<i64>,
    pub expired_at_ms: Option<i64>,
    pub cleanup: Option<&'static str>,
    pub snapshots: [Id; 0],
    pub cleanup_activity: Option<i64>,
}
