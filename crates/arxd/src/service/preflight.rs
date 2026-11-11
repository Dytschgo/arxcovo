//! Fixed response-only details. Never extend the SQLite reason vocabulary.
use crate::store::Reason;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionUnavailable {
    NotConfigured,
    ServiceModeRequired,
    PlatformUnsupported,
    AccountMissing,
    AccountIdentityChanged,
    RightsMismatch,
    AdminUnsupportedOnPlatform,
    BatchRightMissing,
    MechanismUnavailable,
    ServiceReconfigurationRequired,
    PreflightFailed,
    Quarantined,
    Busy,
    Stopping,
}
impl ExecutionUnavailable {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotConfigured => "not_configured",
            Self::ServiceModeRequired => "service_mode_required",
            Self::PlatformUnsupported => "platform_unsupported",
            Self::AccountMissing => "account_missing",
            Self::AccountIdentityChanged => "account_identity_changed",
            Self::RightsMismatch => "rights_mismatch",
            Self::AdminUnsupportedOnPlatform => "admin_unsupported_on_platform",
            Self::BatchRightMissing => "batch_right_missing",
            Self::MechanismUnavailable => "mechanism_unavailable",
            Self::ServiceReconfigurationRequired => "service_reconfiguration_required",
            Self::PreflightFailed => "preflight_failed",
            Self::Quarantined => "quarantined",
            Self::Busy => "busy",
            Self::Stopping => "stopping",
        }
    }
    pub fn stored_reason(self) -> Reason {
        match self {
            Self::NotConfigured => Reason::Unavailable,
            Self::ServiceModeRequired => Reason::Unavailable,
            Self::PlatformUnsupported => Reason::Unavailable,
            Self::AccountMissing => Reason::PreflightFailed,
            Self::AccountIdentityChanged => Reason::PreflightFailed,
            Self::RightsMismatch => Reason::PreflightFailed,
            Self::AdminUnsupportedOnPlatform => Reason::PreflightFailed,
            Self::BatchRightMissing => Reason::PreflightFailed,
            Self::MechanismUnavailable => Reason::PreflightFailed,
            Self::ServiceReconfigurationRequired => Reason::PreflightFailed,
            Self::PreflightFailed => Reason::PreflightFailed,
            Self::Quarantined => Reason::Quarantined,
            Self::Busy => Reason::Busy,
            Self::Stopping => Reason::Shutdown,
        }
    }
}
