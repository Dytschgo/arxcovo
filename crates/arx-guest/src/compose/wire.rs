//! Compose session frames. They share the v1 framing and 32 KiB ceiling with
//! the legacy messages but are decoded only after a pinned ComposeHello on a
//! guest that serves Compose. Every tag starts with `Compose`.
//!
//! No reply carries a path, URL or host instruction. Replies hold opaque IDs
//! the host already sent, digests, counts and closed enums only, so nothing a
//! guest says can authorize a host read, write or fetch.

use serde::{Deserialize, Serialize};

use super::codec::{SecretText, is_base64_text, is_lower_hex};
use super::manifest::{ManifestRule, is_project_id, is_slot_name};
use crate::protocol::{Message, ProtocolError, private};

/// Version of the Compose session. It is pinned separately from the legacy
/// frame version, which stays at 1.
pub const COMPOSE_PROTOCOL_VERSION: u32 = 1;
/// The closed, sorted capability set. A ComposeHello must name exactly this
/// list; anything else closes the connection.
pub const COMPOSE_CAPABILITIES: [&str; 5] = ["lifecycle", "logs", "manifest", "secrets", "status"];
/// Decoded bytes per staged manifest chunk. Only the last chunk is shorter.
pub const STAGE_CHUNK: usize = 16 * 1024;
/// Decoded bytes per staged manifest.
pub const STAGE_TOTAL: usize = 128 * 1024;
/// Decoded bytes per secret value.
pub const MAX_SECRET: usize = 4096;
pub const MAX_LOG_ENTRIES: u32 = 32;
/// Serialized bytes of one log page reply.
pub const MAX_LOG_PAGE: usize = 8 * 1024;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum ComposeRequest {
    ComposeHello {
        version: u32,
        challenge: String,
        generation: u64,
        capabilities: Vec<String>,
    },
    ComposeStageBegin {
        id: u32,
        digest: String,
        total_bytes: u32,
        chunks: u32,
    },
    ComposeStageChunk {
        id: u32,
        digest: String,
        index: u32,
        data: String,
    },
    ComposeStageCommit {
        id: u32,
        digest: String,
    },
    ComposeDeploy {
        id: u32,
        operation_id: String,
        project_id: String,
        revision: u64,
        digest: String,
    },
    ComposeStart {
        id: u32,
        operation_id: String,
        project_id: String,
    },
    ComposeStop {
        id: u32,
        operation_id: String,
        project_id: String,
    },
    ComposeDown {
        id: u32,
        operation_id: String,
        project_id: String,
    },
    ComposeOperation {
        id: u32,
        operation_id: String,
    },
    ComposeReconcile {
        id: u32,
        operation_id: String,
    },
    ComposeStatus {
        id: u32,
        project_id: String,
    },
    ComposeLogs {
        id: u32,
        project_id: String,
        cursor: u64,
        limit: u32,
    },
    /// Private, non-logging secret delivery. `data` is Base64 and is wiped
    /// when the request is dropped.
    ComposeSecretPut {
        id: u32,
        operation_id: String,
        generation: u64,
        project_id: String,
        slot: String,
        epoch: u64,
        data: SecretText,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum ComposeResponse {
    ComposeHelloAck {
        version: u32,
        challenge: String,
        generation: u64,
        capabilities: Vec<String>,
        cleared_staging: u32,
        ambiguous_operations: u32,
    },
    ComposeStageProgress {
        id: u32,
        digest: String,
        received_bytes: u32,
        next_index: u32,
    },
    ComposeReleaseStaged {
        id: u32,
        digest: String,
        project_id: String,
    },
    ComposeOperationState {
        id: u32,
        operation: OperationView,
    },
    ComposeProjectStatus {
        id: u32,
        project: ProjectView,
    },
    ComposeLogPage {
        id: u32,
        entries: Vec<LogEntry>,
        next_cursor: u64,
    },
    ComposeSecretAccepted {
        id: u32,
        project_id: String,
        slot: String,
        epoch: u64,
        recreate: Option<OperationView>,
    },
    ComposeRejected {
        id: u32,
        code: RejectCode,
        rule: Option<ManifestRule>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectCode {
    Busy,
    ReconcileRequired,
    UnknownOperation,
    OperationConflict,
    NotAmbiguous,
    UnknownProject,
    ProjectLimit,
    VolumeLimit,
    VolumeCollision,
    NoStaging,
    StageOrder,
    StageSize,
    StageIncomplete,
    StageDigest,
    ManifestRejected,
    UnknownRelease,
    ProjectMismatch,
    RevisionNotNewer,
    NotCommitted,
    GenerationMismatch,
    UnknownSlot,
    StaleEpoch,
    SecretSize,
    SecretMissing,
    SecretOwnerMismatch,
    SecretWrite,
    StoreFailure,
    EngineUnavailable,
    JournalFull,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    Deploy,
    Start,
    Stop,
    Down,
    Recreate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationState {
    Running,
    Succeeded,
    Failed,
    /// A physical effect may or may not have happened. Only reconcile
    /// settles it; the effect is never re-run blindly.
    Ambiguous,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationStage {
    Queued,
    Rendering,
    Pulling,
    Volumes,
    Applying,
    HealthCheck,
    Restoring,
    Stopping,
    Removing,
    Recreating,
    Settled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureCode {
    PullFailed,
    PullTimedOut,
    ImageIdentity,
    VolumeCollision,
    VolumeFailed,
    UpFailed,
    HealthFailed,
    StopFailed,
    DownFailed,
    StoreFailed,
    EngineLaunchFailed,
    EngineTimedOut,
    NotApplied,
    WorkerUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationView {
    pub operation_id: String,
    pub kind: OperationKind,
    pub project_id: String,
    pub revision: u64,
    pub state: OperationState,
    pub stage: OperationStage,
    pub failure: Option<FailureCode>,
    pub restored_previous: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Desired {
    None,
    Started,
    Stopped,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceState {
    Created,
    Running,
    Restarting,
    Paused,
    Exited,
    Dead,
    Removing,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthState {
    None,
    Starting,
    Healthy,
    Unhealthy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Observation {
    Live,
    Recorded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceView {
    pub name: String,
    pub state: ServiceState,
    pub health: HealthState,
    pub revision: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectView {
    pub project_id: String,
    pub desired: Desired,
    pub committed_revision: Option<u64>,
    pub previous_revision: Option<u64>,
    pub recovery_required: bool,
    pub running_operation: Option<String>,
    pub observation: Observation,
    pub services: Vec<ServiceView>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogEvent {
    OperationStarted,
    ImageReady,
    ImagePulled,
    VolumeReady,
    Applied,
    HealthPassed,
    HealthFailed,
    Promoted,
    Restored,
    Stopped,
    Removed,
    Recreated,
    OperationFailed,
    OperationAmbiguous,
    Reconciled,
    SecretDelivered,
    StagingCleared,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogEntry {
    pub seq: u64,
    pub unix_s: u64,
    pub project_id: String,
    pub event: LogEvent,
    pub service: Option<String>,
    pub failure: Option<FailureCode>,
}

impl private::Sealed for ComposeRequest {}

impl Message for ComposeRequest {
    fn validate(&self) -> Result<(), ProtocolError> {
        match self {
            Self::ComposeHello {
                version,
                challenge,
                generation,
                capabilities,
            } => {
                validate_hello(*version, challenge, *generation)?;
                if capabilities.len() != COMPOSE_CAPABILITIES.len()
                    || capabilities
                        .iter()
                        .zip(COMPOSE_CAPABILITIES)
                        .any(|(offered, pinned)| offered != pinned)
                {
                    return Err(ProtocolError::UnsupportedCapability);
                }
                Ok(())
            }
            Self::ComposeStageBegin { id, digest, .. }
            | Self::ComposeStageCommit { id, digest } => {
                validate_id(*id)?;
                field(is_lower_hex(digest, 64))
            }
            Self::ComposeStageChunk {
                id, digest, data, ..
            } => {
                validate_id(*id)?;
                field(is_lower_hex(digest, 64) && is_base64_text(data, STAGE_CHUNK))
            }
            Self::ComposeDeploy {
                id,
                operation_id,
                project_id,
                revision,
                digest,
            } => {
                validate_id(*id)?;
                field(
                    is_lower_hex(operation_id, 32)
                        && is_project_id(project_id)
                        && *revision != 0
                        && is_lower_hex(digest, 64),
                )
            }
            Self::ComposeStart {
                id,
                operation_id,
                project_id,
            }
            | Self::ComposeStop {
                id,
                operation_id,
                project_id,
            }
            | Self::ComposeDown {
                id,
                operation_id,
                project_id,
            } => {
                validate_id(*id)?;
                field(is_lower_hex(operation_id, 32) && is_project_id(project_id))
            }
            Self::ComposeOperation { id, operation_id }
            | Self::ComposeReconcile { id, operation_id } => {
                validate_id(*id)?;
                field(is_lower_hex(operation_id, 32))
            }
            Self::ComposeStatus { id, project_id } => {
                validate_id(*id)?;
                field(is_project_id(project_id))
            }
            Self::ComposeLogs {
                id,
                project_id,
                limit,
                ..
            } => {
                validate_id(*id)?;
                field(is_project_id(project_id) && (1..=MAX_LOG_ENTRIES).contains(limit))
            }
            Self::ComposeSecretPut {
                id,
                operation_id,
                generation,
                project_id,
                slot,
                epoch,
                data,
            } => {
                validate_id(*id)?;
                field(
                    is_lower_hex(operation_id, 32)
                        && *generation != 0
                        && is_project_id(project_id)
                        && is_slot_name(slot)
                        && *epoch != 0
                        && is_base64_text(data.as_str(), MAX_SECRET),
                )
            }
        }
    }
}

impl private::Sealed for ComposeResponse {}

impl Message for ComposeResponse {
    fn validate(&self) -> Result<(), ProtocolError> {
        match self {
            Self::ComposeHelloAck {
                version,
                challenge,
                generation,
                ..
            } => validate_hello(*version, challenge, *generation),
            Self::ComposeStageProgress { id, .. }
            | Self::ComposeReleaseStaged { id, .. }
            | Self::ComposeOperationState { id, .. }
            | Self::ComposeProjectStatus { id, .. }
            | Self::ComposeLogPage { id, .. }
            | Self::ComposeSecretAccepted { id, .. }
            | Self::ComposeRejected { id, .. } => validate_id(*id),
        }
    }
}

fn validate_hello(version: u32, challenge: &str, generation: u64) -> Result<(), ProtocolError> {
    if version != COMPOSE_PROTOCOL_VERSION {
        return Err(ProtocolError::UnsupportedVersion);
    }
    if !is_lower_hex(challenge, 32) {
        return Err(ProtocolError::InvalidChallenge);
    }
    field(generation != 0)
}

fn validate_id(id: u32) -> Result<(), ProtocolError> {
    if id == 0 {
        Err(ProtocolError::InvalidId)
    } else {
        Ok(())
    }
}

fn field(valid: bool) -> Result<(), ProtocolError> {
    if valid {
        Ok(())
    } else {
        Err(ProtocolError::InvalidComposeField)
    }
}

/// True when a frame's `type` tag belongs to the Compose session.
pub(crate) fn is_compose_tag(tag: &str) -> bool {
    tag.starts_with("Compose")
}
