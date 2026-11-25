//! Compose session frames. They share the v1 framing and 32 KiB ceiling with
//! the legacy messages but are decoded only after a pinned ComposeHello on a
//! guest that serves Compose. Every tag starts with `Compose`.
//!
//! No reply carries a path, URL or host instruction. Replies hold opaque IDs
//! the host already sent, digests, counts and closed enums only, so nothing a
//! guest says can authorize a host read, write or fetch.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, de};

use super::codec::{SecretText, is_base64_text, is_lower_hex};
use super::manifest::{ManifestRule, is_project_id, is_slot_name};
use crate::compose_backup::{
    BACKUP_CHUNK_BYTES, MAX_BACKUP_BYTES, MAX_BACKUP_CHUNKS, MAX_BACKUP_DEADLINE_S, is_db_basename,
    is_volume_slot,
};
use crate::protocol::{Message, ProtocolError, private};

/// Version of the Compose session. Pinned separately from the legacy frame
/// version (which stays at 1) and bumped to v3 for protected SQLite backups
/// (leg E4).
pub const COMPOSE_PROTOCOL_VERSION: u32 = 3;
/// The closed, sorted capability set. A ComposeHello must name exactly this
/// list; anything else closes the connection.
pub const COMPOSE_CAPABILITIES: [&str; 6] = [
    "backup",
    "lifecycle",
    "logs",
    "manifest",
    "secrets",
    "status",
];
/// Decoded bytes per staged manifest chunk. Only the last chunk is shorter.
pub const STAGE_CHUNK: usize = 16 * 1024;
/// Decoded bytes per staged manifest.
pub const STAGE_TOTAL: usize = 128 * 1024;
/// Decoded bytes per secret value.
pub const MAX_SECRET: usize = 4096;
pub const MAX_LOG_ENTRIES: u32 = 32;
/// Serialized bytes of one log page reply.
pub const MAX_LOG_PAGE: usize = 8 * 1024;

#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type")]
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
    ComposeInstallRoutes {
        id: u32,
        generation: u64,
        readiness_host: String,
        zone: String,
        routes: Vec<super::routing::RouteTarget>,
    },
    /// Private, non-logging secret delivery bound to an exact release digest.
    /// `data` is Base64 and is wiped when the request is dropped.
    ComposeSecretPut {
        id: u32,
        operation_id: String,
        generation: u64,
        project_id: String,
        digest: String,
        slot: String,
        epoch: u64,
        data: SecretText,
    },
    /// Begins a consistent SQLite online backup of an enrolled project volume
    /// into root-owned guest staging under the project's maintenance lease.
    ComposeBackupBegin {
        id: u32,
        operation_id: String,
        generation: u64,
        project_id: String,
        volume: String,
        database: String,
        max_bytes: u64,
        deadline_s: u32,
    },
    /// Queries verified metadata (size, chunk count, SHA-256 digest) of a
    /// succeeded staged backup. Never carries any filesystem path.
    ComposeBackupMeta {
        id: u32,
        operation_id: String,
    },
    /// Reads one exact 16 KiB decoded chunk of a staged backup by index.
    ComposeBackupChunk {
        id: u32,
        operation_id: String,
        index: u32,
    },
    /// Releases guest staging for a backup operation after host settlement or
    /// host-side cancellation. Never touches project volumes or host backups.
    ComposeBackupRelease {
        id: u32,
        operation_id: String,
    },
}

// Hand-written Deserialize avoids serde's internally-tagged `Content` buffer so
// secret fields decode directly from a borrowed slice into the wiping
// `SecretText` type and are wiped even on early rejection.
impl<'de> Deserialize<'de> for ComposeRequest {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(field_identifier, rename_all = "snake_case")]
        enum Field {
            Type,
            Version,
            Challenge,
            Generation,
            Capabilities,
            Id,
            Digest,
            TotalBytes,
            Chunks,
            Index,
            Data,
            OperationId,
            ProjectId,
            Revision,
            Cursor,
            Limit,
            Slot,
            Epoch,
            ReadinessHost,
            Zone,
            Routes,
            Volume,
            Database,
            MaxBytes,
            DeadlineS,
        }

        struct RequestVisitor;

        impl<'de> de::Visitor<'de> for RequestVisitor {
            type Value = ComposeRequest;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a ComposeRequest object")
            }

            fn visit_map<A: de::MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut tag: Option<String> = None;
                let mut version: Option<u32> = None;
                let mut challenge: Option<String> = None;
                let mut generation: Option<u64> = None;
                let mut capabilities: Option<Vec<String>> = None;
                let mut id: Option<u32> = None;
                let mut digest: Option<String> = None;
                let mut total_bytes: Option<u32> = None;
                let mut chunks: Option<u32> = None;
                let mut index: Option<u32> = None;
                let mut data: Option<SecretText> = None;
                let mut operation_id: Option<String> = None;
                let mut project_id: Option<String> = None;
                let mut revision: Option<u64> = None;
                let mut cursor: Option<u64> = None;
                let mut limit: Option<u32> = None;
                let mut slot: Option<String> = None;
                let mut readiness_host = None;
                let mut zone = None;
                let mut routes = None;
                let mut epoch: Option<u64> = None;
                let mut volume: Option<String> = None;
                let mut database: Option<String> = None;
                let mut max_bytes: Option<u64> = None;
                let mut deadline_s: Option<u32> = None;

                macro_rules! set_once {
                    ($slot:ident, $name:literal) => {{
                        if $slot.is_some() {
                            return Err(de::Error::duplicate_field($name));
                        }
                        $slot = Some(map.next_value()?);
                    }};
                }

                while let Some(field) = map.next_key::<Field>()? {
                    match field {
                        Field::Type => set_once!(tag, "type"),
                        Field::Version => set_once!(version, "version"),
                        Field::Challenge => set_once!(challenge, "challenge"),
                        Field::Generation => set_once!(generation, "generation"),
                        Field::Capabilities => set_once!(capabilities, "capabilities"),
                        Field::Id => set_once!(id, "id"),
                        Field::Digest => set_once!(digest, "digest"),
                        Field::TotalBytes => set_once!(total_bytes, "total_bytes"),
                        Field::Chunks => set_once!(chunks, "chunks"),
                        Field::Index => set_once!(index, "index"),
                        Field::Data => set_once!(data, "data"),
                        Field::OperationId => set_once!(operation_id, "operation_id"),
                        Field::ProjectId => set_once!(project_id, "project_id"),
                        Field::Revision => set_once!(revision, "revision"),
                        Field::Cursor => set_once!(cursor, "cursor"),
                        Field::Limit => set_once!(limit, "limit"),
                        Field::Slot => set_once!(slot, "slot"),
                        Field::ReadinessHost => set_once!(readiness_host, "readiness_host"),
                        Field::Zone => set_once!(zone, "zone"),
                        Field::Routes => set_once!(routes, "routes"),
                        Field::Epoch => set_once!(epoch, "epoch"),
                        Field::Volume => set_once!(volume, "volume"),
                        Field::Database => set_once!(database, "database"),
                        Field::MaxBytes => set_once!(max_bytes, "max_bytes"),
                        Field::DeadlineS => set_once!(deadline_s, "deadline_s"),
                    }
                }

                macro_rules! take {
                    ($slot:ident, $name:literal) => {
                        $slot
                            .take()
                            .ok_or_else(|| de::Error::missing_field($name))?
                    };
                }

                let tag = tag.ok_or_else(|| de::Error::missing_field("type"))?;
                let request = match tag.as_str() {
                    "ComposeInstallRoutes" => ComposeRequest::ComposeInstallRoutes {
                        id: take!(id, "id"),
                        generation: take!(generation, "generation"),
                        readiness_host: take!(readiness_host, "readiness_host"),
                        zone: take!(zone, "zone"),
                        routes: take!(routes, "routes"),
                    },
                    "ComposeHello" => ComposeRequest::ComposeHello {
                        version: take!(version, "version"),
                        challenge: take!(challenge, "challenge"),
                        generation: take!(generation, "generation"),
                        capabilities: take!(capabilities, "capabilities"),
                    },
                    "ComposeStageBegin" => ComposeRequest::ComposeStageBegin {
                        id: take!(id, "id"),
                        digest: take!(digest, "digest"),
                        total_bytes: take!(total_bytes, "total_bytes"),
                        chunks: take!(chunks, "chunks"),
                    },
                    "ComposeStageChunk" => ComposeRequest::ComposeStageChunk {
                        id: take!(id, "id"),
                        digest: take!(digest, "digest"),
                        index: take!(index, "index"),
                        data: take!(data, "data").into_non_secret_string(),
                    },
                    "ComposeStageCommit" => ComposeRequest::ComposeStageCommit {
                        id: take!(id, "id"),
                        digest: take!(digest, "digest"),
                    },
                    "ComposeDeploy" => ComposeRequest::ComposeDeploy {
                        id: take!(id, "id"),
                        operation_id: take!(operation_id, "operation_id"),
                        project_id: take!(project_id, "project_id"),
                        revision: take!(revision, "revision"),
                        digest: take!(digest, "digest"),
                    },
                    "ComposeStart" => ComposeRequest::ComposeStart {
                        id: take!(id, "id"),
                        operation_id: take!(operation_id, "operation_id"),
                        project_id: take!(project_id, "project_id"),
                    },
                    "ComposeStop" => ComposeRequest::ComposeStop {
                        id: take!(id, "id"),
                        operation_id: take!(operation_id, "operation_id"),
                        project_id: take!(project_id, "project_id"),
                    },
                    "ComposeDown" => ComposeRequest::ComposeDown {
                        id: take!(id, "id"),
                        operation_id: take!(operation_id, "operation_id"),
                        project_id: take!(project_id, "project_id"),
                    },
                    "ComposeOperation" => ComposeRequest::ComposeOperation {
                        id: take!(id, "id"),
                        operation_id: take!(operation_id, "operation_id"),
                    },
                    "ComposeReconcile" => ComposeRequest::ComposeReconcile {
                        id: take!(id, "id"),
                        operation_id: take!(operation_id, "operation_id"),
                    },
                    "ComposeStatus" => ComposeRequest::ComposeStatus {
                        id: take!(id, "id"),
                        project_id: take!(project_id, "project_id"),
                    },
                    "ComposeLogs" => ComposeRequest::ComposeLogs {
                        id: take!(id, "id"),
                        project_id: take!(project_id, "project_id"),
                        cursor: take!(cursor, "cursor"),
                        limit: take!(limit, "limit"),
                    },
                    "ComposeSecretPut" => ComposeRequest::ComposeSecretPut {
                        id: take!(id, "id"),
                        operation_id: take!(operation_id, "operation_id"),
                        generation: take!(generation, "generation"),
                        project_id: take!(project_id, "project_id"),
                        digest: take!(digest, "digest"),
                        slot: take!(slot, "slot"),
                        epoch: take!(epoch, "epoch"),
                        data: take!(data, "data"),
                    },
                    "ComposeBackupBegin" => ComposeRequest::ComposeBackupBegin {
                        id: take!(id, "id"),
                        operation_id: take!(operation_id, "operation_id"),
                        generation: take!(generation, "generation"),
                        project_id: take!(project_id, "project_id"),
                        volume: take!(volume, "volume"),
                        database: take!(database, "database"),
                        max_bytes: take!(max_bytes, "max_bytes"),
                        deadline_s: take!(deadline_s, "deadline_s"),
                    },
                    "ComposeBackupMeta" => ComposeRequest::ComposeBackupMeta {
                        id: take!(id, "id"),
                        operation_id: take!(operation_id, "operation_id"),
                    },
                    "ComposeBackupChunk" => ComposeRequest::ComposeBackupChunk {
                        id: take!(id, "id"),
                        operation_id: take!(operation_id, "operation_id"),
                        index: take!(index, "index"),
                    },
                    "ComposeBackupRelease" => ComposeRequest::ComposeBackupRelease {
                        id: take!(id, "id"),
                        operation_id: take!(operation_id, "operation_id"),
                    },
                    _ => return Err(de::Error::custom("unknown ComposeRequest type")),
                };

                if version.is_some()
                    || challenge.is_some()
                    || generation.is_some()
                    || capabilities.is_some()
                    || id.is_some()
                    || digest.is_some()
                    || total_bytes.is_some()
                    || chunks.is_some()
                    || index.is_some()
                    || data.is_some()
                    || operation_id.is_some()
                    || project_id.is_some()
                    || revision.is_some()
                    || cursor.is_some()
                    || limit.is_some()
                    || slot.is_some()
                    || readiness_host.is_some()
                    || zone.is_some()
                    || routes.is_some()
                    || epoch.is_some()
                    || volume.is_some()
                    || database.is_some()
                    || max_bytes.is_some()
                    || deadline_s.is_some()
                {
                    return Err(de::Error::custom("unexpected field for ComposeRequest"));
                }
                Ok(request)
            }
        }

        deserializer.deserialize_map(RequestVisitor)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum ComposeResponse {
    ComposeRoutesInstalled {
        id: u32,
        generation: u64,
    },
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
        digest: String,
        slot: String,
        epoch: u64,
        recreate: Option<OperationView>,
    },
    ComposeBackupReady {
        id: u32,
        operation_id: String,
        project_id: String,
        revision: u64,
        total_bytes: u64,
        chunks: u32,
        digest: String,
    },
    ComposeBackupData {
        id: u32,
        operation_id: String,
        index: u32,
        data: String,
    },
    ComposeBackupReleased {
        id: u32,
        operation_id: String,
        removed: bool,
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
    SecretConflict,
    SecretSize,
    SecretMissing,
    SecretOwnerMismatch,
    SecretWrite,
    StoreFailure,
    EngineUnavailable,
    JournalFull,
    UnknownVolume,
    InvalidDatabase,
    BackupNotReady,
    BackupChunkOutOfRange,
    BackupStagingOccupied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    Deploy,
    Start,
    Stop,
    Down,
    SecretPut,
    Recreate,
    Backup,
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
    BackingUp,
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
    BackupNotFound,
    BackupFailed,
    BackupTimedOut,
    BackupTooLarge,
    BackupCorrupt,
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
    BackupStarted,
    BackupStaged,
    BackupReleased,
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
            Self::ComposeInstallRoutes {
                id,
                generation,
                readiness_host,
                zone,
                routes,
            } => {
                validate_id(*id)?;
                field(
                    *generation != 0
                        && super::routing::RoutingTable::new(
                            readiness_host,
                            Some(zone.clone()),
                            super::routing::SLIRP_FORWARD_PEER,
                            routes.clone(),
                        )
                        .is_ok(),
                )
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
                digest,
                slot,
                epoch,
                data,
            } => {
                validate_id(*id)?;
                field(
                    is_lower_hex(operation_id, 32)
                        && *generation != 0
                        && is_project_id(project_id)
                        && is_lower_hex(digest, 64)
                        && is_slot_name(slot)
                        && *epoch != 0
                        && is_base64_text(data.as_str(), MAX_SECRET),
                )
            }
            Self::ComposeBackupBegin {
                id,
                operation_id,
                generation,
                project_id,
                volume,
                database,
                max_bytes,
                deadline_s,
            } => {
                validate_id(*id)?;
                field(
                    is_lower_hex(operation_id, 32)
                        && *generation != 0
                        && is_project_id(project_id)
                        && is_volume_slot(volume)
                        && is_db_basename(database)
                        && (1..=MAX_BACKUP_BYTES).contains(max_bytes)
                        && (1..=MAX_BACKUP_DEADLINE_S).contains(deadline_s),
                )
            }
            Self::ComposeBackupMeta { id, operation_id }
            | Self::ComposeBackupRelease { id, operation_id } => {
                validate_id(*id)?;
                field(is_lower_hex(operation_id, 32))
            }
            Self::ComposeBackupChunk {
                id,
                operation_id,
                index,
            } => {
                validate_id(*id)?;
                field(is_lower_hex(operation_id, 32) && *index < MAX_BACKUP_CHUNKS)
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
            Self::ComposeRoutesInstalled { id, generation } => {
                validate_id(*id)?;
                field(*generation != 0)
            }
            Self::ComposeBackupReady {
                id,
                operation_id,
                project_id,
                revision,
                total_bytes,
                chunks,
                digest,
            } => {
                validate_id(*id)?;
                field(
                    is_lower_hex(operation_id, 32)
                        && is_project_id(project_id)
                        && *revision != 0
                        && (1..=MAX_BACKUP_BYTES).contains(total_bytes)
                        && crate::compose_backup::chunk_count(*total_bytes) == Some(*chunks)
                        && is_lower_hex(digest, 64),
                )
            }
            Self::ComposeBackupData {
                id,
                operation_id,
                index,
                data,
            } => {
                validate_id(*id)?;
                field(
                    is_lower_hex(operation_id, 32)
                        && *index < MAX_BACKUP_CHUNKS
                        && is_base64_text(data, BACKUP_CHUNK_BYTES),
                )
            }
            Self::ComposeBackupReleased {
                id, operation_id, ..
            } => {
                validate_id(*id)?;
                field(is_lower_hex(operation_id, 32))
            }
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
